//! Stack-based bytecode virtual machine
//!
//! The `VirtualMachine` executes bytecode produced by the compiler.
//! It uses a flat operand stack for intermediate values and a call frame
//! stack for function invocation, exception handling, and loop control.
//!
//! # Architecture
//!
//! - **Stack**: A single `Vec<Object>` shared across all frames. Each frame
//!   claims a slice of this stack for its local slots.
//! - **Frames**: `CallFrame` tracks the current chunk, instructio
//!
//! Opcode handlers live in `ops/` submodules:
//! - `ops::stack_vars` — stack, locals, globals, jumps
//! - `ops::arithmetic` — numeric ops, comparisons, not/negate
//! - `ops::collections` — arrays, hashes, indexing
//! - `ops::structs` — struct build, field access, method calls
//! - `ops::calls` — function invocation, closures, await
//! - `ops::exceptions` — throw, catch, finallyn pointer,
//!   slot base, and closure environment.
//! - **Globals**: An `Environment` for name-based global lookups (builtins,
//!   top-level `let` bindings).
//! - **Exception handlers**: A stack of `ExceptionHandler` records for
//!   try/catch/finally semantics.

use std::sync::{Arc, Mutex};

use crate::vm::runtime::runtime_errors::RuntimeError;
use crate::vm::runtime::env::Environment;
use crate::vm::runtime::module_registry::ModuleRegistry;
use crate::vm::obj::Object;
use crate::vm::chunk::Chunk;
use crate::vm::frame::CallFrame;
use crate::vm::instruction::Opcode;
use crate::vm::ops;
use crate::vm::ops::exceptions::{handle_throw_result, ExceptionHandler};

/// The result of executing a single instruction.
#[derive(Debug)]
pub enum ExecResult {
    Continue,
    ContinueWith(Object),
    Return,
    Throw,
    Break,
    ContinueLoop,
    JumpTo(usize),
}

/// The stack-based virtual machine.
///
/// Executes bytecode from a [`Chunk`] using an operand stack and a call
/// frame stack. Globals are stored in an `Environment` for name-based
/// lookups, while locals use the flat stack with slot offsets.
pub struct VirtualMachine {
    stack: Vec<Object>,
    frames: Vec<CallFrame>,
    globals: Arc<Mutex<Environment>>,
    module_registry: Arc<Mutex<ModuleRegistry>>,
    exception_handlers: Vec<ExceptionHandler>,
    /// Flag indicating a return is pending (set when returning from finally block)
    pending_return: bool,
    /// Local names for the root frame (function parameters and local variables)
    root_local_names: Vec<String>,
    /// Closure environment for the root frame (used for functions defined in async contexts)
    root_closure_env: Option<Arc<Mutex<Environment>>>,
}

impl VirtualMachine {
    /// Creates a new VM with the given globals and module registry.
    pub fn new(
        globals: Arc<Mutex<Environment>>,
        module_registry: Arc<Mutex<ModuleRegistry>>,
    ) -> Self {
        VirtualMachine {
            stack: Vec::with_capacity(1024),
            frames: Vec::with_capacity(64),
            globals,
            module_registry,
            exception_handlers: Vec::with_capacity(16),
            pending_return: false,
            root_local_names: Vec::new(),
            root_closure_env: None,
        }
    }
    
    /// Sets the local names for the root frame (used for function bodies)
    pub fn set_root_local_names(&mut self, names: Vec<String>) {
        self.root_local_names = names;
    }

    /// Sets the closure environment for the root frame (used in async function contexts)
    pub fn set_root_closure_env(&mut self, env: Arc<Mutex<Environment>>) {
        self.root_closure_env = Some(env);
    }

    /// Creates a new VM with pre-initialized stack slots (for async function calls)
    pub fn new_with_slots(
        globals: Arc<Mutex<Environment>>,
        module_registry: Arc<Mutex<ModuleRegistry>>,
        slot_count: usize,
        initial_values: Vec<Object>,
    ) -> Self {
        let mut vm = VirtualMachine {
            stack: Vec::with_capacity(1024),
            frames: Vec::with_capacity(64),
            globals,
            module_registry,
            exception_handlers: Vec::with_capacity(16),
            pending_return: false,
            root_local_names: Vec::new(),
            root_closure_env: None,
        };
        vm.stack.resize(slot_count, Object::Null);
        for (i, val) in initial_values.into_iter().enumerate() {
            if i < slot_count {
                vm.stack[i] = val;
            }
        }
        vm
    }

    /// Runs a top-level program chunk to completion.
    ///
    /// Returns the top-of-stack value (the program's result) or a
    /// `RuntimeError` if execution fails.
    pub async fn run(&mut self, chunk: Arc<Chunk>) -> Result<Object, RuntimeError> {
        // Only initialize slots if stack is empty (preserve values from async call setup)
        if self.stack.is_empty() {
            let slot_count = 64;
            self.stack.resize(slot_count, Object::Null);
        }
        
        let local_names = std::mem::take(&mut self.root_local_names);
        self.frames
            .push(CallFrame::new_function_body(Arc::clone(&chunk), local_names));
        
        // Set the closure environment for the root frame if available (for async function contexts)
        if let Some(root_env) = self.root_closure_env.take() 
            && let Some(frame) = self.frames.last_mut() {
                frame.closure_env = Some(root_env);
        }

        let mut result = self.execute().await;
        
        // If the result is a Future, we need to await it
        // This handles the case where an async main() function is called at top level
        let mut await_depth = 0;
        while let Ok(Object::Future(future_arc)) = result {
            await_depth += 1;
            if await_depth > 100 {
                return Err(RuntimeError::InvalidOperation(
                    "Too many nested async calls".to_string(),
                ));
            }
            
            let future_to_await = {
                let mut future_opt_guard = future_arc.lock().unwrap();
                let future = future_opt_guard.take();
                drop(future_opt_guard);
                if let Some(f) = future {
                    f
                } else {
                    return Err(RuntimeError::InvalidOperation(
                        "Cannot await a future that has already been awaited".to_string(),
                    ));
                }
            };
            result = future_to_await.await;
        }
        
        // Check if the final result is an Error and convert to Err for proper handling
        if let Ok(Object::Error(e)) = result {
            return Err(*e);
        }
        
        self.frames.clear();
        self.stack.clear();
        self.exception_handlers.clear();
        result
    }

    /// The main execution loop - optimized for sync operations on hot path.
    ///
    /// This loop is designed to minimize overhead for the common case of synchronous opcodes.
    /// Only opcodes that require async (Call, CallBuiltin, CallAsync, Await, ImportModule, CallMethod)
    /// fall through to the async dispatcher.
    async fn execute(&mut self) -> Result<Object, RuntimeError> {
        'outer_loop: loop {
            // Get current frame once per iteration
            let frame = match self.frames.last_mut() {
                Some(f) => f,
                None => {
                    return Ok(self.stack.pop().unwrap_or(Object::Null));
                }
            };
            
            let mut ip = frame.ip;
            let chunk = Arc::clone(&frame.chunk);
            let slots_base = frame.slots_base;
            let code = &chunk.code;
            let constants = &chunk.constants;
            
            // Main execution path for common opcodes (sync, no async overhead)
            'sync_loop: loop {
                if ip >= code.len() {
                    // Frame exhausted, pop and continue outer loop
                    self.frames.pop();
                    if self.frames.is_empty() {
                        return Ok(self.stack.pop().unwrap_or(Object::Null));
                    }
                    continue 'outer_loop;
                }

                let opcode_byte = code[ip];
                
                // Inline operand reading for most common opcodes to avoid closures
                match opcode_byte {
                    // ─── Stack operations ───
                    0x00 => { // OpConstant
                        let idx = u16::from_be_bytes([code[ip + 1], code[ip + 2]]);
                        self.stack.push(constants[idx as usize].clone());
                        ip += 3;
                        continue 'sync_loop;
                    }
                    0x01 => { // OpPop
                        if let Some(Object::Error(e)) = self.stack.pop() {
                            // If we're popping an error, stop execution
                            return Err(*e);
                        }
                        ip += 1;
                        continue 'sync_loop;
                    }
                    0x02 => { // OpDup
                        if let Some(top) = self.stack.last() {
                            self.stack.push(top.clone());
                        }
                        ip += 1;
                        continue 'sync_loop;
                    }
                    0x03 => { // OpSwap
                        let len = self.stack.len();
                        if len >= 2 {
                            self.stack.swap(len - 1, len - 2);
                        }
                        ip += 1;
                        continue 'sync_loop;
                    }
                    0x10 => { // OpGetLocal
                        let slot = code[ip + 1];
                        let idx = slots_base + slot as usize;
                        if idx < self.stack.len() {
                            let value = self.stack[idx].clone();
                            self.stack.push(value);
                        } else {
                            self.stack.push(Object::Null);
                        }
                        ip += 2;
                        continue 'sync_loop;
                    }
                    0x11 => { // OpSetLocal
                        let slot = code[ip + 1];
                        let idx = slots_base + slot as usize;
                        if let Some(value) = self.stack.pop() {
                            if idx >= self.stack.len() {
                                self.stack.resize(idx + 1, Object::Null);
                            }
                            self.stack[idx] = value;
                        }
                        ip += 2;
                        continue 'sync_loop;
                    }
                    0x12 => { // OpGetGlobal
                        let idx = u16::from_be_bytes([code[ip + 1], code[ip + 2]]);
                        let name_obj = &constants[idx as usize];
                        if let Object::String(name) = name_obj {
                            // Try closure environment first
                            let closure_val = frame.closure_env.as_ref().and_then(|env| env.lock().unwrap().get_by_name(name));
                            if let Some(v) = closure_val {
                                self.stack.push(v);
                            } else {
                                // Try globals
                                let gv = self.globals.lock().unwrap().get_by_name(name);
                                if let Some(v) = gv {
                                    self.stack.push(v);
                                } else {
                                    return Err(RuntimeError::UndefinedVariable(name.clone()));
                                }
                            }
                        }
                        ip += 3;
                        continue 'sync_loop;
                    }
                    0x13 => { // OpSetGlobal
                        let idx = u16::from_be_bytes([code[ip + 1], code[ip + 2]]);
                        let name_obj = &constants[idx as usize];
                        if let Object::String(name) = name_obj 
                            && let Some(value) = self.stack.pop() {
                                let mut globals = self.globals.lock().unwrap();
                                let closure_env = frame.closure_env.as_ref();
                                if let Some(env_arc) = closure_env {
                                    let mut env = env_arc.lock().unwrap();
                                    if env.has_var(name) {
                                        env.set_by_name(name, value);
                                    } else {
                                        globals.set_by_name(name, value);
                                    }
                                } else {
                                    globals.set_by_name(name, value);
                                }
                        }
                        ip += 3;
                        continue 'sync_loop;
                    }
                    0x14 => { // OpGetBuiltin
                        let idx = code[ip + 1];
                        use crate::vm::runtime::builtins::functions::BuiltinsFunctions;
                        if (idx as usize) < BuiltinsFunctions::BUILTIN_NAMES.len() {
                            let name = BuiltinsFunctions::BUILTIN_NAMES[idx as usize];
                            let value = self.globals.lock().unwrap().get_by_name(name).unwrap_or(Object::Null);
                            self.stack.push(value);
                        } else {
                            return Err(RuntimeError::InvalidOperation(format!("Unknown builtin index: {}", idx)));
                        }
                        ip += 2;
                        continue 'sync_loop;
                    }
                    // ─── Arithmetic (hot path) ───
                    0x20 => { // OpAdd
                        let b = self.stack.pop().unwrap_or(Object::Null);
                        if let Some(a) = self.stack.last_mut() {
                            match (a, b) {
                                (Object::Integer(ia), Object::Integer(ib)) => *ia = ia.wrapping_add(ib),
                                (Object::Float(fa), Object::Float(fb)) => *fa += fb,
                                (a_val, b_val) => {
                                    let result = match (a_val.clone(), b_val) {
                                        (Object::String(s), Object::String(t)) => Object::String(format!("{}{}", s, t)),
                                        (Object::String(s), other) => Object::String(format!("{}{}", s, other)),
                                        (other, Object::String(s)) => Object::String(format!("{}{}", other, s)),
                                        (Object::Error(e), _) => return Err(*e),
                                        (_, Object::Error(e)) => return Err(*e),
                                        (a, b) => ops::arithmetic::add(a, b),
                                    };
                                    *a_val = result;
                                }
                            }
                        }
                        ip += 1;
                        continue 'sync_loop;
                    }
                    0x21 => { // OpSubtract
                        let b = self.stack.pop().unwrap_or(Object::Null);
                        if let Some(a) = self.stack.last_mut() {
                            match (a, b) {
                                (Object::Integer(ia), Object::Integer(ib)) => *ia = ia.wrapping_sub(ib),
                                (Object::Float(fa), Object::Float(fb)) => *fa -= fb,
                                (a_val, b_val) => {
                                    let result = match (a_val.clone(), b_val) {
                                        (Object::Error(e), _) => return Err(*e),
                                        (_, Object::Error(e)) => return Err(*e),
                                        (a, b) => ops::arithmetic::subtract(a, b),
                                    };
                                    *a_val = result;
                                }
                            }
                        }
                        ip += 1;
                        continue 'sync_loop;
                    }
                    0x22 => { // OpMultiply
                        let b = self.stack.pop().unwrap_or(Object::Null);
                        if let Some(a) = self.stack.last_mut() {
                            match (a, b) {
                                (Object::Integer(ia), Object::Integer(ib)) => *ia = ia.wrapping_mul(ib),
                                (Object::Float(fa), Object::Float(fb)) => *fa *= fb,
                                (a_val, b_val) => {
                                    let result = match (a_val.clone(), b_val) {
                                        (Object::Error(e), _) => return Err(*e),
                                        (_, Object::Error(e)) => return Err(*e),
                                        (a, b) => ops::arithmetic::multiply(a, b),
                                    };
                                    *a_val = result;
                                }
                            }
                        }
                        ip += 1;
                        continue 'sync_loop;
                    }
                    0x23 => { // OpDivide
                        let b = self.stack.pop().unwrap_or(Object::Null);
                        let a = self.stack.pop().unwrap_or(Object::Null);

                        let has_handlers = !self.exception_handlers.is_empty();
                        if (matches!((&a, &b), (Object::Float(_), Object::Float(fb)) if *fb == 0.0) 
                            || matches!((&a, &b), (Object::Integer(_), Object::Integer(ib)) if *ib == 0)) 
                            && has_handlers {
                                self.stack.push(a);
                                self.stack.push(b);
                                break 'sync_loop;
                        }

                        let result = match (&a, b) {
                            (Object::Integer(ia), Object::Integer(ib)) => {
                                if ib == 0 { return Err(RuntimeError::DivisionByZero); }
                                Object::Integer(ia / ib)
                            }
                            (Object::Float(fa), Object::Float(fb)) => {
                                if fb == 0.0 { return Err(RuntimeError::DivisionByZero); }
                                Object::Float(fa / fb)
                            }
                            (a_val, b_val) => {
                                match (a_val.clone(), b_val) {
                                    (Object::Error(e), _) => return Err(*e),
                                    (_, Object::Error(e)) => return Err(*e),
                                    (a, b) => ops::arithmetic::divide(a, b),
                                }
                            }
                        };
                        self.stack.push(result);
                        ip += 1;
                        continue 'sync_loop;
                    }
                    0x24 => { // OpModulo
                        let b = self.stack.pop().unwrap_or(Object::Null);
                        let a = self.stack.pop().unwrap_or(Object::Null);

                        let has_handlers = !self.exception_handlers.is_empty();
                        if has_handlers && matches!((&a, &b), (Object::Integer(_), Object::Integer(ib)) if *ib == 0)
                        {
                            self.stack.push(a);
                            self.stack.push(b);
                            break 'sync_loop;
                        }

                        let result = match (&a, b) {
                            (Object::Integer(ia), Object::Integer(ib)) => {
                                if ib == 0 { return Err(RuntimeError::DivisionByZero); }
                                Object::Integer(ia % ib)
                            }
                            (a_val, b_val) => {
                                match (a_val.clone(), b_val) {
                                    (Object::Error(e), _) => return Err(*e),
                                    (_, Object::Error(e)) => return Err(*e),
                                    (a, b) => ops::arithmetic::modulo(a, b),
                                }
                            }
                        };
                        self.stack.push(result);
                        ip += 1;
                        continue 'sync_loop;
                    }
                    0x25 => { // OpEqual
                        let b = self.stack.pop().unwrap_or(Object::Null);
                        if let Some(a) = self.stack.last_mut() {
                            *a = Object::Boolean(*a == b);
                        }
                        ip += 1;
                        continue 'sync_loop;
                    }
                    0x26 => { // OpNotEqual
                        let b = self.stack.pop().unwrap_or(Object::Null);
                        if let Some(a) = self.stack.last_mut() {
                            *a = Object::Boolean(*a != b);
                        }
                        ip += 1;
                        continue 'sync_loop;
                    }
                    0x27 => { // OpLessThan
                        let b = self.stack.pop().unwrap_or(Object::Null);
                        if let Some(a) = self.stack.last_mut() {
                            let result = match (&*a, b) {
                                (Object::Integer(ia), Object::Integer(ib)) => Object::Boolean(*ia < ib),
                                (Object::Float(fa), Object::Float(fb)) => Object::Boolean(*fa < fb),
                                (a_val, b_val) => {
                                    match (a_val.clone(), b_val) {
                                        (Object::Error(e), _) => return Err(*e),
                                        (_, Object::Error(e)) => return Err(*e),
                                        (a, b) => ops::arithmetic::less_than(a, b),
                                    }
                                }
                            };
                            *a = result;
                        }
                        ip += 1;
                        continue 'sync_loop;
                    }
                    0x28 => { // OpGreaterThan
                        let b = self.stack.pop().unwrap_or(Object::Null);
                        if let Some(a) = self.stack.last_mut() {
                            let result = match (&*a, b) {
                                (Object::Integer(ia), Object::Integer(ib)) => Object::Boolean(*ia > ib),
                                (Object::Float(fa), Object::Float(fb)) => Object::Boolean(*fa > fb),
                                (a_val, b_val) => {
                                    match (a_val.clone(), b_val) {
                                        (Object::Error(e), _) => return Err(*e),
                                        (_, Object::Error(e)) => return Err(*e),
                                        (a, b) => ops::arithmetic::greater_than(a, b),
                                    }
                                }
                            };
                            *a = result;
                        }
                        ip += 1;
                        continue 'sync_loop;
                    }
                    0x29 => { // OpLessEqual
                        let b = self.stack.pop().unwrap_or(Object::Null);
                        if let Some(a) = self.stack.last_mut() {
                            let result = match (&*a, b) {
                                (Object::Integer(ia), Object::Integer(ib)) => Object::Boolean(*ia <= ib),
                                (Object::Float(fa), Object::Float(fb)) => Object::Boolean(*fa <= fb),
                                (a_val, b_val) => {
                                    match (a_val.clone(), b_val) {
                                        (Object::Error(e), _) => return Err(*e),
                                        (_, Object::Error(e)) => return Err(*e),
                                        (a, b) => ops::arithmetic::less_equal(a, b),
                                    }
                                }
                            };
                            *a = result;
                        }
                        ip += 1;
                        continue 'sync_loop;
                    }
                    0x2A => { // OpGreaterEqual
                        let b = self.stack.pop().unwrap_or(Object::Null);
                        if let Some(a) = self.stack.last_mut() {
                            let result = match (&*a, b) {
                                (Object::Integer(ia), Object::Integer(ib)) => Object::Boolean(*ia >= ib),
                                (Object::Float(fa), Object::Float(fb)) => Object::Boolean(*fa >= fb),
                                (a_val, b_val) => {
                                    match (a_val.clone(), b_val) {
                                        (Object::Error(e), _) => return Err(*e),
                                        (_, Object::Error(e)) => return Err(*e),
                                        (a, b) => ops::arithmetic::greater_equal(a, b),
                                    }
                                }
                            };
                            *a = result;
                        }
                        ip += 1;
                        continue 'sync_loop;
                    }
                    0x2B => { // OpNot
                        if let Some(a) = self.stack.last_mut() {
                            let is_truthy = match a {
                                Object::Boolean(b) => *b,
                                Object::Null => false,
                                Object::Integer(i) => *i != 0,
                                Object::Float(f) => *f != 0.0,
                                Object::String(s) => !s.is_empty(),
                                Object::Array(arr) => !arr.is_empty(),
                                Object::Hash(h) => !h.is_empty(),
                                _ => true,
                            };
                            *a = Object::Boolean(!is_truthy);
                        }
                        ip += 1;
                        continue 'sync_loop;
                    }
                    0x2C => { // OpNegate
                        if let Some(a) = self.stack.last_mut() {
                            match a {
                                Object::Integer(i) => *i = i.wrapping_neg(),
                                Object::Float(f) => *f = -*f,
                                a_val => {
                                    *a_val = ops::arithmetic::execute_negate(a_val.clone());
                                }
                            }
                        }
                        ip += 1;
                        continue 'sync_loop;
                    }
                    0x2D => { // OpGetLen
                        let a = self.stack.pop().unwrap_or(Object::Null);
                        let len = match &a {
                            Object::Array(arr) => arr.len() as i64,
                            Object::String(s) => s.len() as i64,
                            Object::Hash(h) => h.len() as i64,
                            _ => {
                                return Err(RuntimeError::InvalidOperation(format!("Cannot get length of {}", a.type_name())));
                            }
                        };
                        self.stack.push(Object::Integer(len));
                        ip += 1;
                        continue 'sync_loop;
                    }
                    0x32 => { // OpJumpIfFalse
                        let offset = u16::from_be_bytes([code[ip + 1], code[ip + 2]]);
                        let value = self.stack.last().unwrap_or(&Object::Null);
                        let is_truthy = match value {
                            Object::Boolean(b) => *b,
                            Object::Null => false,
                            Object::Integer(i) => *i != 0,
                            Object::Float(f) => *f != 0.0,
                            Object::String(s) => !s.is_empty(),
                            Object::Array(arr) => !arr.is_empty(),
                            Object::Hash(h) => !h.is_empty(),
                            _ => true,
                        };
                        if !is_truthy { ip = offset as usize; } else { ip += 3; }
                        continue 'sync_loop;
                    }
                    0x33 => { // OpJumpIfTruthy
                        let offset = u16::from_be_bytes([code[ip + 1], code[ip + 2]]);
                        let value = self.stack.last().unwrap_or(&Object::Null);
                        let is_truthy = match value {
                            Object::Boolean(b) => *b,
                            Object::Null => false,
                            Object::Integer(i) => *i != 0,
                            Object::Float(f) => *f != 0.0,
                            Object::String(s) => !s.is_empty(),
                            Object::Array(arr) => !arr.is_empty(),
                            Object::Hash(h) => !h.is_empty(),
                            _ => true,
                        };
                        if is_truthy { ip = offset as usize; } else { ip += 3; }
                        continue 'sync_loop;
                    }
                    0x34 => { // OpPopJumpIfFalse
                        let offset = u16::from_be_bytes([code[ip + 1], code[ip + 2]]);
                        let value = match self.stack.pop() {
                            Some(v) => v,
                            None => {
                                ip += 3;
                                continue 'sync_loop;
                            }
                        };
                        let should_jump = match value {
                            Object::Boolean(b) => !b,
                            Object::Null => true,
                            Object::Integer(i) => i == 0,
                            Object::Float(f) => f == 0.0,
                            Object::String(s) => s.is_empty(),
                            Object::Array(a) => a.is_empty(),
                            Object::Hash(h) => h.is_empty(),
                            Object::Error(e) => return Err(*e),
                            _ => false,
                        };
                        if should_jump {
                            ip = offset as usize;
                        } else {
                            ip += 3;
                        }
                        continue 'sync_loop;
                    }
                    0x30 => { // OpJump
                        ip = u16::from_be_bytes([code[ip + 1], code[ip + 2]]) as usize;
                        continue 'sync_loop;
                    }
                    0x31 => { // OpJumpBackward
                        ip = u16::from_be_bytes([code[ip + 1], code[ip + 2]]) as usize;
                        continue 'sync_loop;
                    }
                    // For all other opcodes, fall through to async dispatcher
                    _ => {
                        // Break out of sync loop to handle with async dispatcher
                        break 'sync_loop;
                    }
                };
            }
            
            // Update IP in frame before async dispatch
            if let Some(frame) = self.frames.last_mut() {
                frame.ip = ip;
            }
            
            // Async dispatch path for non-trivial opcodes
            let frame = match self.frames.last() {
                Some(f) => f,
                None => {
                    return Ok(self.stack.pop().unwrap_or(Object::Null));
                }
            };
            let ip = frame.ip;
            let chunk = Arc::clone(&frame.chunk);

            if ip >= chunk.code.len() {
                self.frames.pop();
                if self.frames.is_empty() {
                    return Ok(self.stack.pop().unwrap_or(Object::Null));
                }
                continue;
            }

            let opcode_byte = chunk.code[ip];
            let opcode = match Opcode::from_byte(opcode_byte) {
                Some(op) => op,
                None => {
                    return Err(RuntimeError::InvalidOperation(format!(
                        "Unknown opcode: 0x{:02X} at IP {}",
                        opcode_byte, ip
                    )))
                }
            };

            let width = opcode.operand_width();
            if ip + 1 + width > chunk.code.len() {
                return Err(RuntimeError::InvalidOperation(format!(
                    "Truncated instruction at IP {}",
                    ip
                )));
            }

            let read_u8 = |offset: usize| chunk.code[ip + offset];
            let read_u16 = |offset: usize| {
                u16::from_be_bytes([chunk.code[ip + offset], chunk.code[ip + offset + 1]])
            };

            let frame_count_before = self.frames.len();
            let result = self.dispatch(&chunk, &opcode, &read_u8, &read_u16).await?;

            match result {
                ExecResult::Continue => {
                    if self.frames.len() == frame_count_before
                        && let Some(frame) = self.frames.last_mut() {
                            frame.ip = ip + 1 + width;
                    }
                }
                ExecResult::ContinueWith(obj) => {
                    self.stack.push(obj);
                    if self.frames.len() == frame_count_before 
                        && let Some(frame) = self.frames.last_mut() {
                            frame.ip = ip + 1 + width;
                    }
                }
                ExecResult::Return => {
                    let frame_count_after = self.frames.len();
                    let return_value = self.stack.pop().unwrap_or(Object::Null);
                    let caller_stack_len = self.frames.last().map(|f| f.caller_stack_len).unwrap_or(0);

                    if frame_count_after > 0 {
                        self.frames.pop();
                        self.stack.truncate(caller_stack_len);
                        self.stack.push(return_value);
                        if self.frames.is_empty() {
                            return Ok(self.stack.pop().unwrap_or(Object::Null));
                        }
                    } else {
                        if let Some(frame) = self.frames.last_mut() {
                            frame.ip = ip + 1 + width;
                        }
                    }
                }
                ExecResult::Throw => {
                    let result = handle_throw_result(
                        &mut self.stack,
                        &mut self.exception_handlers,
                        &mut self.frames,
                    );
                    match result {
                        Ok(ExecResult::Continue) => {},
                        Ok(ExecResult::Throw) => {
                            return Ok(self.stack.pop().unwrap_or(Object::Null));
                        }
                        Ok(_) => {}
                        Err(RuntimeError::UncaughtException(msg)) => {
                            return Ok(Object::ThrownValue(Box::new(Object::String(msg))));
                        }
                        Err(e) => {
                            return Err(e);
                        }
                    }
                }
                ExecResult::JumpTo(addr) => {
                    if let Some(frame) = self.frames.last_mut() {
                        frame.ip = addr;
                    }
                }
                ExecResult::Break => {
                    let addr = match self.stack.pop() {
                        Some(Object::Integer(a)) => a as usize,
                        _ => {
                            return Err(RuntimeError::InvalidOperation(
                                "Break without address".to_string(),
                            ));
                        }
                    };
                    if let Some(frame) = self.frames.last_mut() {
                        frame.ip = addr;
                    }
                }
                ExecResult::ContinueLoop => {
                    let addr = match self.stack.pop() {
                        Some(Object::Integer(a)) => a as usize,
                        _ => {
                            return Err(RuntimeError::InvalidOperation(
                                "Continue without address".to_string(),
                            ));
                        }
                    };
                    if let Some(frame) = self.frames.last_mut() {
                        frame.ip = addr;
                    }
                }
            }
        }
    }

    /// Dispatch a single decoded instruction to its handler.
    async fn dispatch(
        &mut self,
        chunk: &Chunk,
        opcode: &Opcode,
        read_u8: &impl Fn(usize) -> u8,
        read_u16: &impl Fn(usize) -> u16,
    ) -> Result<ExecResult, RuntimeError> {
        match opcode {
            Opcode::OpConstant => {
                let idx = read_u16(1);
                ops::stack_vars::execute_constant(&mut self.stack, chunk, idx);
                Ok(ExecResult::Continue)
            }
            Opcode::OpPop => {
                if let Some(Object::Error(e)) = ops::stack_vars::execute_pop_check_error(&mut self.stack) {
                    return Err(*e);
                }
                Ok(ExecResult::Continue)
            }
            Opcode::OpDup => {
                ops::stack_vars::execute_dup(&mut self.stack);
                Ok(ExecResult::Continue)
            }
            Opcode::OpSwap => {
                ops::stack_vars::execute_swap(&mut self.stack);
                Ok(ExecResult::Continue)
            }
            Opcode::OpGetLocal => {
                let slot = read_u8(1);
                ops::stack_vars::execute_get_local(&mut self.stack, &self.frames, slot);
                Ok(ExecResult::Continue)
            }
            Opcode::OpSetLocal => {
                let slot = read_u8(1);
                ops::stack_vars::execute_set_local(&mut self.stack, &self.frames, slot);
                Ok(ExecResult::Continue)
            }
            Opcode::OpGetGlobal => {
                let idx = read_u16(1);
                
                // Check if closure_env is the same Arc as globals
                let same_arc = self.frames.last().map(|f| {
                    if let Some(closure_arc) = &f.closure_env {
                        std::ptr::eq(
                            closure_arc.as_ref() as *const _, 
                            self.globals.as_ref() as *const _
                        )
                    } else {
                        false
                    }
                }).unwrap_or(false);
                
                if same_arc {
                    // Closure env is the same Arc as globals, so just lock once
                    let globals = self.globals.lock().unwrap();
                    ops::stack_vars::execute_get_global(
                        &mut self.stack,
                        chunk,
                        &globals,
                        None,
                        idx,
                    );
                } else {
                    // Closure env is different, so lock both separately
                    let globals = self.globals.lock().unwrap();
                    let closure_env = self.frames.last().and_then(|f| {
                        f.closure_env.as_ref().map(|e| e.lock().unwrap())
                    });
                    ops::stack_vars::execute_get_global(
                        &mut self.stack,
                        chunk,
                        &globals,
                        closure_env.as_deref(),
                        idx,
                    );
                }
                
                Ok(ExecResult::Continue)
            }
            Opcode::OpSetGlobal => {
                let idx = read_u16(1);
                let mut globals = self.globals.lock().unwrap();
                let closure_env = self.frames.last().and_then(|f| f.closure_env.clone());
                ops::stack_vars::execute_set_global(
                    &mut self.stack,
                    chunk,
                    &mut globals,
                    closure_env.as_ref(),
                    idx,
                );
                Ok(ExecResult::Continue)
            }
            Opcode::OpGetBuiltin => {
                let idx = read_u8(1);
                let globals = self.globals.lock().unwrap();
                ops::stack_vars::execute_get_builtin(&mut self.stack, &globals, idx);
                Ok(ExecResult::Continue)
            }
            Opcode::OpAdd => {
                let b = self.stack.pop().unwrap_or(Object::Null);
                let a = self.stack.pop().unwrap_or(Object::Null);
                self.stack.push(ops::arithmetic::add(a, b));
                Ok(ExecResult::Continue)
            }
            Opcode::OpSubtract => {
                let b = self.stack.pop().unwrap_or(Object::Null);
                let a = self.stack.pop().unwrap_or(Object::Null);
                self.stack.push(ops::arithmetic::subtract(a, b));
                Ok(ExecResult::Continue)
            }
            Opcode::OpMultiply => {
                let b = self.stack.pop().unwrap_or(Object::Null);
                let a = self.stack.pop().unwrap_or(Object::Null);
                self.stack.push(ops::arithmetic::multiply(a, b));
                Ok(ExecResult::Continue)
            }
            Opcode::OpDivide => {
                let b = self.stack.pop().unwrap_or(Object::Null);
                let a = self.stack.pop().unwrap_or(Object::Null);
                let result = ops::arithmetic::divide(a, b);
                if let Object::Error(_) = &result
                    && !self.exception_handlers.is_empty() {
                        self.stack.push(Object::ThrownValue(Box::new(result)));
                        return Ok(ExecResult::Throw);
                }
                self.stack.push(result);
                Ok(ExecResult::Continue)
            }
            Opcode::OpModulo => {
                let b = self.stack.pop().unwrap_or(Object::Null);
                let a = self.stack.pop().unwrap_or(Object::Null);
                let result = ops::arithmetic::modulo(a, b);
                if let Object::Error(_) = &result
                    && !self.exception_handlers.is_empty() {
                        self.stack.push(Object::ThrownValue(Box::new(result)));
                        return Ok(ExecResult::Throw);
                }
                self.stack.push(result);
                Ok(ExecResult::Continue)
            }
            Opcode::OpEqual => {
                let b = self.stack.pop().unwrap_or(Object::Null);
                let a = self.stack.pop().unwrap_or(Object::Null);
                self.stack.push(ops::arithmetic::execute_equal(a, b));
                Ok(ExecResult::Continue)
            }
            Opcode::OpNotEqual => {
                let b = self.stack.pop().unwrap_or(Object::Null);
                let a = self.stack.pop().unwrap_or(Object::Null);
                self.stack.push(ops::arithmetic::execute_not_equal(a, b));
                Ok(ExecResult::Continue)
            }
            Opcode::OpLessThan => {
                let b = self.stack.pop().unwrap_or(Object::Null);
                let a = self.stack.pop().unwrap_or(Object::Null);
                self.stack.push(ops::arithmetic::less_than(a, b));
                Ok(ExecResult::Continue)
            }
            Opcode::OpGreaterThan => {
                let b = self.stack.pop().unwrap_or(Object::Null);
                let a = self.stack.pop().unwrap_or(Object::Null);
                self.stack.push(ops::arithmetic::greater_than(a, b));
                Ok(ExecResult::Continue)
            }
            Opcode::OpLessEqual => {
                let b = self.stack.pop().unwrap_or(Object::Null);
                let a = self.stack.pop().unwrap_or(Object::Null);
                self.stack.push(ops::arithmetic::less_equal(a, b));
                Ok(ExecResult::Continue)
            }
            Opcode::OpGreaterEqual => {
                let b = self.stack.pop().unwrap_or(Object::Null);
                let a = self.stack.pop().unwrap_or(Object::Null);
                self.stack.push(ops::arithmetic::greater_equal(a, b));
                Ok(ExecResult::Continue)
            }
            Opcode::OpNot => {
                let a = self.stack.pop().unwrap_or(Object::Null);
                self.stack.push(ops::arithmetic::execute_not(a));
                Ok(ExecResult::Continue)
            }
            Opcode::OpNegate => {
                let a = self.stack.pop().unwrap_or(Object::Null);
                self.stack.push(ops::arithmetic::execute_negate(a));
                Ok(ExecResult::Continue)
            }
            Opcode::OpGetLen => {
                let a = self.stack.pop().unwrap_or(Object::Null);
                let len = match &a {
                    Object::Array(arr) => arr.len() as i64,
                    Object::String(s) => s.len() as i64,
                    Object::Hash(h) => h.len() as i64,
                    _ => {
                        return Ok(ExecResult::ContinueWith(Object::Error(
                            Box::new(RuntimeError::InvalidOperation(format!(
                                "Cannot get length of {}",
                                a.type_name()
                            ))),
                        )));
                    }
                };
                self.stack.push(Object::Integer(len));
                Ok(ExecResult::Continue)
            }
            Opcode::OpJump => {
                let offset = read_u16(1);
                Ok(ops::stack_vars::execute_jump(offset))
            }
            Opcode::OpJumpBackward => {
                let offset = read_u16(1);
                Ok(ops::stack_vars::execute_jump_backward(offset))
            }
            Opcode::OpJumpIfFalse => {
                let offset = read_u16(1);
                Ok(ops::stack_vars::execute_jump_if_false(
                    &mut self.stack,
                    ops::arithmetic::is_truthy,
                    offset,
                ))
            }
            Opcode::OpJumpIfTruthy => {
                let offset = read_u16(1);
                Ok(ops::stack_vars::execute_jump_if_truthy(
                    &mut self.stack,
                    ops::arithmetic::is_truthy,
                    offset,
                ))
            }
            Opcode::OpPopJumpIfFalse => {
                let offset = read_u16(1);
                let value = match self.stack.pop() {
                    Some(v) => v,
                    None => return Ok(ExecResult::Continue),
                };
                // Inline is_truthy for Boolean (most common case)
                let should_jump = match value {
                    Object::Boolean(b) => !b,
                    Object::Null => true,
                    Object::Integer(i) => i == 0,
                    Object::Float(f) => f == 0.0,
                    Object::String(s) => s.is_empty(),
                    Object::Array(a) => a.is_empty(),
                    Object::Hash(h) => h.is_empty(),
                    _ => false,
                };
                if should_jump {
                    Ok(ExecResult::JumpTo(offset as usize))
                } else {
                    Ok(ExecResult::Continue)
                }
            }
            Opcode::OpCall => {
                let argc = read_u8(1) as usize;
                ops::calls::execute_call(
                    &mut self.stack,
                    &mut self.frames,
                    &self.module_registry,
                    &self.globals,
                    argc,
                )
            }
            Opcode::OpCallBuiltin => {
                let argc = read_u8(1) as usize;
                ops::calls::execute_call(
                    &mut self.stack,
                    &mut self.frames,
                    &self.module_registry,
                    &self.globals,
                    argc,
                )
            }
            Opcode::OpCallAsync => {
                let argc = read_u8(1) as usize;
                ops::calls::execute_call(
                    &mut self.stack,
                    &mut self.frames,
                    &self.module_registry,
                    &self.globals,
                    argc,
                )
            }
            Opcode::OpReturnValue => {
                // Check if there's an active finally block we need to jump to
                if let Some(handler) = self.exception_handlers.last()
                    && let Some(finally_addr) = handler.finally_addr {
                        // There's a finally block - jump to it instead of returning
                        self.pending_return = true;
                        if let Some(frame) = self.frames.last_mut() {
                            frame.ip = finally_addr as usize;
                        }
                        return Ok(ExecResult::Continue);
                }
                // No finally block - return normally
                Ok(ops::calls::execute_return_value())
            }
            Opcode::OpClosure => {
                ops::calls::execute_closure(
                    &mut self.stack,
                    &mut self.frames,
                );
                Ok(ExecResult::Continue)
            }
            Opcode::OpAwait => {
                let future_obj = match self.stack.pop() {
                    Some(v) => v,
                    None => {
                        return Ok(ExecResult::ContinueWith(Object::Error(
                            Box::new(RuntimeError::InvalidOperation(
                                "Stack underflow on Await".to_string(),
                            )),
                        )));
                    }
                };

                match future_obj {
                    Object::Future(future_arc) => {
                        let future_to_await = {
                            let mut future_opt_guard = future_arc.lock().unwrap();
                            let future = future_opt_guard.take();
                            drop(future_opt_guard);
                            if let Some(f) = future {
                                f
                            } else {
                                return Ok(ExecResult::ContinueWith(Object::Error(
                                    Box::new(RuntimeError::InvalidOperation(
                                        "Cannot await a future that has already been awaited"
                                            .to_string(),
                                    )),
                                )));
                            }
                        };

                        let result = future_to_await.await;
                        match result {
                            Ok(obj) => {
                                self.stack.push(obj);
                            }
                            Err(e) => {
                                self.stack.push(Object::Error(Box::new(e)));
                            }
                        }
                        Ok(ExecResult::Continue)
                    }
                    Object::Error(e) => {
                        self.stack.push(Object::Error(e));
                        Ok(ExecResult::Continue)
                    }
                    _ => Ok(ExecResult::ContinueWith(Object::Error(
                        Box::new(RuntimeError::InvalidOperation(format!(
                            "Cannot await non-future type: {}",
                            future_obj.type_name()
                        ))),
                    ))),
                }
            }
            Opcode::OpBuildArray => {
                let count = read_u16(1);
                ops::collections::execute_build_array(&mut self.stack, count);
                Ok(ExecResult::Continue)
            }
            Opcode::OpBuildHash => {
                let pair_count = read_u16(1);
                ops::collections::execute_build_hash(&mut self.stack, pair_count);
                Ok(ExecResult::Continue)
            }
            Opcode::OpIndex => {
                ops::collections::execute_index(&mut self.stack);
                Ok(ExecResult::Continue)
            }
            Opcode::OpSetIndex => {
                ops::collections::execute_set_index(&mut self.stack);
                Ok(ExecResult::Continue)
            }
            Opcode::OpBuildStruct => {
                let field_count = read_u8(1);
                ops::structs::execute_build_struct(&mut self.stack, field_count);
                Ok(ExecResult::Continue)
            }
            Opcode::OpGetField => {
                ops::structs::execute_get_field(&mut self.stack);
                Ok(ExecResult::Continue)
            }
            Opcode::OpSetField => {
                ops::structs::execute_set_field(&mut self.stack);
                Ok(ExecResult::Continue)
            }
            Opcode::OpCallMethod => {
                let argc = read_u8(1) as usize;
                match ops::structs::execute_call_method(&mut self.stack, argc)? {
                    ops::structs::MethodCallResult::NeedsCall(new_argc) => {
                        ops::calls::execute_call(
                            &mut self.stack,
                            &mut self.frames,
                            &self.module_registry,
                            &self.globals,
                            new_argc,
                        )
                    }
                    ops::structs::MethodCallResult::Done => {
                        Ok(ExecResult::Continue)
                    }
                    ops::structs::MethodCallResult::Error(err_obj) => {
                        self.stack.push(err_obj);
                        Ok(ExecResult::Continue)
                    }
                }
            }
            Opcode::OpThrow => Ok(ops::exceptions::execute_throw(&mut self.stack)),
            Opcode::OpPushCatch => {
                let catch_addr = read_u16(1);
                let finally_addr = read_u16(3);
                ops::exceptions::execute_push_catch(
                    &mut self.exception_handlers,
                    catch_addr,
                    finally_addr,
                );
                Ok(ExecResult::Continue)
            }
            Opcode::OpPopCatch => {
                ops::exceptions::execute_pop_catch(&mut self.exception_handlers, &self.stack);
                Ok(ExecResult::Continue)
            }
            Opcode::OpPushFinally => {
                let addr = read_u16(1);
                ops::exceptions::execute_push_finally(&mut self.exception_handlers, addr);
                Ok(ExecResult::Continue)
            }
            Opcode::OpEndFinally => {
                let result = ops::exceptions::execute_end_finally(&mut self.exception_handlers, &mut self.stack, &mut self.pending_return);
                Ok(result)
            }
            Opcode::OpBreak => {
                let addr = read_u16(1);
                self.stack.push(Object::Integer(addr as i64));
                Ok(ExecResult::Break)
            }
            Opcode::OpContinue => {
                let addr = read_u16(1);
                self.stack.push(Object::Integer(addr as i64));
                Ok(ExecResult::ContinueLoop)
            }
            Opcode::OpImportModule => {
                let idx = read_u16(1);
                ops::modules::execute_import_module(
                    &mut self.stack,
                    chunk,
                    &self.module_registry,
                    idx,
                )?;
                Ok(ExecResult::Continue)
            }
            Opcode::OpGetExport => {
                ops::modules::execute_get_export(&mut self.stack);
                Ok(ExecResult::Continue)
            }
        }
    }
}