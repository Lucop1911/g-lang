use std::sync::Arc;
use ahash::HashMapExt;

use crate::{
    ast::ast::{Ident, ImportItems},
    errors::RuntimeError,
    interpreter::{
        module_registry::ModuleRegistry, obj::{Object, HashMap}
    },
};
use super::super::eval::Evaluator;

impl Evaluator {
    pub fn register_ident(&mut self, ident: Ident, object: Object) -> Object {
        let Ident(name) = ident;
        self.env.lock().unwrap().set(&name, object.clone());
        object
    }

    pub fn eval_import(&mut self, path: Vec<String>, items: ImportItems) -> impl Future<Output = Object> + Send + '_  {
        let self_clone = self.clone();
        async move {
            let path_clone = path.clone();
            let module_registry_arc = Arc::clone(&self_clone.module_registry);
            
            let module_result = ModuleRegistry::load_module(module_registry_arc, &path_clone).await;
            
            let module = match module_result {
                Ok(m) => m,
                Err(e) => return Object::Error(e),
            };
            
            let simple_name = path.last().unwrap().clone();
            
            match items {
                ImportItems::All => {
                    let module_obj = Object::Module {
                        name: path.join("::"),
                        exports: module.exports.clone(),
                    };
                    self_clone.env.lock().unwrap().set(&simple_name, module_obj);
                }
                ImportItems::Specific(names) => {
                    let mut exports = HashMap::new();
                    for name in names {
                        if let Some(obj) = module.exports.get(&name) {
                            exports.insert(name, obj.clone());
                        } else {
                            return Object::Error(RuntimeError::InvalidOperation(
                                format!("Module {} has no export '{}'", module.name, name)
                            ));
                        }
                    }
                    let module_obj = Object::Module {
                        name: path.join("::"),
                        exports,
                    };
                    self_clone.env.lock().unwrap().set(&simple_name, module_obj);
                }
                ImportItems::Single(name) => {
                    if let Some(obj) = module.exports.get(&name) {
                        self_clone.env.lock().unwrap().set(&name, obj.clone());
                    } else {
                        return Object::Error(RuntimeError::InvalidOperation(
                            format!("Module {} has no export '{}'", module.name, name)
                        ));
                    }
                }
            }
            
            Object::Null
        }
    }
}