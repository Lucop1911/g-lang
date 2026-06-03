//! Function compilation: declarations, calls, closures, async, await.

use crate::ast::ast::{Expr, Ident, Program, SlotIndex};
use crate::vm::obj::{FunctionData, Object};
use crate::vm::compiler::Compiler;
use crate::vm::instruction::Instruction;
use crate::vm::runtime::env::Environment;

/// Compiles a function declaration: `fn name(params) { body }`.
pub fn compile_fn_declaration(
    compiler: &mut Compiler,
    name: &Ident,
    params: &[Ident],
    body: &Program,
    line: u16,
) {
    compile_closure_instruction(compiler, params, body, line);
    // Stack: [Function]

    // Dup so we can store in both locations
    compiler.emit(Instruction::Dup, line);
    // Stack: [Function, Function]

    // Store in primary location (local slot or global)
    if name.slot != SlotIndex::UNSET {
        compiler.emit(Instruction::SetLocal(name.slot.0 as u8), line);
    } else {
        let name_idx = compiler
            .chunk
            .add_constant(Object::String(name.name.clone()));
        if let Some(name_idx) = name_idx {
            compiler.emit(Instruction::SetGlobal(name_idx), line);
        }
    }
    // Stack: [Function]

    // Also store as global for recursive/self-referential calls
    let name_idx = compiler
        .chunk
        .add_constant(Object::String(name.name.clone()));
    if let Some(name_idx) = name_idx {
        compiler.emit(Instruction::SetGlobal(name_idx), line);
    }
    // Stack: []
}

/// Compiles a function expression: `fn(params) { body }`.
pub fn compile_fn_expr(
    compiler: &mut Compiler,
    params: &[Ident],
    body: &Program,
    is_async: bool,
    line: u16,
) {
    if is_async {
        compile_async_closure(compiler, params, body, line);
    } else {
        compile_closure_instruction(compiler, params, body, line);
    }
}

/// Compiles a function call expression: `fn(arg1, arg2, ...)`.
pub fn compile_call_expr(compiler: &mut Compiler, function: &Expr, arguments: &[Expr], line: u16) {
    compiler.compile_expression(function, line);

    for arg in arguments {
        compiler.compile_expression(arg, line);
    }

    let arg_count = arguments.len() as u8;
    compiler.emit(Instruction::Call(arg_count), line);
}

/// Compiles an `await` expression: `await expr`.
pub fn compile_await_expr(compiler: &mut Compiler, expr: &Expr, line: u16) {
    compiler.compile_expression(expr, line);
    compiler.emit(Instruction::Await, line);
}

fn compile_closure_instruction(
    compiler: &mut Compiler,
    params: &[Ident],
    body: &Program,
    line: u16,
) {
    let (chunk, _param_count, local_names) = Compiler::compile_function_body(params, body, false);
    let fn_obj = Object::Function(Box::new(FunctionData {
        params: params.to_vec(),
        chunk: std::sync::Arc::new(chunk),
        env: std::sync::Arc::new(std::sync::Mutex::new(
            Environment::new(),
        )),
        local_names,
    }));

    let fn_idx = compiler.chunk.add_constant(fn_obj);
    if let Some(fn_idx) = fn_idx {
        compiler.emit(Instruction::Constant(fn_idx), line);
    }
    // Emit OpClosure to capture the current scope's environment at runtime
    compiler.emit(
        Instruction::Closure {
            param_count: params.len() as u8,
            chunk_offset: 0,
        },
        line,
    );
}

fn compile_async_closure(compiler: &mut Compiler, params: &[Ident], body: &Program, line: u16) {
    let (chunk, _param_count, local_names) = Compiler::compile_function_body(params, body, true);
    let fn_obj = Object::AsyncFunction(Box::new(FunctionData {
        params: params.to_vec(),
        chunk: std::sync::Arc::new(chunk),
        env: std::sync::Arc::new(std::sync::Mutex::new(
            Environment::new(),
        )),
        local_names,
    }));

    let fn_idx = compiler.chunk.add_constant(fn_obj);
    if let Some(fn_idx) = fn_idx {
        compiler.emit(Instruction::Constant(fn_idx), line);
    }
    compiler.emit(
        Instruction::Closure {
            param_count: params.len() as u8,
            chunk_offset: 0,
        },
        line,
    );
}
