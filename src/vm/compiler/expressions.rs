//! Expression compilation: literals, operators, identifiers, and calls.

use crate::ast::ast::{Expr, Ident, Infix, Literal, Prefix, SlotIndex};
use crate::vm::obj::Object;
use crate::vm::compiler::Compiler;
use crate::vm::instruction::Instruction;
use crate::vm::runtime::builtins::functions::BuiltinsFunctions;

/// Compiles an identifier expression.
///
/// Uses the pre-computed slot index for O(1) access. If `UNSET`,
/// falls back to name-based global lookup.
pub(crate) fn compile_ident(compiler: &mut Compiler, ident: &Ident, line: u16) {
    if ident.slot != SlotIndex::UNSET {
        compiler.emit(Instruction::GetLocal(ident.slot.0 as u8), line);
    } else if let Some(idx) = BuiltinsFunctions::BUILTIN_NAMES
        .iter()
        .position(|&name| name == ident.name)
    {
        compiler.emit(Instruction::GetBuiltin(idx as u8), line);
    } else {
        let idx = compiler
            .chunk
            .add_constant(Object::String(ident.name.clone()));
        if let Some(idx) = idx {
            compiler.emit(Instruction::GetGlobal(idx), line);
        }
    }
}

/// Compiles a literal expression by adding it to the constant pool.
pub(crate) fn compile_literal(compiler: &mut Compiler, literal: &Literal, line: u16) {
    let obj = match literal {
        Literal::IntLiteral(i) => Object::Integer(*i),
        Literal::BigIntLiteral(b) => Object::BigInteger(Box::new(b.clone())),
        Literal::FloatLiteral(f) => Object::Float(*f),
        Literal::BoolLiteral(b) => Object::Boolean(*b),
        Literal::StringLiteral(s) => Object::String(s.clone()),
        Literal::NullLiteral => Object::Null,
    };
    compiler.emit_constant(obj, line);
}

/// Compiles a prefix (unary) expression: `!x`, `-x`, `+x`.
pub(crate) fn compile_prefix(compiler: &mut Compiler, op: &Prefix, operand: &Expr, line: u16) {
    compiler.compile_expression(operand, line);

    match op {
        Prefix::Not => {
            compiler.emit(Instruction::Not, line);
        }
        Prefix::PrefixMinus => {
            compiler.emit(Instruction::Negate, line);
        }
        Prefix::PrefixPlus => {
            // Unary plus is a no-op — the value is already on the stack.
        }
    }
}

/// Compiles an infix (binary) expression: `a + b`, `a == b`, etc.
pub(crate) fn compile_infix(compiler: &mut Compiler, op: &Infix, left: &Expr, right: &Expr, line: u16) {
    // Short-circuit evaluation for && and ||
    match op {
        Infix::And => {
            compiler.compile_expression(left, line);
            let jump_patch = compiler.emit_jump_if_false(line);
            compiler.emit(Instruction::Pop, line); // Pop the left value
            compiler.compile_expression(right, line);
            compiler.patch_jump(jump_patch);
            return;
        }
        Infix::Or => {
            compiler.compile_expression(left, line);
            let jump_patch = compiler.emit_jump_if_truthy(line);
            compiler.emit(Instruction::Pop, line);
            compiler.compile_expression(right, line);
            compiler.patch_jump(jump_patch);
            return;
        }
        _ => {}
    }

    // Standard infix: compile left, compile right, apply operator
    compiler.compile_expression(left, line);
    compiler.compile_expression(right, line);

    let instr = match op {
        Infix::Plus => Instruction::Add,
        Infix::Minus => Instruction::Subtract,
        Infix::Multiply => Instruction::Multiply,
        Infix::Divide => Instruction::Divide,
        Infix::Modulo => Instruction::Modulo,
        Infix::Equal => Instruction::Equal,
        Infix::NotEqual => Instruction::NotEqual,
        Infix::LessThan => Instruction::LessThan,
        Infix::GreaterThan => Instruction::GreaterThan,
        Infix::LessThanEqual => Instruction::LessEqual,
        Infix::GreaterThanEqual => Instruction::GreaterEqual,
        Infix::And | Infix::Or => unreachable!("handled above"),
    };

    compiler.emit(instr, line);
}

/// Compiles a `this` expression — looks up `this` as a local variable.
///
/// In method calls, `this` is passed as the first implicit parameter
/// at slot 0.
pub(crate) fn compile_this_expr(compiler: &mut Compiler, line: u16) {
    // `this` is always at slot 0 in method frames
    compiler.emit(Instruction::GetLocal(0), line);
}
