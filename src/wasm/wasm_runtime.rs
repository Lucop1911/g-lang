use crate::errors::RuntimeError;
use std::path::Path;
use wasmtime::component::{
    Component, Func, Instance, Linker as ComponentLinker, ResourceTable, Val,
};
use wasmtime::{Engine, Linker as ClassicLinker, Memory, Module, Store, Val as ClassicVal};
use wasmtime_wasi::filesystem::{DirPerms, FilePerms};
use wasmtime_wasi::p1::{self as wasi_p1, WasiP1Ctx};
use wasmtime_wasi::WasiCtx;
use wasmtime_wasi::WasiCtxBuilder;
use wasmtime_wasi::WasiView;

use super::type_conversions;

pub use type_conversions::*;

// Wasmtime engine wrapper - manages compilation and execution of WASM modules
#[derive(Default)]
pub struct WasmRuntime {
    engine: Engine,
}

impl WasmRuntime {
    pub fn new() -> Result<Self, RuntimeError> {
        Ok(Self::default())
    }

    // Get reference to the underlying Wasmtime engine
    pub fn engine(&self) -> &Engine {
        &self.engine
    }

    // WASI preview 2 context (Component Model)
    pub fn create_wasi_ctx() -> WasiCtx {
        WasiCtxBuilder::new()
            .inherit_stdio()
            .inherit_env()
            .inherit_args()
            .inherit_network()
            .preopened_dir(".", ".", DirPerms::all(), FilePerms::all())
            .unwrap()
            .allow_blocking_current_thread(true)
            .build()
    }

    // WASI preview 1 context (classic WASI)
    pub fn create_wasi_p1_ctx() -> WasiP1Ctx {
        WasiCtxBuilder::new()
            .inherit_stdio()
            .inherit_env()
            .inherit_args()
            .inherit_network()
            .preopened_dir(".", ".", DirPerms::all(), FilePerms::all())
            .unwrap()
            .allow_blocking_current_thread(true)
            .build_p1()
    }

    // New store (execution context) with WASI support
    pub fn create_store(&self) -> Store<WasmContext> {
        let context = WasmContext {
            wasi: Self::create_wasi_ctx(),
            wasi_p1: Self::create_wasi_p1_ctx(),
            table: ResourceTable::new(),
        };
        Store::new(&self.engine, context)
    }

    // Alias for create_store (for backwards compatibility)
    pub fn create_store_with_ctx(&self) -> Store<WasmContext> {
        self.create_store()
    }
}

// Represents a loaded WASM module or component
pub enum WasmModule {
    Component { component: Component }, // WASI Preview 2 component
    Classic { module: Module },         // WASI Preview 1 classic module
}

impl WasmModule {
    // Load a module from a file path
    pub fn load(engine: &Engine, path: &Path) -> Result<Self, RuntimeError> {
        let wasm_bytes = std::fs::read(path).map_err(|e| {
            RuntimeError::InvalidOperation(format!(
                "Failed to read wasm file '{}': {}",
                path.display(),
                e
            ))
        })?;

        // Extract file name (without extension) for error messages
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();

        Self::load_from_binary(engine, &name, &wasm_bytes)
    }

    // Load from raw bytes - auto-detects binary format (classic vs component) or WAT text
    fn load_from_binary(
        engine: &Engine,
        name: &str,
        wasm_bytes: &[u8],
    ) -> Result<Self, RuntimeError> {
        // Check for WASM binary magic header
        if wasm_bytes.len() >= 8 && wasm_bytes.starts_with(&[0x00, 0x61, 0x73, 0x6d]) {
            let version = &wasm_bytes[4..8];
            // Version 1 = classic module (P1), version 13 = component (P2)
            return if version == [0x01, 0x00, 0x00, 0x00] {
                Self::load_classic_module(engine, name, wasm_bytes)
            } else {
                Self::load_component(engine, name, wasm_bytes)
            };
        }

        // Handle WAT (WebAssembly Text) format - parse to binary first
        if let Ok(wat_str) = std::str::from_utf8(wasm_bytes) {
            let trimmed = wat_str.trim_start();
            if trimmed.starts_with("(module") || trimmed.starts_with("(;") {
                // WAT for classic module - parse with wat crate
                let wasm_binary = wat::parse_str(wat_str).map_err(|e| {
                    RuntimeError::InvalidOperation(format!(
                        "Failed to parse WAT for module '{}': {}",
                        name, e
                    ))
                })?;
                return Self::load_classic_module(engine, name, &wasm_binary);
            }
            if trimmed.starts_with("(component") {
                // WAT for component - parse with wat crate
                let wasm_binary = wat::parse_str(wat_str).map_err(|e| {
                    RuntimeError::InvalidOperation(format!(
                        "Failed to parse WAT for component '{}': {}",
                        name, e
                    ))
                })?;
                return Self::load_component(engine, name, &wasm_binary);
            }
        }

        Err(RuntimeError::InvalidOperation(format!(
            "File '{}' does not appear to be a valid WASM module or component",
            name
        )))
    }

    // Load a WASI preview 2 component
    fn load_component(
        engine: &Engine,
        name: &str,
        wasm_bytes: &[u8],
    ) -> Result<Self, RuntimeError> {
        let component = Component::new(engine, wasm_bytes).map_err(|e| {
            RuntimeError::InvalidOperation(format!(
                "Failed to compile wasm component '{}': {}",
                name, e
            ))
        })?;

        Ok(WasmModule::Component { component })
    }

    // Load a WASI preview 1 classic module
    fn load_classic_module(
        engine: &Engine,
        name: &str,
        wasm_bytes: &[u8],
    ) -> Result<Self, RuntimeError> {
        let module = Module::new(engine, wasm_bytes).map_err(|e| {
            RuntimeError::InvalidOperation(format!(
                "Failed to compile wasm module '{}': {}",
                name, e
            ))
        })?;

        Ok(WasmModule::Classic { module })
    }

    // Load from bytes (wrapper)
    pub fn load_from_bytes(
        engine: &Engine,
        name: &str,
        bytes: &[u8],
    ) -> Result<Self, RuntimeError> {
        Self::load_from_binary(engine, name, bytes)
    }

    // Instantiate the module into a runnable instance
    pub fn instantiate(
        &self,
        store: &mut Store<WasmContext>,
    ) -> Result<WasmInstance, RuntimeError> {
        match self {
            // Handle WASI Preview 2 components
            WasmModule::Component { component } => {
                let engine = store.engine();
                let mut linker: ComponentLinker<WasmContext> = ComponentLinker::new(engine);
                // Add WASI P2 support to the linker
                wasmtime_wasi::p2::add_to_linker_sync(&mut linker).map_err(|e| {
                    RuntimeError::InvalidOperation(format!("Failed to add WASI to linker: {}", e))
                })?;

                let instance = linker.instantiate(&mut *store, component).map_err(|e| {
                    RuntimeError::InvalidOperation(format!(
                        "Failed to instantiate wasm component '{}': {}",
                        self.name(),
                        e
                    ))
                })?;

                Ok(WasmInstance::Component(WasmComponentInstance { instance }))
            }
            // Handle WASI Preview 1 classic modules
            WasmModule::Classic { module } => {
                let engine = store.engine();
                let mut linker: ClassicLinker<WasmContext> = ClassicLinker::new(engine);
                // Add WASI P1 support to the linker
                wasi_p1::add_to_linker_sync(&mut linker, |ctx| &mut ctx.wasi_p1).map_err(|e| {
                    RuntimeError::InvalidOperation(format!("Failed to add WASI to linker: {}", e))
                })?;

                let instance = linker.instantiate(&mut *store, module).map_err(|e| {
                    RuntimeError::InvalidOperation(format!(
                        "Failed to instantiate wasm module '{}': {}",
                        self.name(),
                        e
                    ))
                })?;

                // Try to get exported memory (optional for classic modules)
                let memory = instance
                    .get_export(&mut *store, "memory")
                    .and_then(|e| e.into_memory());

                Ok(WasmInstance::Classic(WasmClassicInstance {
                    instance,
                    memory,
                }))
            }
        }
    }

    // Get the type name of this module for debugging
    pub fn name(&self) -> &'static str {
        match self {
            WasmModule::Component { .. } => "component",
            WasmModule::Classic { .. } => "classic",
        }
    }
}

// Instantiated WASM module - can be called and queried
pub enum WasmInstance {
    Component(WasmComponentInstance),
    Classic(WasmClassicInstance),
}

// WASI Preview 2 component instance wrapper
pub struct WasmComponentInstance {
    instance: Instance,
}

impl WasmComponentInstance {
    // Get an exported function by name
    fn get_export(&self, store: &mut Store<WasmContext>, name: &str) -> Option<Func> {
        self.instance.get_func(store, name)
    }

    // Call an exported function with component values
    pub fn call_func_with_args(
        &self,
        store: &mut Store<WasmContext>,
        name: &str,
        args: &[Val],
    ) -> Result<Vec<Val>, RuntimeError> {
        let func = self.get_export(store, name).ok_or_else(|| {
            RuntimeError::InvalidOperation(format!(
                "Function '{}' not found in wasm component",
                name
            ))
        })?;

        // Get result types to allocate space for return values
        let func_ty = func.ty(&mut *store);
        let result_count = func_ty.results().len();
        let mut results = vec![Val::S32(0); result_count];

        func.call(&mut *store, args, &mut results).map_err(|e| {
            RuntimeError::InvalidOperation(format!(
                "Failed to call wasm function '{}': {}",
                name, e
            ))
        })?;

        Ok(results)
    }
}

// WASI Preview 1 classic module instance wrapper
pub struct WasmClassicInstance {
    instance: wasmtime::Instance,
    memory: Option<Memory>, // Optional exported memory
}

impl WasmClassicInstance {
    // Get exported memory if available
    pub fn get_memory(&self) -> Option<&Memory> {
        self.memory.as_ref()
    }

    // Get an exported function by name
    fn get_export(&self, store: &mut Store<WasmContext>, name: &str) -> Option<wasmtime::Func> {
        self.instance
            .get_export(&mut *store, name)
            .and_then(|e| e.into_func())
    }

    // Call an exported function with classic (non-component) values
    pub fn call_func_with_args(
        &self,
        store: &mut Store<WasmContext>,
        name: &str,
        args: &[ClassicVal],
    ) -> Result<Vec<ClassicVal>, RuntimeError> {
        let func = self.get_export(store, name).ok_or_else(|| {
            RuntimeError::InvalidOperation(format!("Function '{}' not found in wasm module", name))
        })?;

        // Get result types to allocate space for return values
        let func_ty = func.ty(&mut *store);
        let result_count = func_ty.results().len();
        let mut results = vec![ClassicVal::I32(0); result_count];

        func.call(&mut *store, args, &mut results).map_err(|e| {
            RuntimeError::InvalidOperation(format!(
                "Failed to call wasm function '{}': {}",
                name, e
            ))
        })?;

        Ok(results)
    }
}

impl WasmInstance {
    // Get names of all exported functions
    pub fn get_export_names(&mut self, store: &mut Store<WasmContext>) -> Vec<String> {
        match self {
            WasmInstance::Classic(i) => i
                .instance
                .exports(store)
                .filter_map(|e| {
                    let name = e.name().to_string();
                    if e.into_func().is_some() {
                        Some(name)
                    } else {
                        None
                    }
                })
                .collect(),
            // Components require more complex export traversal - not implemented yet
            WasmInstance::Component(_) => {
                Vec::new()
            },
            
        }
    }

    // Check if a function with the given name exists
    pub fn has_func(&self, store: &mut Store<WasmContext>, name: &str) -> bool {
        match self {
            WasmInstance::Component(i) => i.get_export(store, name).is_some(),
            WasmInstance::Classic(i) => i.get_export(store, name).is_some(),
        }
    }

    // Get exported memory (only available for classic modules)
    pub fn get_memory(&self) -> Option<&Memory> {
        match self {
            // Components handle memory differently through the resource table
            WasmInstance::Component(_) => None,
            WasmInstance::Classic(i) => i.get_memory(),
        }
    }

    // Unified call interface - converts component values to classic values for classic modules
    pub fn call_func_with_args(
        &self,
        store: &mut Store<WasmContext>,
        name: &str,
        args: &[Val],
    ) -> Result<Vec<Val>, RuntimeError> {
        match self {
            // Components use component values directly
            WasmInstance::Component(i) => i.call_func_with_args(store, name, args),
            // Classic modules need conversion from component values to classic values
            WasmInstance::Classic(i) => {
                // Convert component values to classic values
                let classic_args: Vec<ClassicVal> = args
                    .iter()
                    .map(|v| match v {
                        Val::S32(n) => ClassicVal::I32(*n),
                        Val::U32(n) => ClassicVal::I32(*n as i32),
                        Val::S64(n) => ClassicVal::I64(*n),
                        Val::U64(n) => ClassicVal::I64(*n as i64),
                        Val::Float32(n) => ClassicVal::F32(n.to_bits()),
                        Val::Float64(n) => ClassicVal::F64(n.to_bits()),
                        // Other types default to 0
                        _ => ClassicVal::I32(0),
                    })
                    .collect();

                // Call the function
                let results: Vec<ClassicVal> = i.call_func_with_args(store, name, &classic_args)?;

                // Convert results back to component values
                Ok(results
                    .iter()
                    .map(|v| match v {
                        ClassicVal::I32(n) => Val::S32(*n),
                        ClassicVal::I64(n) => Val::S64(*n),
                        ClassicVal::F32(n) => Val::Float32(f32::from_bits(*n)),
                        ClassicVal::F64(n) => Val::Float64(f64::from_bits(*n)),
                        _ => Val::S32(0),
                    })
                    .collect())
            }
        }
    }
}

// Context stored in each WASM store - holds WASI state and resource tables
pub struct WasmContext {
    pub wasi: WasiCtx,        // WASI Preview 2 context
    pub wasi_p1: WasiP1Ctx,   // WASI Preview 1 context
    pub table: ResourceTable, // Resource table for component model handles
}

// Required trait for WASI Preview 2 integration
impl WasiView for WasmContext {
    fn ctx(&mut self) -> wasmtime_wasi::WasiCtxView<'_> {
        wasmtime_wasi::WasiCtxView {
            ctx: &mut self.wasi,
            table: &mut self.table,
        }
    }
}

// Default implementation creates fresh WASI contexts
impl Default for WasmContext {
    fn default() -> Self {
        Self {
            wasi: WasmRuntime::create_wasi_ctx(),
            wasi_p1: WasmRuntime::create_wasi_p1_ctx(),
            table: ResourceTable::new(),
        }
    }
}

// Type alias for store with WASM context
pub type WasmStore = Store<WasmContext>;

// Helper function to create a new WASM store
pub fn create_wasm_store() -> Store<WasmContext> {
    let runtime = WasmRuntime::new().expect("WasmRuntime is infallible");
    runtime.create_store()
}

// Helper function to create a WASM store with custom context
pub fn create_wasm_store_with_context(context: WasmContext) -> Store<WasmContext> {
    let runtime = WasmRuntime::new().expect("WasmRuntime is infallible");
    Store::new(runtime.engine(), context)
}
