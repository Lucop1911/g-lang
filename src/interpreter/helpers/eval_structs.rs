use std::sync::{Arc, Mutex};
use ahash::HashMapExt;
use crate::{
    ast::ast::{Expr, Ident},
    errors::RuntimeError,
    interpreter::{
        env::Environment, obj::{Object, HashMap}
    },
};
use crate::interpreter::builtins::methods::BuiltinMethods;
use super::super::eval::Evaluator;

impl Evaluator {
    pub fn eval_struct_def(&mut self, name: Ident, fields: Vec<(Ident, Expr)>, methods: Vec<(Ident, Expr)>) -> impl Future<Output = Object> + Send + '_  {
        let mut self_clone = self.clone();
        async move {
            let Ident(struct_name) = name.clone();
            
            let mut default_fields = HashMap::new();
            for (Ident(field_name), expr) in fields {
                let value = self_clone.eval_expr(expr).await;
                default_fields.insert(field_name, value);
            }
            
            let mut struct_methods = HashMap::new();
            for (Ident(method_name), expr) in methods {
                let method_obj = self_clone.eval_expr(expr).await;
                struct_methods.insert(method_name, method_obj);
            }

            let struct_obj = Object::Struct {
                name: struct_name.clone(),
                fields: default_fields,
                methods: struct_methods,
            };
            
            self_clone.env.lock().unwrap().set(&struct_name, struct_obj.clone());
            
            Object::Null
        }
    }

    pub fn eval_struct_literal(&mut self, name: Ident, field_assignments: Vec<(Ident, Expr)>) -> impl Future<Output = Object> + Send + '_  {
        let mut self_clone = self.clone();
        async move {
            let Ident(struct_name) = name;
            
            let (default_fields, methods) = match self_clone.env.lock().unwrap().get(&struct_name) {
                Some(Object::Struct { fields, methods, .. }) => (fields.clone(), methods.clone()),
                Some(_) => return Object::Error(RuntimeError::InvalidOperation(
                    format!("{} is not a struct", struct_name)
                )),
                None => return Object::Error(RuntimeError::UndefinedVariable(struct_name)),
            };
            
            let mut instance_fields = default_fields;
            
            // Override with provided field assignments
            for (Ident(field_name), expr) in field_assignments {
                let value = self_clone.eval_expr(expr).await;
                instance_fields.insert(field_name, value);
            }
            
            Object::Struct {
                name: struct_name,
                fields: instance_fields,
                methods,
            }
        }
    }

    pub fn eval_field_access(&mut self, object_expr: Expr, field_name: String) -> impl Future<Output = Object> + Send + '_  {
        let mut self_clone = self.clone();
        async move {
            let object = self_clone.eval_expr(object_expr).await;
            
            match object {
                Object::Struct { fields, .. } => {
                    match fields.get(&field_name) {
                        Some(value) => value.clone(),
                        None => Object::Error(RuntimeError::InvalidOperation(
                            format!("struct has no field '{}'", field_name)
                        ))
                    }
                }
                Object::Module { exports, .. } => {
                    match exports.get(&field_name) {
                        Some(value) => value.clone(),
                        None => Object::Error(RuntimeError::InvalidOperation(
                            format!("module has no export '{}'", field_name)
                        ))
                    }
                }
                other => Object::Error(RuntimeError::InvalidOperation(
                    format!("{} does not have fields", other.type_name())
                ))
            }
        }
    }

    pub fn eval_field_assign(&mut self, object_expr: Expr, field_name: String, value_expr: Expr) -> impl Future<Output = Object> + Send + '_  {
        let mut self_clone = self.clone();
        async move {
            let value = self_clone.eval_expr(value_expr).await;
            
            if let Expr::ThisExpr = object_expr {
                let current_this = self_clone.env.lock().unwrap().get("this");
                match current_this {
                    Some(Object::Struct { name, mut fields, methods }) => {
                        fields.insert(field_name, value.clone());
                        let updated_struct = Object::Struct { name, fields, methods };
                        self_clone.env.lock().unwrap().set("this", updated_struct);
                        return value;
                    }
                    Some(other) => {
                        return Object::Error(RuntimeError::InvalidOperation(
                            format!("{} does not have fields", other.type_name())
                        ));
                    }
                    None => {
                        return Object::Error(RuntimeError::InvalidOperation(
                            "'this' is not defined in current scope".to_string()
                        ));
                    }
                }
            }
            
            Object::Error(RuntimeError::InvalidOperation(
                "Can only assign to 'this.field', not other object fields".to_string()
            ))
        }
    }

    pub fn eval_method_call(&mut self, object_expr: Expr, method_name: String, args_expr: Vec<Expr>) -> impl Future<Output = Object> + Send + '_  {
        let mut self_clone = self.clone();
        async move {
            let var_name_if_ident = if let Expr::IdentExpr(Ident(ref name)) = object_expr {
                Some(name.clone())
            } else {
                None
            };
            
            let object = self_clone.eval_expr(object_expr).await;

            if let Object::Struct { ref methods, .. } = object {
                if let Some(method_obj) = methods.get(&method_name) {
                    let old_env = Arc::clone(&self_clone.env);
                    let mut new_env = Environment::new_with_outer(Arc::clone(&self_clone.env));
                    new_env.set("this", object.clone());
                    
                    self_clone.env = Arc::new(Mutex::new(new_env));
                    
                    let result = match method_obj.clone() {
                        Object::Function(params, body, _) => {
                            let mut args = Vec::new();
                            for e in args_expr {
                                args.push(self_clone.eval_expr(e).await);
                            }
                            self_clone.eval_fn_call_direct(args, params, body).await
                        }
                        _ => {
                            return Object::Error(RuntimeError::NotCallable(method_name));
                        }
                    };
                    
                    let modified_this = self_clone.env.lock().unwrap().get("this").unwrap_or(object.clone());
                    
                    self_clone.env = old_env;
                    
                    if let Some(var_name) = var_name_if_ident {
                        self_clone.env.lock().unwrap().set(&var_name, modified_this.clone());
                    }

                    let final_result = match result {
                        Object::Null => modified_this,
                        Object::ReturnValue(val) => *val,
                        other => other,
                    };
                    
                    return self_clone.returned(final_result);
                }
            }

            if let Object::Module { ref exports, .. } = object {
                if let Some(func_obj) = exports.get(&method_name) {
                    let mut args = Vec::new();
                    for e in &args_expr {
                        args.push(self_clone.eval_expr(e.clone()).await);
                    }
                    return match func_obj.clone() {
                        Object::Function(params, body, _) => {
                            self_clone.eval_fn_call_direct(args, params, body).await
                        }
                        Object::BuiltinStd(name, min, max, func) => {
                            if args.len() < min || args.len() > max {
                                return Object::Error(RuntimeError::InvalidOperation(
                                    format!("{} expects between {} and {} arguments, got {}", name, min, max, args.len())
                                ));
                            }
                            match func(args) {
                                Ok(obj) => obj,
                                Err(e) => Object::Error(e),
                            }
                        }
                        Object::BuiltinStdAsync(name, min, max, func) => {
                            if args.len() < min || args.len() > max {
                                return Object::Error(RuntimeError::InvalidOperation(
                                    format!("{} expects between {} and {} arguments, got {}", name, min, max, args.len())
                                ));
                            }
                            match func(args) {
                                Ok(obj) => obj,
                                Err(e) => Object::Error(e),
                            }
                        }
                        Object::Builtin(name, min, max, func) => {
                            if args.len() < min || args.len() > max {
                                return Object::Error(RuntimeError::InvalidOperation(
                                    format!("{} expects between {} and {} arguments, got {}", name, min, max, args.len())
                                ));
                            }
                            match func(args) {
                                Ok(obj) => obj,
                                Err(e) => Object::Error(RuntimeError::InvalidOperation(e)),
                            }
                        }
                        #[cfg(feature = "wasm")]
                        Object::WasmImportedFunction { module_name, func_name, instance } => {
                            self_clone.eval_wasm_fn_call(args_expr.clone(), module_name, func_name, instance).await
                        }
                        _ => Object::Error(RuntimeError::NotCallable(method_name)),
                    };
                } else {
                    return Object::Error(RuntimeError::InvalidOperation(
                        format!("module has no export '{}'", method_name)
                    ));
                }
            }
            
            let mut args = Vec::new();
            for e in args_expr {
                args.push(self_clone.eval_expr(e).await);
            }
            match BuiltinMethods::call_method(object, &method_name, args) {
                Ok(obj) => obj,
                Err(e) => Object::Error(e),
            }
        }
    }
}
