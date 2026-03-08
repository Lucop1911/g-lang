#[cfg(feature = "wasm")]
use crate::interpreter::module_registry::Module;
use crate::interpreter::module_registry::ModuleRegistry;
use tokio::fs;
use std::sync::{Arc, Mutex};
use crate::interpreter::obj::Object;
use crate::errors::RuntimeError;

#[cfg(feature = "wasm")]
use crate::wasm::WasmModule;

impl ModuleRegistry {
    #[cfg(feature = "wasm")]
    pub async fn load_wasm_module(module_registry_arc: Arc<Mutex<Self>>, path: &[String]) -> Result<Module, RuntimeError> {
        use std::collections::HashMap;

        if path.is_empty() {
            return Err(RuntimeError::InvalidOperation(
                "wasm module name is required".to_string()
            ));
        }

        let module_name = path.join("::");
        
        let loaded_module = {
            let registry = module_registry_arc.lock().unwrap();
            registry.loaded_modules.get(&format!("wasm::{}", module_name)).cloned()
        };

        if let Some(module) = loaded_module {
            return Ok(module);
        }

        let base_path = { module_registry_arc.lock().unwrap().base_path.clone() };
        let mut file_path = base_path;
        
        for part in path {
            file_path.push(part);
        }

        let wasm_path = file_path.with_extension("wasm");
        let wat_path = file_path.with_extension("wat");
        
        let (file_path, is_wat) = if wasm_path.exists() {
            (wasm_path, false)
        } else if wat_path.exists() {
            (wat_path, true)
        } else {
            return Err(RuntimeError::InvalidOperation(
                format!("Failed to find wasm module '{}': neither .wasm nor .wat file found", module_name)
            ));
        };

        let runtime = {
            let mut registry = module_registry_arc.lock().unwrap();
            registry.wasm_runtime.take().ok_or_else(|| {
                RuntimeError::InvalidOperation("WASM runtime not available".to_string())
            })?
        };

        let wasm_module = if is_wat {
            let wat_bytes = fs::read(&file_path).await.map_err(|e| RuntimeError::InvalidOperation(
                format!("Failed to read wasm file '{}': {}", file_path.display(), e)
            ))?;
            WasmModule::load_from_bytes(runtime.engine(), &module_name, &wat_bytes)?
        } else {
            WasmModule::load(runtime.engine(), &file_path)?
        };

        let mut store = runtime.create_store();
        let instance = wasm_module.instantiate(&mut store)?;

        let all_export_names: Vec<String> = instance
            .get_exports(&mut store)
            .map(|export| export.name().to_string())
            .collect();

        let export_names: Vec<String> = all_export_names
            .into_iter()
            .filter(|name| instance.get_function(&mut store, name).is_some())
            .collect();

        let instance_arc = Arc::new(Mutex::new(Some(instance)));

        let mut exports = HashMap::new();
        for name in &export_names {
            exports.insert(name.clone(), Object::WasmImportedFunction {
                module_name: module_name.clone(),
                func_name: name.clone(),
                instance: Arc::clone(&instance_arc),
            });
        }
        
        let _wasm_module_obj = Object::WasmModule {
            name: module_name.clone(),
            exports: exports.clone(),
            instance: instance_arc,
        };
        
        {
            let mut registry = module_registry_arc.lock().unwrap();
            registry.wasm_runtime = Some(runtime);
            registry.wasm_store = Some(store);
            registry.loaded_modules.insert(
                format!("wasm::{}", module_name),
                Module {
                    name: module_name.clone(),
                    exports: exports.clone(),
                }
            );
        }

        let module = Module {
            name: module_name,
            exports,
        };

        Ok(module)
    }

    #[cfg(not(feature = "wasm"))]
    async fn load_wasm_module(_module_registry_arc: Arc<Mutex<Self>>, path: &[String]) -> Result<Module, RuntimeError> {
        Err(RuntimeError::InvalidOperation(
            format!("WASM support not enabled, cannot load module '{}'", path.join("::"))
        ))
    }
}