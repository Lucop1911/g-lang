use crate::ast::ast::Stmt;
use crate::std::math::*;
use crate::std::string::*;
use crate::std::time::*;
use crate::std::io::*;
use crate::std::json::*;
use crate::std::http::*;
use crate::std::env::*;
use std::collections::HashMap;
use std::path::PathBuf;
use tokio::fs;
use std::sync::{Arc, Mutex};
use crate::ast::ast::{Program, Ident};
use crate::interpreter::obj::Object;
use crate::errors::RuntimeError;

#[cfg(feature = "wasm")]
use crate::wasm::{WasmRuntime, WasmStore};

pub struct ModuleRegistry {
    pub(crate) loaded_modules: HashMap<String, Module>,
    stdlib: HashMap<String, Module>,
    pub(crate) base_path: PathBuf,
    #[cfg(feature = "wasm")]
    pub(crate) wasm_runtime: Option<WasmRuntime>,
    #[cfg(feature = "wasm")]
    pub(crate) wasm_store: Option<WasmStore>,
}

#[derive(Clone, Debug)]
pub struct Module {
    pub name: String,
    pub exports: HashMap<String, Object>,
}

impl ModuleRegistry {
    pub fn new(base_path: PathBuf) -> Self {
        #[cfg(feature = "wasm")]
        let (wasm_runtime, wasm_store) = match WasmRuntime::new() {
            Ok(runtime) => {
                let store = runtime.create_store();
                (Some(runtime), Some(store))
            }
            Err(e) => {
                eprintln!("Warning: Failed to initialize WASM runtime: {}", e);
                (None, None)
            }
        };
        
        let mut registry = ModuleRegistry {
            loaded_modules: HashMap::new(),
            stdlib: HashMap::new(),
            base_path,
            #[cfg(feature = "wasm")]
            wasm_runtime,
            #[cfg(feature = "wasm")]
            wasm_store,
        };
        
        registry.load_stdlib();
        
        registry
    }
    
    fn load_stdlib(&mut self) {
        // String modules
        let mut string_exports = HashMap::new();
        
        string_exports.insert("join".to_string(), create_builtin("join", 2, 2, string_join));
        string_exports.insert("reverse".to_string(), create_builtin("reverse", 1, 1, string_reverse));
        string_exports.insert("repeat".to_string(), create_builtin("repeat", 2, 2, string_repeat));
        
        self.stdlib.insert("std::string".to_string(), Module {
            name: "std::string".to_string(),
            exports: string_exports,
        });

        // Math modules
        let mut math_exports = HashMap::new();
        
        math_exports.insert("clamp".to_string(), create_builtin("clamp", 3, 3, math_clamp));
        math_exports.insert("random".to_string(), create_builtin("random", 0, 2, math_random));
        math_exports.insert("round".to_string(), create_builtin("round", 1, 1, math_round));
        math_exports.insert("floor".to_string(), create_builtin("floor", 1, 1, math_floor));
        math_exports.insert("ceil".to_string(), create_builtin("ceil", 1, 1, math_ceil));
        math_exports.insert("sqrt".to_string(), create_builtin("sqrt", 1, 1, math_sqrt));
        math_exports.insert("sin".to_string(), create_builtin("sin", 1, 1, math_sin));
        math_exports.insert("cos".to_string(), create_builtin("cos", 1, 1, math_cos));
        math_exports.insert("tan".to_string(), create_builtin("tan", 1, 1, math_tan));
        math_exports.insert("log".to_string(), create_builtin("log", 1, 1, math_log));
        math_exports.insert("log10".to_string(), create_builtin("log10", 1, 1, math_log10));
        math_exports.insert("abs".to_string(), create_builtin("abs", 1, 1, math_abs_int));
        math_exports.insert("min".to_string(), create_builtin("min", 2, 2, math_min_int));
        math_exports.insert("max".to_string(), create_builtin("max", 2, 2, math_max_int));
        math_exports.insert("PI".to_string(), math_pi());
        math_exports.insert("E".to_string(), math_e());

        self.stdlib.insert("std::math".to_string(), Module {
            name: "std::math".to_string(),
            exports: math_exports,
        });

        // Time modules
        let mut time_exports = HashMap::new();

        time_exports.insert("now".to_string(), create_builtin("now", 0, 0, time_now));
        time_exports.insert("sleep".to_string(), create_builtin("sleep", 1, 1, time_sleep_wrapper));

        self.stdlib.insert("std::time".to_string(), Module {
            name: "std::time".to_string(),
            exports: time_exports,
        });

        // IO modules
        let mut io_exports = HashMap::new();
        
        io_exports.insert("read_file".to_string(), create_builtin("read_file", 1, 1, io_read_file));
        io_exports.insert("read_file_async".to_string(), create_builtin("read_file_async", 1, 1, io_read_file_wrapper));
        io_exports.insert("write_file".to_string(), create_builtin("write_file", 2, 2, io_write_file));
        io_exports.insert("write_file_async".to_string(), create_builtin("write_file_async", 2, 2, io_write_file_wrapper));
        io_exports.insert("append_file".to_string(), create_builtin("append_file", 2, 2, io_append_file));
        io_exports.insert("append_file_async".to_string(), create_builtin("append_file_async", 2, 2, io_append_file_wrapper));

        io_exports.insert("exists".to_string(), create_builtin("exists", 1, 1, io_exists));
        io_exports.insert("is_file".to_string(), create_builtin("is_file", 1, 1, io_is_file));

        io_exports.insert("is_dir".to_string(), create_builtin("is_dir", 1, 1, io_is_dir));

        io_exports.insert("list_dir".to_string(), create_builtin("list_dir", 1, 1, io_list_dir));
        io_exports.insert("list_dir_async".to_string(), create_builtin("list_dir_async", 1, 1, io_list_dir_wrapper));

        io_exports.insert("create_dir".to_string(), create_builtin("create_dir", 1, 1, io_create_dir));
        io_exports.insert("create_dir_async".to_string(), create_builtin("create_dir_async", 1, 1, io_create_dir_wrapper));
        io_exports.insert("delete_file".to_string(), create_builtin("delete_file", 1, 1, io_delete_file));
        io_exports.insert("delete_file_async".to_string(), create_builtin("delete_file_async", 1, 1, io_delete_file_wrapper));
        io_exports.insert("delete_dir".to_string(), create_builtin("delete_dir", 1, 1, io_delete_dir));
        io_exports.insert("delete_dir_async".to_string(), create_builtin("delete_dir_async", 1, 1, io_delete_dir_wrapper));

        self.stdlib.insert("std::io".to_string(), Module {
            name: "std::io".to_string(),
            exports: io_exports,
        });

        // JSON modules
        let mut json_exports = HashMap::new();
        
        json_exports.insert("serialize".to_string(), create_builtin("serialize", 1, 1, json_serialize));
        json_exports.insert("deserialize".to_string(), create_builtin("deserialize", 1, 1, json_deserialize));

        self.stdlib.insert("std::json".to_string(), Module {
            name: "std::json".to_string(),
            exports: json_exports,
        });

        // HTTP modules
        let mut http_exports = HashMap::new();
        
        http_exports.insert("get".to_string(), create_builtin("get", 1, 1, http_get));
        http_exports.insert("post".to_string(), create_builtin("post", 2, 2, http_post));
        http_exports.insert("put".to_string(), create_builtin("put", 2, 2, http_put));
        http_exports.insert("delete".to_string(), create_builtin("delete", 1, 1, http_delete));

        self.stdlib.insert("std::http".to_string(), Module {
            name: "std::http".to_string(),
            exports: http_exports,
        });

        // Env modules
        let mut env_exports = HashMap::new();

        env_exports.insert("args".to_string(), create_builtin("args", 0, 0, env_args));

        self.stdlib.insert("std::env".to_string(), Module {
            name: "std::env".to_string(),
            exports: env_exports,
        });
    }
    
    pub async fn load_module(module_registry_arc: Arc<Mutex<Self>>, path: &[String]) -> Result<Module, RuntimeError> {
        let module_path = path.join("::");
        
        let loaded_module = {
            let registry = module_registry_arc.lock().unwrap();
            registry.loaded_modules.get(&module_path).cloned()
        };

        if let Some(module) = loaded_module {
            return Ok(module);
        }
        
        let stdlib_module = {
            let registry = module_registry_arc.lock().unwrap();
            registry.stdlib.get(&module_path).cloned()
        };

        if let Some(module) = stdlib_module {
            return Ok(module);
        }
        
        ModuleRegistry::load_user_module(module_registry_arc, path).await
    }
    
    async fn load_user_module(module_registry_arc: Arc<Mutex<Self>>, path: &[String]) -> Result<Module, RuntimeError> {
        if path.first().map(|s| s.as_str()) == Some("wasm") {
            return ModuleRegistry::load_wasm_module(module_registry_arc, &path[1..]).await;
        }

        let base_path = { module_registry_arc.lock().unwrap().base_path.clone() };
        let mut file_path = base_path;
        
        for part in path {
            file_path.push(part);
        }
        file_path.set_extension("giu");
        
        let source = fs::read_to_string(&file_path).await
            .map_err(|e| RuntimeError::InvalidOperation(
                format!("Failed to load module '{}': {}", path.join("::"), e)
            ))?;
        
        let module = ModuleRegistry::parse_and_extract_module(Arc::clone(&module_registry_arc), &source, path).await?;
        
        let module_path = path.join("::");
        module_registry_arc.lock().unwrap().loaded_modules.insert(module_path.clone(), module.clone());
        
        Ok(module)
    }
    
    async fn parse_and_extract_module(module_registry_arc: Arc<Mutex<Self>>, source: &str, path: &[String]) -> Result<Module, RuntimeError> {
        use crate::{Lexer, Parser, Tokens};
        
        let token_vec = Lexer::lex_tokens(source.as_bytes())
            .map_err(|e| RuntimeError::InvalidOperation(
                format!("Failed to lex module: {:?}", e)
            ))?
            .1;
        
        let tokens = Tokens::new(&token_vec);
        
        let program = Parser::parse_tokens(tokens)
            .map_err(|e| RuntimeError::InvalidOperation(
                format!("Failed to parse module: {:?}", e)
            ))?
            .1;
        
        let base_path = { module_registry_arc.lock().unwrap().base_path.clone() };
        let registry_arc_for_eval = Arc::new(Mutex::new(ModuleRegistry::new(base_path)));
        
        let loaded_modules_for_eval = { module_registry_arc.lock().unwrap().loaded_modules.clone() };
        for (key, val) in loaded_modules_for_eval {
            registry_arc_for_eval.lock().unwrap().loaded_modules.insert(key.clone(), val.clone());
        }
        
        let exports = ModuleRegistry::extract_exports(program, registry_arc_for_eval).await?;
        
        Ok(Module {
            name: path.join("::"),
            exports,
        })
    }
    
    async fn extract_exports(program: Program, registry: Arc<Mutex<Self>>) -> Result<HashMap<String, Object>, RuntimeError> {
        use crate::interpreter::eval::Evaluator;
        
        let mut evaluator = Evaluator::new(registry);
        let mut exports = HashMap::new();
        
        for stmt in program {
            match stmt.clone() {
                Stmt::StructStmt { name, .. } => {
                    let _obj = evaluator.eval_statement(stmt).await;
                    let Ident(struct_name) = name;
                    
                    if let Some(struct_obj) = evaluator.env.lock().unwrap().get(&struct_name) {
                        exports.insert(struct_name.clone(), struct_obj);
                    }
                }
                Stmt::LetStmt(ident, _) => {
                    evaluator.eval_statement(stmt).await;
                    let Ident(var_name) = ident;
                    
                    if let Some(obj) = evaluator.env.lock().unwrap().get(&var_name) {
                        exports.insert(var_name, obj);
                    }
                }
                Stmt::FnStmt { name, params: _, body: _ } => {
                    evaluator.eval_statement(stmt).await;
                    let Ident(fn_name) = name;

                    if let Some(obj) = evaluator.env.lock().unwrap().get(&fn_name) {
                        exports.insert(fn_name, obj);
                    }
                }
                _ => {
                    evaluator.eval_statement(stmt).await;
                }
            }
        }
        
        Ok(exports)
    }
}

fn create_builtin(name: &str, min: usize, max: usize, func: fn(Vec<Object>) -> Result<Object, RuntimeError>) -> Object {
    Object::BuiltinStd(name.to_string(), min, max, func)
}