use crate::errors::RuntimeError;
use std::path::Path;
use wasmtime::{Engine, Func, Instance, Memory, Module, Store, Val};
use wat::parse_bytes;

use super::type_conversions;

pub use type_conversions::*;

pub struct WasmRuntime {
    engine: Engine,
}

impl WasmRuntime {
    pub fn new() -> Result<Self, RuntimeError> {
        let engine = Engine::default();
        Ok(WasmRuntime { engine })
    }

    pub fn engine(&self) -> &Engine {
        &self.engine
    }

    pub fn create_store(&self) -> Store<WasmContext> {
        Store::new(&self.engine, WasmContext::default())
    }
}

pub struct WasmModule {
    module: Module,
    name: String,
}

impl WasmModule {
    pub fn load(engine: &Engine, path: &Path) -> Result<Self, RuntimeError> {
        let wasm_bytes = std::fs::read(path).map_err(|e| {
            RuntimeError::InvalidOperation(format!(
                "Failed to read wasm file '{}': {}",
                path.display(),
                e
            ))
        })?;

        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();

        Self::load_from_binary(engine, &name, &wasm_bytes)
    }

    fn load_from_binary(
        engine: &Engine,
        name: &str,
        wasm_bytes: &[u8],
    ) -> Result<Self, RuntimeError> {
        let module = Module::from_binary(engine, wasm_bytes).map_err(|e| {
            RuntimeError::InvalidOperation(format!(
                "Failed to compile wasm module '{}': {}",
                name, e
            ))
        })?;

        Ok(WasmModule {
            module,
            name: name.to_string(),
        })
    }

    pub fn load_from_bytes(
        engine: &Engine,
        name: &str,
        bytes: &[u8],
    ) -> Result<Self, RuntimeError> {
        let wat_str = std::str::from_utf8(bytes).ok();
        let is_wat = wat_str
            .map(|s| s.trim_start().starts_with("(module") || s.trim_start().starts_with("(;"))
            .unwrap_or(false);

        let wasm_bytes: Vec<u8> = if is_wat {
            let wat = wat_str.unwrap();
            parse_bytes(wat.as_bytes())
                .map_err(|e| {
                    RuntimeError::InvalidOperation(format!("Failed to parse WAT '{}': {}", name, e))
                })?
                .into_owned()
        } else {
            bytes.to_vec()
        };

        Self::load_from_binary(engine, name, &wasm_bytes)
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn instantiate(
        &self,
        store: &mut Store<WasmContext>,
    ) -> Result<WasmInstance, RuntimeError> {
        let instance = Instance::new(&mut *store, &self.module, &[]).map_err(|e| {
            RuntimeError::InvalidOperation(format!(
                "Failed to instantiate wasm module '{}': {}",
                self.name, e
            ))
        })?;

        Ok(WasmInstance::new(instance, store))
    }
}

pub struct WasmInstance {
    instance: Instance,
    memory: Option<Memory>,
}

impl WasmInstance {
    fn new(instance: Instance, store: &mut Store<WasmContext>) -> Self {
        let memory = instance
            .get_export(&mut *store, "memory")
            .and_then(|e| e.into_memory());

        WasmInstance { instance, memory }
    }

    pub fn get_memory(&self) -> Option<&Memory> {
        self.memory.as_ref()
    }

    pub fn get_function(&self, store: &mut Store<WasmContext>, name: &str) -> Option<Func> {
        self.instance
            .get_export(&mut *store, name)
            .and_then(|e| e.into_func())
    }

    pub fn get_exports<'a>(
        &'a self,
        store: &'a mut Store<WasmContext>,
    ) -> impl Iterator<Item = wasmtime::Export<'a>> + 'a {
        self.instance.exports(store)
    }

    pub fn call_func_with_args(
        &self,
        store: &mut Store<WasmContext>,
        name: &str,
        args: &[Val],
    ) -> Result<Vec<Val>, RuntimeError> {
        let func = self
            .instance
            .get_export(&mut *store, name)
            .and_then(|e| e.into_func())
            .ok_or_else(|| {
                RuntimeError::InvalidOperation(format!(
                    "Function '{}' not found in wasm module",
                    name
                ))
            })?;

        let func_ty = func.ty(&mut *store);
        let result_count = func_ty.results().len();

        let mut results = vec![Val::I32(0); result_count];

        func.call(&mut *store, args, &mut results).map_err(|e| {
            RuntimeError::InvalidOperation(format!(
                "Failed to call wasm function '{}': {}",
                name, e
            ))
        })?;

        Ok(results)
    }
}

#[derive(Default)]
pub struct WasmContext {
    pub user_data: (),
}

pub type WasmStore = Store<WasmContext>;

pub fn create_wasm_store() -> Store<WasmContext> {
    Store::default()
}

pub fn create_wasm_store_with_context(context: WasmContext) -> Store<WasmContext> {
    Store::new(&Engine::default(), context)
}
