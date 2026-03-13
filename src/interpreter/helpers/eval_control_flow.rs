use std::sync::{Arc, Mutex};
use crate::{
    ast::ast::{Expr, Ident, Program, Stmt},
    errors::RuntimeError,
    interpreter::{
        env::Environment, obj::Object
    },
};

use super::super::eval::{Evaluator, EvalFuture};

impl Evaluator {
    pub fn eval_if(&mut self, cond: Expr, conse: Program, maybe_alter: Option<Program>) -> EvalFuture {
        let mut self_clone = self.clone();
        Box::pin(async move {
            let object = self_clone.eval_expr(cond).await;
            match self_clone.obj_to_bool(object) {
                Ok(b) => {
                    if b {
                        self_clone.eval_blockstmt(&conse).await
                    } else {
                        match maybe_alter {
                            Some(else_conse) => self_clone.eval_blockstmt(&else_conse).await,
                            _ => Object::Null,
                        }
                    }
                }
                Err(err) => err,
            }
        })
    }

    pub fn eval_while(&mut self, cond: Box<Expr>, body: Program) -> impl Future<Output = Object> + Send + '_ {
        let mut self_clone = self.clone();
        async move {
            loop {
                let cond_result = self_clone.eval_expr((*cond).clone()).await;
                match self_clone.obj_to_bool(cond_result) {
                    Ok(true) => {
                        let result = self_clone.eval_blockstmt(&body).await;
                        match result {
                            Object::Break => return Object::Null,
                            Object::Continue => continue,
                            Object::ReturnValue(_) => return result,
                            Object::Error(_) => return result,
                            _ => {}
                        }
                    }
                    Ok(false) => return Object::Null,
                    Err(e) => return e,
                }
            }
        }
    }

    pub fn eval_for(&mut self, ident: Ident, iterable: Box<Expr>, body: Program) -> impl Future<Output = Object> + Send + '_  {
        let mut self_clone = self.clone();
        async move {
            let iter_obj = self_clone.eval_expr(*iterable).await;

            let items = match iter_obj {
                Object::Array(arr) => arr,
                Object::String(s) => {
                    s.chars().map(|c| Object::String(c.to_string())).collect()
                }
                _ => {
                    return Object::Error(RuntimeError::InvalidOperation(format!("cannot iterate over {}", iter_obj.type_name())))
                }
            };

            let Ident(var_name) = ident;

            for item in items {
                self_clone.env.lock().unwrap().set(&var_name, item);

                let result = self_clone.eval_blockstmt(&body).await;
                match result {
                    Object::Break => return Object::Null,
                    Object::Continue => continue,
                    Object::ReturnValue(_) => return result,
                    Object::Error(_) => return  result,
                    _ => {}
                }
            }
            Object::Null
        }
    }

    pub fn eval_c_style_for(
        &mut self,
        init: Option<Box<Stmt>>,
        cond: Option<Box<Expr>>,
        update: Option<Box<Stmt>>,
        body: Program,
    ) -> impl Future<Output = Object> + Send + '_  {
        let mut self_clone = self.clone();
        async move {
            if let Some(init_stmt) = init {
                let result = self_clone.eval_statement(*init_stmt).await;
                if let Object::Error(_) = result {
                    return result;
                }
            }
            
            loop {
                let should_continue = if let Some(ref cond_expr) = cond {
                    match self_clone.eval_expr(cond_expr.as_ref().clone()).await {
                        Object::Boolean(b) => b,
                        Object::Error(e) => return Object::Error(e),
                        _ => return Object::Error(RuntimeError::TypeMismatch {
                            expected: "boolean".to_string(),
                            got: "non-boolean".to_string(),
                        }),
                    }
                } else {
                    true
                };
                
                if !should_continue {
                    break;
                }
                
                let result = self_clone.eval_blockstmt(&body).await;
                match result {
                    Object::Break => return Object::Null,
                    Object::Continue => {},
                    Object::ReturnValue(_) => return result,
                    Object::Error(_) => return result,
                    _ => {}
                }
                
                if let Some(ref update_stmt) = update {
                    let result = self_clone.eval_statement(update_stmt.as_ref().clone()).await;
                    if let Object::Error(_) = result {
                        return result;
                    }
                }
            }
            
            Object::Null
        }
    }

    pub fn eval_try_catch_expr(
        &mut self, 
        try_body: Program, 
        catch_ident: Option<Ident>, 
        catch_body: Option<Program>, 
        finally_body: Option<Program>
    ) -> impl Future<Output = Object> + Send + '_  {
        let mut self_clone = self.clone();
        async move {
            let mut try_result = self_clone.eval_blockstmt(&try_body).await;
            let final_result: Object;
        
            let caught_exception_obj = match try_result.clone() {
                Object::ThrownValue(ex) => Some(*ex),
                Object::Error(err) => Some(Object::Error(err)),
                _ => None,
            };

            if let Some(exception) = caught_exception_obj {
                if let Some(Ident(e_name)) = catch_ident {
                    if let Some(c_body) = catch_body {
                        let old_env = Arc::clone(&self_clone.env);
                        let mut new_env = Environment::new_with_outer(Arc::clone(&self_clone.env));
                        new_env.set(&e_name, exception);
                        self_clone.env = Arc::new(Mutex::new(new_env));
                        
                        try_result = self_clone.eval_blockstmt(&c_body).await;
                        self_clone.env = old_env;
                    }
                }
            }
        
            if let Some(f_body) = finally_body {
                let finally_result = self_clone.eval_blockstmt(&f_body).await;
                match finally_result {
                    Object::ReturnValue(_) | Object::Break | Object::Continue | Object::ThrownValue(_) | Object::Error(_) => {
                        final_result = finally_result;
                    },
                    _ => {
                        final_result = try_result;
                    }
                }
            } else {
                final_result = try_result;
            }

            if let Object::ThrownValue(_) = final_result {
                return final_result;
            }
        
            match final_result {
                Object::ReturnValue(_) | Object::Break | Object::Continue | Object::Error(_) => final_result,
                _ => self_clone.returned(final_result), 
            }
        }
    }
}
