//! AST → bytecode compiler for the VM.
//!
//! The compiler walks the existing AST and emits bytecode instructions
//! into a [`Chunk`]. Slot indices from `compute_slots` are reused directly
//! from `Ident.slot` for O(1) variable access.
//!
//! # Module structure
//!
//! The compiler is split across several files to keep each manageable:
//!
//! - `mod.rs` — [`Compiler`] struct, entry points, scope/loop tracking
//! - `expressions.rs` — literals, operators, identifiers, method calls
//! - `statements.rs` — let, assign, return, import, expression statements
//! - `control_flow.rs` — if/else, while, for-in, c-style for, break, continue
//! - `functions.rs` — fn declarations, calls, closures, async, await
//! - `exceptions.rs` — try/catch/finally, throw
//! - `collections.rs` — arrays, hashes, indexing, struct literals

pub(crate) mod collections;
pub(crate) mod compilation_errors;
pub(crate) mod compute_slots;
pub(crate) mod control_flow;
pub(crate) mod exceptions;
pub(crate) mod expressions;
pub(crate) mod functions;
pub(crate) mod statements;

use crate::ast::ast::{Expr, Ident, Program, Stmt};
use crate::vm::obj::Object;
use crate::vm::chunk::Chunk;
use crate::vm::compiler::compute_slots::compute_slots;
use crate::vm::compiler::compilation_errors::CompilationError;
use crate::vm::instruction::Instruction;
use ahash::AHasher;
use std::hash::BuildHasherDefault;

type HashMap<K, V> = std::collections::HashMap<K, V, BuildHasherDefault<AHasher>>;

/// A forward-jump placeholder that needs backpatching once the target
/// address is known.
///
/// Stores the byte offset within the chunk's `code` vector where the
/// u16 operand should be written.
#[derive(Debug, Clone, Copy)]
struct JumpPatch {
    /// Byte offset in `chunk.code` where the u16 operand begins.
    addr: usize,
}

/// Tracks loop boundaries for `break` and `continue` compilation.
#[derive(Debug, Clone)]
struct LoopContext {
    /// Addresses of `Break` instructions to backpatch to the loop exit.
    break_patches: Vec<JumpPatch>,
    /// Addresses of `Continue` instructions to backpatch to the loop condition.
    continue_patches: Vec<JumpPatch>,
}

/// The compiler translates an AST [`Program`] into a [`Chunk`] of bytecode.
///
/// It maintains a single output chunk and uses jump-patch lists for
/// forward references (if/else, loops, short-circuit operators).
pub(crate) struct Compiler {
    chunk: Chunk,
    loop_contexts: Vec<LoopContext>,
    finally_depth: usize,
    error: Option<CompilationError>,
    struct_templates: HashMap<String, Object>,
}

impl Compiler {
    /// Compiles a program into a bytecode chunk.
    ///
    /// Runs `compute_slots` on the program to populate slot indices on every `Ident`.
    /// The program is passed by mutable reference to avoid cloning the entire AST.
    pub fn compile_program(program: &mut Program) -> Result<Chunk, CompilationError> {
        compute_slots(program);

        let mut compiler = Compiler {
            chunk: Chunk::new(),
            loop_contexts: Vec::new(),
            finally_depth: 0,
            error: None,
            struct_templates: HashMap::default(),
        };

        compiler.compile_program_body(program, false);

        if let Some(err) = compiler.error.take() {
            Err(err)
        } else {
            Ok(compiler.chunk)
        }
    }

    /// Compiles a function body into a sub-chunk.
    ///
    /// Returns the compiled chunk, parameter count, and local variable names
    /// indexed by slot (params first, then lets).
    pub fn compile_function_body(
        params: &[Ident],
        body: &Program,
        is_async: bool,
    ) -> (Chunk, usize, Vec<String>) {
        // Wrap body in a fake FnStmt so compute_slots assigns param slots correctly.
        let fake_fn = Stmt::FnStmt {
            name: Ident::new("".to_string()),
            params: params.to_vec(),
            body: body.clone(),
        };
        let mut wrapper_program = Program::new();
        wrapper_program.push(fake_fn);
        compute_slots(&mut wrapper_program);

        // Extract the body with assigned slots
        let program = match wrapper_program.pop().unwrap() {
            Stmt::FnStmt { body, .. } => body,
            _ => unreachable!(),
        };

        // Build local names array: params first (slot 0..N), then lets, then functions
        let mut local_names: Vec<String> = params.iter().map(|p| p.name.clone()).collect();
        for stmt in &program {
            match stmt {
                crate::ast::ast::Stmt::LetStmt(ident, _) => {
                    // Ensure the vector is large enough
                    while local_names.len() <= ident.slot.0 as usize {
                        local_names.push(String::new());
                    }
                    local_names[ident.slot.0 as usize] = ident.name.clone();
                }
                crate::ast::ast::Stmt::FnStmt { name, .. } => {
                    // Also track function declarations so they can be captured by inner closures
                    while local_names.len() <= name.slot.0 as usize {
                        local_names.push(String::new());
                    }
                    local_names[name.slot.0 as usize] = name.name.clone();
                }
                _ => {}
            }
        }

        let mut compiler = Compiler {
            chunk: Chunk::new(),
            loop_contexts: Vec::new(),
            finally_depth: 0,
            error: None,
            struct_templates: HashMap::default(),
        };

        compiler.compile_program_body(&program, false);

        // Always emit ReturnValue to ensure the stack is cleaned up correctly
        compiler.emit(Instruction::ReturnValue, 0);

        let param_count = params.len();

        if is_async {
            // For async functions, the body is compiled normally but will
            // be wrapped in an async call at the call site.
        }

        (compiler.chunk, param_count, local_names)
    }

    // ─── Program-level compilation ──────────────────────────────────

    pub fn compile_program_body(&mut self, program: &Program, discard_last: bool) {
        if program.is_empty() {
            if !discard_last {
                self.emit_constant(Object::Null, 0);
            }
            return;
        }

        for (i, stmt) in program.iter().enumerate() {
            let line = self.statement_line(stmt);
            self.compile_statement(stmt, line);

            let is_last = i == program.len() - 1;
            if !is_last || discard_last {
                match stmt {
                    Stmt::ExprStmt(_) | Stmt::ExprValueStmt(_) => {
                        self.emit(Instruction::Pop, line);
                    }
                    _ => {}
                }
            } else {
                // Last statement and we need its value
                match stmt {
                    Stmt::ExprStmt(_) | Stmt::ExprValueStmt(_) => {
                        // Keeps value on stack
                    }
                    _ => {
                        self.emit_constant(Object::Null, line);
                    }
                }
            }
        }
    }

    // ─── Statement dispatch ─────────────────────────────────────────

    fn compile_statement(&mut self, stmt: &Stmt, line: u16) {
        match stmt {
            Stmt::LetStmt(ident, expr) => {
                statements::compile_let_stmt(self, ident, expr, line);
            }
            Stmt::MultiLetStmt { idents, values } => {
                statements::compile_multi_let(self, idents, values, line);
            }
            Stmt::AssignStmt(ident, expr) => {
                statements::compile_assign(self, ident, expr, line);
            }
            Stmt::TupleAssignStmt { targets, values } => {
                statements::compile_tuple_assign(self, targets, values, line);
            }
            Stmt::FieldAssignStmt {
                object,
                field,
                value,
            } => {
                statements::compile_field_assign(self, object, field, value, line);
            }
            Stmt::IndexAssignStmt {
                target,
                index,
                value,
            } => {
                statements::compile_index_assign(self, target, index, value, line);
            }
            Stmt::ReturnStmt(expr) => {
                statements::compile_return_stmt(self, expr, line);
            }
            Stmt::ExprStmt(expr) => {
                self.compile_expression(expr, line);
            }
            Stmt::ExprValueStmt(expr) => {
                self.compile_expression(expr, line);
            }
            Stmt::FnStmt { name, params, body } => {
                functions::compile_fn_declaration(self, name, params, body, line);
            }
            Stmt::StructStmt {
                name,
                fields,
                methods,
            } => {
                collections::compile_struct_stmt(self, name, fields, methods, line);
            }
            Stmt::ImportStmt { path, items } => {
                statements::compile_import_stmt(self, path, items, line);
            }
            Stmt::BreakStmt => {
                control_flow::compile_break(self, line);
            }
            Stmt::ContinueStmt => {
                control_flow::compile_continue(self, line);
            }
            Stmt::ThrowStmt(expr) => {
                exceptions::compile_throw(self, expr, line);
            }
        }
    }

    // ─── Expression dispatch ────────────────────────────────────────

    fn compile_expression(&mut self, expr: &Expr, line: u16) {
        match expr {
            Expr::IdentExpr(ident) => {
                expressions::compile_ident(self, ident, line);
            }
            Expr::LitExpr(literal) => {
                expressions::compile_literal(self, literal, line);
            }
            Expr::PrefixExpr(op, operand) => {
                expressions::compile_prefix(self, op, operand, line);
            }
            Expr::InfixExpr(op, left, right) => {
                expressions::compile_infix(self, op, left, right, line);
            }
            Expr::IfExpr {
                cond,
                consequence,
                alternative,
            } => {
                control_flow::compile_if_expr(self, cond, consequence, alternative, line);
            }
            Expr::FnExpr { params, body } => {
                functions::compile_fn_expr(self, params, body, false, line);
            }
            Expr::CallExpr {
                function,
                arguments,
            } => {
                functions::compile_call_expr(self, function, arguments, line);
            }
            Expr::ArrayExpr(elements) => {
                collections::compile_array_expr(self, elements, line);
            }
            Expr::HashExpr(pairs) => {
                collections::compile_hash_expr(self, pairs, line);
            }
            Expr::IndexExpr { array, index } => {
                collections::compile_index_expr(self, array, index, line);
            }
            Expr::MethodCallExpr {
                object,
                method,
                arguments,
            } => {
                collections::compile_method_call(self, object, method, arguments, line);
            }
            Expr::StructLiteral { name, fields } => {
                collections::compile_struct_literal(self, name, fields, line);
            }
            Expr::ThisExpr => {
                expressions::compile_this_expr(self, line);
            }
            Expr::FieldAccessExpr { object, field } => {
                collections::compile_field_access(self, object, field, line);
            }
            Expr::WhileExpr { cond, body } => {
                control_flow::compile_while_expr(self, cond, body, line);
            }
            Expr::ForExpr {
                ident,
                iterable,
                body,
            } => {
                control_flow::compile_for_expr(self, ident, iterable, body, line);
            }
            Expr::CStyleForExpr {
                init,
                cond,
                update,
                body,
            } => {
                control_flow::compile_cstyle_for(self, init, cond, update, body, line);
            }
            Expr::TryCatchExpr {
                try_body,
                catch_ident,
                catch_body,
                finally_body,
            } => {
                exceptions::compile_try_catch(
                    self,
                    try_body,
                    catch_ident,
                    catch_body,
                    finally_body,
                    line,
                );
            }
            Expr::AsyncFnExpr { params, body } => {
                functions::compile_fn_expr(self, params, body, true, line);
            }
            Expr::AwaitExpr(expr) => {
                functions::compile_await_expr(self, expr, line);
            }
            Expr::MatchExpr { value, arms } => {
                control_flow::compile_match_expr(self, value, arms, line);
            }
        }
    }

    // ─── Code emission ──────────────────────────────────────────────

    /// Emits a single instruction and returns the byte offset where
    /// its first operand byte was written (for backpatching).
    fn emit(&mut self, instr: Instruction, line: u16) -> usize {
        let offset = self.chunk.code.len();
        self.chunk.write_instruction(instr, line);
        offset
    }

    /// Adds a constant to the chunk's pool and emits `OpConstant`.
    fn emit_constant(&mut self, value: Object, line: u16) {
        if let Some(idx) = self.chunk.add_constant(value) {
            self.emit(Instruction::Constant(idx), line);
        } else {
            self.error = Some(CompilationError::ConstantPoolOverflow);
        }
    }

    /// Emits a forward jump instruction and records its patch address.
    fn emit_jump(&mut self, line: u16) -> JumpPatch {
        let addr = self.chunk.code.len();
        self.emit(Instruction::Jump(0), line);
        JumpPatch { addr: addr + 1 } // +1 to skip opcode byte
    }

    /// Emits a `JumpIfFalse` and records its patch address.
    fn emit_jump_if_false(&mut self, line: u16) -> JumpPatch {
        let addr = self.chunk.code.len();
        self.emit(Instruction::JumpIfFalse(0), line);
        JumpPatch { addr: addr + 1 }
    }

    /// Emits a `PopJumpIfFalse` and records its patch address.
    fn emit_pop_jump_if_false(&mut self, line: u16) -> JumpPatch {
        let addr = self.chunk.code.len();
        self.emit(Instruction::PopJumpIfFalse(0), line);
        JumpPatch { addr: addr + 1 }
    }

    /// Emits a `JumpIfTruthy` and records its patch address.
    fn emit_jump_if_truthy(&mut self, line: u16) -> JumpPatch {
        let addr = self.chunk.code.len();
        self.emit(Instruction::JumpIfTruthy(0), line);
        JumpPatch { addr: addr + 1 }
    }

    /// Patches a previously emitted jump instruction with the current offset.
    fn patch_jump(&mut self, patch: JumpPatch) {
        let offset = self.chunk.current_offset();
        self.chunk.patch_u16(patch.addr, offset);
    }

    // ─── Line number extraction ─────────────────────────────────────
    // The current parser doesn't track line numbers in AST nodes,
    // so we use 0 for now. This will be updated once the parser
    // adds span information.

    fn statement_line(&self, _stmt: &Stmt) -> u16 {
        0
    }
}
