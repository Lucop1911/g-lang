//! Control flow compilation: if/else, while, for-in, c-style for, break, continue.

use crate::ast::ast::{Expr, Ident, Pattern, Program, Stmt};
use crate::ast::ast::Literal;
use crate::vm::compiler::{Compiler, JumpPatch, LoopContext};
use crate::vm::instruction::Instruction;
use crate::vm::obj::Object;

/// Compiles an if/else expression.
pub(crate) fn compile_if_expr(
    compiler: &mut Compiler,
    cond: &Expr,
    consequence: &Program,
    alternative: &Option<Program>,
    line: u16,
) {
    compiler.compile_expression(cond, line);
    let else_jump = compiler.emit_pop_jump_if_false(line);

    compiler.compile_program_body(consequence, false);

    if let Some(alt) = alternative {
        let end_jump = compiler.emit_jump(line);
        compiler.patch_jump(else_jump);
        compiler.compile_program_body(alt, false);
        compiler.patch_jump(end_jump);
    } else {
        let end_jump = compiler.emit_jump(line);
        compiler.patch_jump(else_jump);
        compiler.emit_constant(crate::vm::obj::Object::Null, line);
        compiler.patch_jump(end_jump);
    }
}

/// Compiles a while loop expression.
pub(crate) fn compile_while_expr(compiler: &mut Compiler, cond: &Expr, body: &Program, line: u16) {
    let loop_start = compiler.chunk.current_offset();

    compiler.loop_contexts.push(LoopContext {
        break_patches: Vec::new(),
        continue_patches: Vec::new(),
    });

    compiler.compile_expression(cond, line);
    let end_jump = compiler.emit_pop_jump_if_false(line);

    compiler.compile_program_body(body, true);

    compiler.emit(Instruction::JumpBackward(loop_start), line);

    let loop_ctx = compiler.loop_contexts.pop().unwrap();
    let continue_addr = loop_start;
    let break_addr = compiler.chunk.current_offset();

    for patch in loop_ctx.continue_patches {
        compiler.chunk.patch_u16(patch.addr, continue_addr);
    }

    for patch in loop_ctx.break_patches {
        compiler.chunk.patch_u16(patch.addr, break_addr);
    }

    compiler
        .chunk
        .patch_u16(end_jump.addr, compiler.chunk.current_offset());

    compiler.emit_constant(crate::vm::obj::Object::Null, line);
}

/// Compiles a for-in loop expression.
pub(crate) fn compile_for_expr(
    compiler: &mut Compiler,
    idents: &[Ident],
    iterable: &Expr,
    body: &Program,
    line: u16,
) {
    use crate::vm::obj::Object;

    // Allocate slots for the iterable and counter AFTER the ident slots.
    // The ident slots are assigned by compute_slots.
    let base_slot = if !idents.is_empty() && idents[0].slot != crate::ast::ast::SlotIndex::UNSET {
        idents[0].slot.0 as u8
    } else {
        0
    };
    let iter_slot = base_slot + idents.len() as u8;
    let counter_slot = iter_slot + 1;

    compiler.compile_expression(iterable, line);
    compiler.emit(Instruction::SetLocal(iter_slot), line);

    compiler.emit_constant(Object::Integer(0), line);
    compiler.emit(Instruction::SetLocal(counter_slot), line);

    let loop_start = compiler.chunk.current_offset();

    compiler.loop_contexts.push(LoopContext {
        break_patches: Vec::new(),
        continue_patches: Vec::new(),
    });

    compiler.emit(Instruction::GetLocal(counter_slot), line);
    compiler.emit(Instruction::GetLocal(iter_slot), line);
    compiler.emit(Instruction::GetLen, line);
    compiler.emit(Instruction::LessThan, line);
    let end_jump = compiler.emit_pop_jump_if_false(line);

    compiler.emit(Instruction::GetLocal(iter_slot), line);
    compiler.emit(Instruction::GetLocal(counter_slot), line);
    compiler.emit(Instruction::Index, line);

    if idents.len() == 1 {
        let ident = &idents[0];
        if ident.slot != crate::ast::ast::SlotIndex::UNSET {
            compiler.emit(Instruction::SetLocal(ident.slot.0 as u8), line);
        }
    } else {
        for (i, ident) in idents.iter().enumerate() {
            compiler.emit(Instruction::Dup, line);
            compiler.emit_constant(Object::Integer(i as i64), line);
            compiler.emit(Instruction::Index, line);
            if ident.slot != crate::ast::ast::SlotIndex::UNSET {
                compiler.emit(Instruction::SetLocal(ident.slot.0 as u8), line);
            }
        }
        compiler.emit(Instruction::Pop, line);
    }

    compile_program_body(compiler, body, true);

    let continue_addr = compiler.chunk.current_offset();

    compiler.emit(Instruction::GetLocal(counter_slot), line);
    compiler.emit_constant(Object::Integer(1), line);
    compiler.emit(Instruction::Add, line);
    compiler.emit(Instruction::SetLocal(counter_slot), line);

    compiler.emit(Instruction::JumpBackward(loop_start), line);

    let break_addr = compiler.chunk.current_offset();

    let loop_ctx = compiler.loop_contexts.pop().unwrap();
    for patch in loop_ctx.continue_patches {
        compiler.chunk.patch_u16(patch.addr, continue_addr);
    }

    for patch in loop_ctx.break_patches {
        compiler.chunk.patch_u16(patch.addr, break_addr);
    }

    compiler
        .chunk
        .patch_u16(end_jump.addr, compiler.chunk.current_offset());

    compiler.emit_constant(Object::Null, line);
}

/// Compiles a C-style for loop.
pub(crate) fn compile_cstyle_for(
    compiler: &mut Compiler,
    init: &Option<Box<Stmt>>,
    cond: &Option<Box<Expr>>,
    update: &Option<Box<Stmt>>,
    body: &Program,
    line: u16,
) {
    use crate::vm::obj::Object;

    if let Some(init_stmt) = init {
        compiler.compile_statement(init_stmt, line);
    }

    let cond_start = compiler.chunk.current_offset();

    compiler.loop_contexts.push(LoopContext {
        break_patches: Vec::new(),
        continue_patches: Vec::new(),
    });

    if let Some(cond_expr) = cond {
        compiler.compile_expression(cond_expr, line);
    } else {
        compiler.emit_constant(Object::Boolean(true), line);
    }
    let end_jump = compiler.emit_pop_jump_if_false(line);

    compile_program_body(compiler, body, true);

    let continue_addr = compiler.chunk.current_offset();

    if let Some(update_stmt) = update {
        compiler.compile_statement(update_stmt, line);
    }

    compiler.emit(Instruction::JumpBackward(cond_start), line);

    compiler
        .chunk
        .patch_u16(end_jump.addr, compiler.chunk.current_offset());

    let loop_ctx = compiler.loop_contexts.pop().unwrap();
    for patch in loop_ctx.continue_patches {
        compiler.chunk.patch_u16(patch.addr, continue_addr);
    }

    for patch in loop_ctx.break_patches {
        compiler
            .chunk
            .patch_u16(patch.addr, compiler.chunk.current_offset());
    }

    compiler.emit_constant(Object::Null, line);
}

/// Compiles a match expression.
pub(crate) fn compile_match_expr(
    compiler: &mut Compiler,
    value: &Expr,
    arms: &[(Pattern, Program)],
    line: u16,
) {
    let mut end_jumps: Vec<JumpPatch> = Vec::new();

    compiler.compile_expression(value, line);

    for (i, (pattern, body)) in arms.iter().enumerate() {
        match pattern {
            Pattern::Literal(lit) => {
                compiler.emit(Instruction::Dup, line);
                let lit_obj = match lit {
                    Literal::IntLiteral(n) => Object::Integer(*n),
                    Literal::BigIntLiteral(b) => Object::BigInteger(Box::new(b.clone())),
                    Literal::FloatLiteral(f) => Object::Float(*f),
                    Literal::BoolLiteral(b) => Object::Boolean(*b),
                    Literal::StringLiteral(s) => Object::String(s.clone()),
                    Literal::NullLiteral => Object::Null,
                };
                compiler.emit_constant(lit_obj, line);
                compiler.emit(Instruction::Equal, line);
                let next_arm = compiler.emit_pop_jump_if_false(line);

                compiler.emit(Instruction::Pop, line);
                compile_program_body(compiler, body, false);

                if i < arms.len() - 1 {
                    end_jumps.push(compiler.emit_jump(line));
                }
                compiler.patch_jump(next_arm);
            }
            Pattern::Wildcard => {
                compiler.emit(Instruction::Pop, line);
                compile_program_body(compiler, body, false);
                if i < arms.len() - 1 {
                    end_jumps.push(compiler.emit_jump(line));
                }
            }
        }
    }

    for jump in end_jumps {
        compiler.patch_jump(jump);
    }
}
pub(crate) fn compile_break(compiler: &mut Compiler, line: u16) {
    if compiler.loop_contexts.is_empty() {
        return;
    }
    let offset = compiler.chunk.code.len();
    compiler.emit(Instruction::Break(0), line);
    compiler
        .loop_contexts
        .last_mut()
        .unwrap()
        .break_patches
        .push(JumpPatch { addr: offset + 1 });
}

/// Emits a `continue` instruction and records it for backpatching.
pub(crate) fn compile_continue(compiler: &mut Compiler, line: u16) {
    if compiler.loop_contexts.is_empty() {
        return;
    }
    let offset = compiler.chunk.code.len();
    compiler.emit(Instruction::Continue(0), line);
    compiler
        .loop_contexts
        .last_mut()
        .unwrap()
        .continue_patches
        .push(JumpPatch { addr: offset + 1 });
}

/// Compiles a sequence of statements for block bodies.
/// Pops intermediate expression results. If `discard_last` is true, also pops the last result.
/// If `discard_last` is false and the last statement was not an expression, pushes `Null`.
fn compile_program_body(compiler: &mut Compiler, program: &Program, discard_last: bool) {
    use crate::vm::obj::Object;

    if program.is_empty() {
        if !discard_last {
            compiler.emit_constant(Object::Null, 0);
        }
        return;
    }

    for (i, stmt) in program.iter().enumerate() {
        let line = 0u16;
        compiler.compile_statement(stmt, line);

        let is_last = i == program.len() - 1;
        if !is_last || discard_last {
            match stmt {
                Stmt::ExprStmt(_) | Stmt::ExprValueStmt(_) => {
                    compiler.emit(Instruction::Pop, line);
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
                    compiler.emit_constant(Object::Null, line);
                }
            }
        }
    }
}
