//! Exception handling compilation: try/catch/finally, throw.

use crate::ast::ast::{Expr, Ident, Program, SlotIndex, Stmt};
use crate::vm::obj::Object;
use crate::vm::compiler::Compiler;
use crate::vm::instruction::Instruction;

/// Compiles a `throw expr;` statement.
pub(crate) fn compile_throw(compiler: &mut Compiler, expr: &Expr, line: u16) {
    compiler.compile_expression(expr, line);
    compiler.emit(Instruction::Throw, line);
}

/// Compiles a `try { ... } catch (e) { ... } finally { ... }` expression.
pub(crate) fn compile_try_catch(
    compiler: &mut Compiler,
    try_body: &Program,
    catch_ident: &Option<Ident>,
    catch_body: &Option<Program>,
    finally_body: &Option<Program>,
    line: u16,
) {
    let has_catch = catch_body.is_some();
    let has_finally = finally_body.is_some();

    if !has_catch && !has_finally {
        compile_block_body(compiler, try_body, line);
        return;
    }

    // PushCatch with placeholder addresses
    let push_catch_offset = compiler.chunk.current_offset();
    compiler.emit(
        Instruction::PushCatch {
            catch_addr: 0,
            finally_addr: 0,
        },
        line,
    );

    // Compile try body
    compile_block_body(compiler, try_body, line);

    // Jumps that need to reach the finally block (or end if no finally)
    let mut jumps_to_finally = Vec::new();

    // Normal path: jump over catch handler
    if has_catch {
        jumps_to_finally.push(compiler.emit_jump(line));
    }

    // Catch handler
    if has_catch {
        let catch_addr = compiler.chunk.current_offset();
        compiler
            .chunk
            .patch_u16(push_catch_offset as usize + 1, catch_addr);

        if let Some(ident) = catch_ident {
            // The thrown value is on the stack. Store it to the catch variable.
            // This pops one value from the stack.
            // After this, the stack will be empty. The catch body will GetLocal
            // to push the value back onto the stack.
            if ident.slot != SlotIndex::UNSET {
                compiler.emit(Instruction::SetLocal(ident.slot.0 as u8), line);
            } else {
                let idx = compiler
                    .chunk
                    .add_constant(Object::String(ident.name.clone()));
                if let Some(idx) = idx {
                    compiler.emit(Instruction::SetGlobal(idx), line);
                }
            }
            // Stack now has: [] (empty)
            // Catch body will push the catch variable's value
        } else {
            // No catch identifier - just pop the thrown value
            compiler.emit(Instruction::Pop, line);
        }

        if let Some(body) = catch_body {
            compile_block_body(compiler, body, line);
        }

        // After catch body, go to finally (or end if no finally)
        jumps_to_finally.push(compiler.emit_jump(line));
    }

    if has_finally {
        let finally_addr = compiler.chunk.current_offset();
        compiler
            .chunk
            .patch_u16(push_catch_offset as usize + 3, finally_addr);

        // Patch all jumps to finally
        for jump in &jumps_to_finally {
            compiler.patch_jump(*jump);
        }

        // Compile finally body
        compiler.finally_depth += 1;
        if let Some(body) = finally_body {
            compile_block_body(compiler, body, line);
        }
        compiler.finally_depth -= 1;

        compiler.emit(Instruction::EndFinally, line);

        // Pop the exception handler after finally
        compiler.emit(Instruction::PopCatch, line);
    } else {
        // No finally: patch jumps to the current position (end of try/catch)
        for jump in &jumps_to_finally {
            compiler.patch_jump(*jump);
        }

        // Pop the exception handler (for try/catch without finally)
        compiler.emit(Instruction::PopCatch, line);
    }
}

/// Compiles a block of statements, leaving the last expression's value on the stack.
fn compile_block_body(compiler: &mut Compiler, body: &Program, line: u16) {
    for (i, stmt) in body.iter().enumerate() {
        compiler.compile_statement(stmt, line);

        // Pop intermediate expression results
        if i < body.len() - 1
            && let Stmt::ExprStmt(_) = stmt {
                compiler.emit(Instruction::Pop, line);
        }
    }
}
