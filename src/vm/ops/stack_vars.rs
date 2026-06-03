//! Stack, variable, and control flow operations.

use std::sync::Arc;

use crate::vm::runtime::builtins::functions::BuiltinsFunctions;
use crate::vm::runtime::env::Environment;
use crate::vm::obj::Object;
use crate::vm::runtime::runtime_errors::RuntimeError;
use crate::vm::chunk::Chunk;
use crate::vm::frame::CallFrame;
use crate::vm::vm::ExecResult;

pub fn execute_constant(stack: &mut Vec<Object>, chunk: &Chunk, idx: u16) {
    let idx_usize = idx as usize;
    if idx_usize >= chunk.constants.len() {
        eprintln!(
            "PANIC: constant index {} out of bounds (constants.len={})",
            idx,
            chunk.constants.len()
        );
        eprintln!("  This usually means IP is corrupted or wrong chunk is being used");
        eprintln!("  chunk code len: {}", chunk.code.len());
        eprintln!("  Stack size: {}", stack.len());
        if !stack.is_empty() {
            eprintln!("  Top 10 stack values:");
            for (i, obj) in stack.iter().rev().take(10).enumerate() {
                eprintln!("    [{}] {:?}", i, obj);
            }
        }
        eprintln!(
            "  First 10 constants: {:?}",
            &chunk.constants[..chunk.constants.len().min(10)]
        );
        panic!("constant index out of bounds");
    }
    // For small primitives, avoid cloning where possible
    let value = match &chunk.constants[idx_usize] {
        Object::Integer(i) => Object::Integer(*i),
        Object::Float(f) => Object::Float(*f),
        Object::Boolean(b) => Object::Boolean(*b),
        Object::Null => Object::Null,
        other => other.clone(),
    };
    stack.push(value);
}

pub fn execute_pop(stack: &mut Vec<Object>) {
    stack.pop();
}

pub fn execute_pop_check_error(stack: &mut Vec<Object>) -> Option<Object> {
    // Pops the value and returns it if it's an error, otherwise returns None
    stack.pop().filter(|value| matches!(value, Object::Error(_)))
}

pub fn execute_dup(stack: &mut Vec<Object>) {
    if let Some(top) = stack.last() {
        let value = match top {
            Object::Integer(i) => Object::Integer(*i),
            Object::Float(f) => Object::Float(*f),
            Object::Boolean(b) => Object::Boolean(*b),
            Object::Null => Object::Null,
            other => other.clone(),
        };
        stack.push(value);
    }
}

pub fn execute_swap(stack: &mut [Object]) {
    let len = stack.len();
    if len >= 2 {
        stack.swap(len - 1, len - 2);
    }
}

pub fn execute_get_local(stack: &mut Vec<Object>, frames: &[CallFrame], slot: u8) {
    if let Some(frame) = frames.last() {
        let idx = frame.slots_base + slot as usize;
        if idx < stack.len() {
            let value = match &stack[idx] {
                Object::Integer(i) => Object::Integer(*i),
                Object::Float(f) => Object::Float(*f),
                Object::Boolean(b) => Object::Boolean(*b),
                Object::Null => Object::Null,
                other => other.clone(),
            };
            stack.push(value);
        } else {
            stack.push(Object::Null);
        }
    }
}

pub fn execute_set_local(stack: &mut Vec<Object>, frames: &[CallFrame], slot: u8) {
    if let Some(frame) = frames.last() {
        let idx = frame.slots_base + slot as usize;
        if let Some(value) = stack.pop() {
            if idx >= stack.len() {
                stack.resize(idx + 1, Object::Null);
            }
            stack[idx] = value;
        }
    }
}

pub fn execute_get_global(
    stack: &mut Vec<Object>,
    chunk: &Chunk,
    globals: &Environment,
    closure_env: Option<&Environment>,
    idx: u16,
) {
    let name_obj = &chunk.constants[idx as usize];
    let name = match name_obj {
        Object::String(s) => s.clone(),
        _ => {
            stack.push(Object::Error(Box::new(RuntimeError::InvalidOperation(
                "Global name must be a string constant".to_string(),
            ))));
            return;
        }
    };

    // Check closure environment first (for captured variables), then globals
    let closure_val = closure_env.and_then(|env| env.get_by_name(&name));
    let value = match closure_val {
        Some(v) => v,
        None => {
            let gv = globals.get_by_name(&name);
            match gv {
                Some(v) => v,
                None => {
                    stack.push(Object::Error(Box::new(RuntimeError::UndefinedVariable(name))));
                    return;
                }
            }
        }
    };
    stack.push(value);
}

pub fn execute_set_global(
    stack: &mut Vec<Object>,
    chunk: &Chunk,
    globals: &mut Environment,
    closure_env: Option<&Arc<std::sync::Mutex<Environment>>>,
    idx: u16,
) {
    let name_obj = &chunk.constants[idx as usize];
    let name = match name_obj {
        Object::String(s) => s.clone(),
        _ => {
            stack.push(Object::Error(Box::new(RuntimeError::InvalidOperation(
                "Global name must be a string constant".to_string(),
            ))));
            return;
        }
    };

    if let Some(value) = stack.pop() {
        // Check if this variable exists in the closure environment (captured var)
        // If so, update it there. Otherwise update globals.
        if let Some(env_arc) = closure_env {
            let env = env_arc.lock().unwrap();
            if env.has_var(&name) {
                drop(env);
                env_arc.lock().unwrap().set_by_name(&name, value);
            } else {
                drop(env);
                globals.set_by_name(&name, value);
            }
        } else {
            globals.set_by_name(&name, value);
        }
    }
}

pub fn execute_get_builtin(stack: &mut Vec<Object>, globals: &Environment, idx: u8) {
    let name = if (idx as usize) < BuiltinsFunctions::BUILTIN_NAMES.len() {
        BuiltinsFunctions::BUILTIN_NAMES[idx as usize].to_string()
    } else {
        stack.push(Object::Error(Box::new(RuntimeError::InvalidOperation(format!(
            "Unknown builtin index: {}",
            idx
        )))));
        return;
    };

    let value = globals.get_by_name(&name).unwrap_or(Object::Null);
    stack.push(value);
}

pub fn execute_jump(offset: u16) -> ExecResult {
    ExecResult::JumpTo(offset as usize)
}

pub fn execute_jump_backward(offset: u16) -> ExecResult {
    ExecResult::JumpTo(offset as usize)
}

pub fn execute_jump_if_false(
    stack: &mut [Object],
    is_truthy: impl Fn(&Object) -> bool,
    offset: u16,
) -> ExecResult {
    let value = match stack.last() {
        Some(v) => v,
        None => return ExecResult::Continue,
    };
    if !is_truthy(value) {
        ExecResult::JumpTo(offset as usize)
    } else {
        ExecResult::Continue
    }
}

pub fn execute_jump_if_truthy(
    stack: &mut [Object],
    is_truthy: impl Fn(&Object) -> bool,
    offset: u16,
) -> ExecResult {
    let value = match stack.last() {
        Some(v) => v,
        None => return ExecResult::Continue,
    };
    if is_truthy(value) {
        ExecResult::JumpTo(offset as usize)
    } else {
        ExecResult::Continue
    }
}

pub fn execute_pop_jump_if_false(
    stack: &mut Vec<Object>,
    is_truthy: impl Fn(&Object) -> bool,
    offset: u16,
) -> ExecResult {
    let value = match stack.pop() {
        Some(v) => v,
        None => return ExecResult::Continue,
    };
    if !is_truthy(&value) {
        ExecResult::JumpTo(offset as usize)
    } else {
        ExecResult::Continue
    }
}
