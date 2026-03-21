use crate::errors::RuntimeError;
use crate::interpreter::obj::Object;
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;
use wasmtime::{Memory, Store, ValType};

#[cfg(feature = "wasm")]
use wasmtime::component::Val as ComponentVal;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WasmType {
    I32,
    I64,
    F32,
    F64,
}

impl From<ValType> for WasmType {
    fn from(vt: ValType) -> Self {
        match vt {
            ValType::I32 => WasmType::I32,
            ValType::I64 => WasmType::I64,
            ValType::F32 => WasmType::F32,
            ValType::F64 => WasmType::F64,
            _ => WasmType::I32,
        }
    }
}

impl WasmType {
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "i32" => Some(WasmType::I32),
            "i64" => Some(WasmType::I64),
            "f32" => Some(WasmType::F32),
            "f64" => Some(WasmType::F64),
            _ => None,
        }
    }

    pub fn to_valtype(&self) -> ValType {
        match self {
            WasmType::I32 => ValType::I32,
            WasmType::I64 => ValType::I64,
            WasmType::F32 => ValType::F32,
            WasmType::F64 => ValType::F64,
        }
    }
}

pub struct TypeMapping {
    g_to_wasm: HashMap<String, WasmType>,
    wasm_to_g: HashMap<WasmType, String>,
}

impl TypeMapping {
    pub fn new() -> Self {
        let mut g_to_wasm = HashMap::new();
        let mut wasm_to_g = HashMap::new();

        g_to_wasm.insert("Int".to_string(), WasmType::I32);
        g_to_wasm.insert("Float".to_string(), WasmType::F64);
        g_to_wasm.insert("Bool".to_string(), WasmType::I32);
        g_to_wasm.insert("String".to_string(), WasmType::I32);

        wasm_to_g.insert(WasmType::I32, "Int".to_string());
        wasm_to_g.insert(WasmType::I64, "Int".to_string());
        wasm_to_g.insert(WasmType::F32, "Float".to_string());
        wasm_to_g.insert(WasmType::F64, "Float".to_string());

        TypeMapping {
            g_to_wasm,
            wasm_to_g,
        }
    }

    pub fn get_wasm_type(&self, g_type: &str) -> Option<WasmType> {
        self.g_to_wasm.get(g_type).copied()
    }

    pub fn get_g_type(&self, wasm_type: WasmType) -> Option<String> {
        self.wasm_to_g.get(&wasm_type).cloned()
    }
}

impl Default for TypeMapping {
    fn default() -> Self {
        Self::new()
    }
}

pub fn g_to_component_val(obj: &Object) -> Result<ComponentVal, RuntimeError> {
    match obj {
        Object::Integer(n) => Ok(ComponentVal::S32(*n as i32)),
        Object::Float(n) => Ok(ComponentVal::Float64(*n)),
        Object::Boolean(b) => Ok(ComponentVal::S32(if *b { 1 } else { 0 })),
        _ => Err(RuntimeError::InvalidOperation(format!(
            "Cannot convert {:?} to wasm component value",
            obj
        ))),
    }
}

pub fn component_val_to_g(val: &ComponentVal) -> Result<Object, RuntimeError> {
    match val {
        ComponentVal::S32(n) => Ok(Object::Integer(*n as i64)),
        ComponentVal::U32(n) => Ok(Object::Integer(*n as i64)),
        ComponentVal::S64(n) => Ok(Object::Integer(*n as i64)),
        ComponentVal::U64(n) => Ok(Object::Integer(*n as i64)),
        ComponentVal::Float32(n) => Ok(Object::Float(*n as f64)),
        ComponentVal::Float64(n) => Ok(Object::Float(*n)),
        ComponentVal::Bool(b) => Ok(Object::Boolean(*b)),
        _ => Err(RuntimeError::InvalidOperation(
            "Unsupported wasm value type".to_string(),
        )),
    }
}

pub fn allocate_in_wasm_memory<T>(
    memory: &Memory,
    store: &mut Store<T>,
    size: usize,
) -> Result<usize, RuntimeError> {
    let pages = memory.size(&*store) as usize;
    let max_size = pages * 65536;

    if max_size < size {
        return Err(RuntimeError::InvalidOperation(
            "Not enough memory to allocate".to_string(),
        ));
    }

    let data = memory.data(&*store);
    for ptr in 0..max_size {
        let mut can_use = true;
        for i in 0..size {
            if ptr + i < max_size && data.get(ptr + i).is_some() {
                can_use = false;
                break;
            }
        }
        if can_use {
            return Ok(ptr);
        }
    }

    Err(RuntimeError::InvalidOperation(
        "Failed to find free memory location".to_string(),
    ))
}

pub fn read_string_from_wasm<T>(
    memory: &Memory,
    store: &mut Store<T>,
    ptr: i32,
    max_len: usize,
) -> Result<String, RuntimeError> {
    let ptr = ptr as usize;

    let mut data = vec![0u8; max_len];
    memory.read(&*store, ptr, &mut data).map_err(|e| {
        RuntimeError::InvalidOperation(format!("Failed to read from wasm memory: {}", e))
    })?;

    if let Some(null_pos) = data.iter().position(|&b| b == 0) {
        let string_data = &data[..null_pos];
        String::from_utf8(string_data.to_vec()).map_err(|e| {
            RuntimeError::InvalidOperation(format!("Invalid UTF-8 in wasm string: {}", e))
        })
    } else {
        String::from_utf8(data).map_err(|e| {
            RuntimeError::InvalidOperation(format!("Invalid UTF-8 in wasm string: {}", e))
        })
    }
}

pub struct WasmMemoryManager {
    pub memory: Memory,
    pub next_ptr: Arc<RefCell<usize>>,
}

impl WasmMemoryManager {
    pub fn new(memory: Memory) -> Self {
        WasmMemoryManager {
            memory,
            next_ptr: Arc::new(RefCell::new(4096)),
        }
    }

    pub fn allocate<T>(&self, _store: &mut Store<T>, size: usize) -> Result<i32, RuntimeError> {
        let mut next = self.next_ptr.borrow_mut();
        let ptr = *next;
        *next += size;
        Ok(ptr as i32)
    }

    pub fn write_string<T>(&self, store: &mut Store<T>, s: &str) -> Result<i32, RuntimeError> {
        let bytes = s.as_bytes();
        let ptr = self.allocate(store, bytes.len() + 1)?;

        self.memory
            .write(&mut *store, ptr as usize, bytes)
            .map_err(|e| {
                RuntimeError::InvalidOperation(format!("Failed to write string to wasm: {}", e))
            })?;

        let null_ptr = (ptr as usize) + bytes.len();
        self.memory
            .write(&mut *store, null_ptr, &[0])
            .map_err(|e| {
                RuntimeError::InvalidOperation(format!("Failed to write null terminator: {}", e))
            })?;

        Ok(ptr)
    }

    pub fn read_string<T>(
        &self,
        store: &mut Store<T>,
        ptr: i32,
        max_len: usize,
    ) -> Result<String, RuntimeError> {
        let ptr = ptr as usize;

        let mut data = vec![0u8; max_len];
        self.memory.read(&*store, ptr, &mut data).map_err(|e| {
            RuntimeError::InvalidOperation(format!("Failed to read from wasm memory: {}", e))
        })?;

        if let Some(null_pos) = data.iter().position(|&b| b == 0) {
            data.truncate(null_pos);
        }

        String::from_utf8(data)
            .map_err(|e| RuntimeError::InvalidOperation(format!("Invalid UTF-8 from wasm: {}", e)))
    }
}
