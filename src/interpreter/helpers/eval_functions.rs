use std::sync::{Arc, Mutex};
use tokio::task::JoinHandle; 
use crate::{
    ast::ast::{Expr, Ident, Program},
    errors::RuntimeError,
    interpreter::{
        env::Environment, obj::{BuiltinFunction, Object, StdFunction}
    },
    wasm::{WasmInstance, giulio_to_wasm_val, wasm_val_to_giulio},
};
use futures::stream::{FuturesUnordered, StreamExt};
use super::super::eval::{Evaluator, EvalFuture};
use wasmtime::Val;

impl Evaluator {
    pub fn eval_fn(&mut self, params: Vec<Ident>, body: Program) -> Object {
        Object::Function(params, body, Arc::clone(&self.env))
    }

    pub fn eval_method(&mut self, params: Vec<Ident>, body: Program) -> Object {
        Object::Method(params, body, Arc::clone(&self.env))
    }

    pub async fn eval_call(&mut self, fn_expr: Expr, args_expr: Vec<Expr>) -> Object {
        let fn_object = self.eval_expr(fn_expr).await;
        let fn_ = self.obj_to_func(fn_object);

        match fn_ {
            Object::Function(params, body, f_env) => {
                self.eval_fn_call(args_expr, params, body, &f_env).await
            }
            Object::AsyncFunction(params, body, f_env) => {
                let future_obj = self.eval_async_fn_call(args_expr, params, body, &f_env).await;
                if !self.in_async_context {
                    if let Object::Future(future_arc) = future_obj {
                        let future_to_await = {
                            let mut future_opt_guard = future_arc.lock().unwrap();
                            future_opt_guard.take()
                        };
                        if let Some(f) = future_to_await {
                            match f.await {
                                Ok(obj) => obj,
                                Err(e) => Object::Error(e),
                            }
                        } else {
                            Object::Error(RuntimeError::InvalidOperation(
                                "Cannot await a future that has already been awaited".to_string()
                            ))
                        }
                    } else {
                        future_obj
                    }
                } else {
                    future_obj
                }
            }
            Object::WasmImportedFunction { module_name, func_name, instance } => {
                self.eval_wasm_fn_call(args_expr, module_name, func_name, instance).await
            }

            Object::Builtin(_, min_params, max_params, b_fn) => {
                self.eval_builtin_call(args_expr, min_params, max_params, b_fn).await
            }
            Object::BuiltinStd(_, min_params, max_params, s_fn) => {
                self.eval_std_call(args_expr, min_params, max_params, s_fn).await
            }
            o_err => o_err,
        }
    }

    pub fn eval_fn_call(
        &mut self,
        args_expr: Vec<Expr>,
        params: Vec<Ident>,
        body: Program,
        f_env: &Arc<Mutex<Environment>>,
    ) -> EvalFuture {
        let mut self_clone = self.clone();
        let f_env_clone = Arc::clone(f_env);
        Box::pin(async move {
            if args_expr.len() < params.len() {
                return Object::Error(RuntimeError::WrongNumberOfArguments {
                    min: params.len(),
                    max: params.len(),
                    got: args_expr.len(),
                });
            }

            let mut args = Vec::new();
            for e in args_expr {
                args.push(self_clone.eval_expr(e).await);
            }

            let old_env = Arc::clone(&self_clone.env);
            let mut new_env = Environment::new_with_outer(f_env_clone);
            let zipped = params.into_iter().zip(args);
            for (Ident(name), o) in zipped {
                new_env.set(&name, o);
            }
            self_clone.env = Arc::new(Mutex::new(new_env));
            let object = self_clone.eval_blockstmt(body).await;
            self_clone.env = old_env;
            self_clone.returned(object)
        })
    }

    pub async fn eval_async_fn_call(
        &mut self,
        args_expr: Vec<Expr>,
        params: Vec<Ident>,
        body: Program,
        f_env: &Arc<Mutex<Environment>>,
    ) -> Object {
        if args_expr.len() < params.len() {
            return Object::Error(RuntimeError::WrongNumberOfArguments {
                min: params.len(),
                max: params.len(),
                got: args_expr.len(),
            });
        }
    
        let mut args_futures = FuturesUnordered::new();
        for e in args_expr {
            let mut evaluator = self.clone();
            args_futures.push(async move {
                evaluator.eval_expr(e).await
            });
        }
    
        let mut args: Vec<Object> = Vec::new();
        while let Some(arg) = args_futures.next().await {
            args.push(arg);
        }
        
        let f_env_clone = Arc::clone(f_env);
        let mut evaluator = self.clone();
    
        let future: JoinHandle<Object> = tokio::spawn(async move {
            let mut new_env = Environment::new_with_outer(f_env_clone);
            for (i, ident) in params.iter().enumerate() {
                let Ident(name) = ident;
                new_env.set(name, args[i].clone());
            }
            evaluator.env = Arc::new(Mutex::new(new_env));
            evaluator.in_async_context = true;

            let result = evaluator.eval_blockstmt(body).await;
            evaluator.returned(result)
        });

        let mapped_future = async {
            match future.await {
                Ok(obj) => Ok(obj),
                Err(e) => Err(RuntimeError::InvalidOperation(format!("Future panicked: {}", e))),
            }
        };
    
        Object::Future(Arc::new(Mutex::new(Some(Box::pin(mapped_future)))))
    }

    pub fn eval_builtin_call(
        &mut self,
        args_expr: Vec<Expr>,
        min_params: usize,
        max_params: usize,
        b_fn: BuiltinFunction,
    ) -> EvalFuture {
        let mut self_clone = self.clone();
        Box::pin(async move {
            if args_expr.len() < min_params || args_expr.len() > max_params {
                return Object::Error(RuntimeError::WrongNumberOfArguments {
                    min: min_params,
                    max: max_params,
                    got: args_expr.len(),
                });
            }

            let mut args = Vec::new();
            for e in args_expr {
                args.push(self_clone.eval_expr(e).await);
            }
            
            match b_fn(args) {
                Ok(obj) => obj,
                Err(e) => Object::Error(RuntimeError::InvalidArguments(e)),
            }
        })
    }

    pub fn eval_std_call(
        &mut self,
        args_expr: Vec<Expr>,
        min_params: usize,
        max_params: usize,
        s_fn: StdFunction,
    ) -> EvalFuture {
        let mut self_clone = self.clone();
        Box::pin(async move {
            if args_expr.len() < min_params || args_expr.len() > max_params {
                return Object::Error(RuntimeError::WrongNumberOfArguments {
                    min: min_params,
                    max: max_params,
                    got: args_expr.len(),
                });
            }

            let mut args = Vec::new();
            for e in args_expr {
                args.push(self_clone.eval_expr(e).await);
            }
            
            match s_fn(args) {
                Ok(obj) => obj,
                Err(e) => Object::Error(e),
            }
        })
    }

    pub fn eval_fn_call_direct(
        &mut self,
        args: Vec<Object>,
        params: Vec<Ident>,
        body: Program,
    ) -> EvalFuture {
        let mut self_clone = self.clone();
        Box::pin(async move {
            if args.len() != params.len() {
                return Object::Error(RuntimeError::WrongNumberOfArguments {
                    min: params.len(),
                    max: params.len(),
                    got: args.len(),
                });
            }

            let zipped = params.into_iter().zip(args);
            for (Ident(name), o) in zipped {
                self_clone.env.lock().unwrap().set(&name, o);
            }
            
            self_clone.eval_blockstmt(body).await
        })
    }

    pub async fn eval_wasm_fn_call(
        &mut self,
        args_expr: Vec<Expr>,
        _module_name: String,
        func_name: String,
        instance: Arc<Mutex<Option<WasmInstance>>>,
    ) -> Object {
        let mut self_clone = self.clone();
        
        let mut args = Vec::new();
        for e in args_expr {
            args.push(self_clone.eval_expr(e).await);
        }
        
        let result = {
            let mut registry = self_clone.module_registry.lock().unwrap();
            let store = match &mut registry.wasm_store {
                Some(store) => store,
                None => return Object::Error(RuntimeError::InvalidOperation(
                    "WASM store not available".to_string()
                )),
            };
            
            let inst = instance.lock().unwrap();
            let inst = match inst.as_ref() {
                Some(i) => i,
                None => return Object::Error(RuntimeError::InvalidOperation(
                    "WASM instance not available".to_string()
                )),
            };
            
            let memory = inst.get_memory();
            
            let wasm_args: Result<Vec<Val>, RuntimeError> = args
                .iter()
                .map(|arg| giulio_to_wasm_val(arg, memory, store))
                .collect();
            
            let wasm_args = match wasm_args {
                Ok(a) => a,
                Err(e) => return Object::Error(e),
            };
            
            inst.call_func_with_args(store, &func_name, &wasm_args)
                .map_err(RuntimeError::from)
                .and_then(|results| {
                    if results.is_empty() {
                        Ok(Object::Null)
                    } else {
                        wasm_val_to_giulio(&results[0])
                    }
                })
        };
        
        match result {
            Ok(obj) => obj,
            Err(e) => Object::Error(e),
        }
    }
}