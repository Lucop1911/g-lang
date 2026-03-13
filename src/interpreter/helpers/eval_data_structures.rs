use std::collections::HashMap;
use crate::{
    ast::ast::{Expr, Ident},
    errors::RuntimeError,
    interpreter::obj::Object
};
use super::super::eval::{Evaluator};

impl Evaluator {
    pub fn eval_index_assign(&mut self, target_expr: Expr, index_expr: Expr, value_expr: Expr) -> impl Future<Output = Object> + Send + '_  {
        let mut self_clone = self.clone();
        async move {
            let index = self_clone.eval_expr(index_expr).await;
            let value = self_clone.eval_expr(value_expr).await;
            
            match target_expr {
                Expr::IdentExpr(Ident(ref var_name)) => {
                    let current_value = match self_clone.env.lock().unwrap().get(var_name) {
                        Some(val) => val,
                        None => return Object::Error(RuntimeError::UndefinedVariable(var_name.clone())),
                    };
                    
                    let updated_value = match current_value {
                        Object::Array(mut arr) => {
                            match self_clone.obj_to_int(index.clone()) {
                                Ok(idx_num) => {
                                    if idx_num < 0 {
                                        return Object::Error(RuntimeError::IndexOutOfBounds {
                                            index: idx_num,
                                            length: arr.len(),
                                        });
                                    }
                                    let idx = idx_num as usize;
                                    if idx >= arr.len() {
                                        return Object::Error(RuntimeError::IndexOutOfBounds {
                                            index: idx_num,
                                            length: arr.len(),
                                        });
                                    }
                                    arr[idx] = value.clone();
                                    Object::Array(arr)
                                }
                                Err(err) => return err,
                            }
                        }
                        Object::Hash(mut hash) => {
                            let key = self_clone.obj_to_hash(index.clone());
                            if let Object::Error(_) = key {
                                return key;
                            }
                            hash.insert(key, value.clone());
                            Object::Hash(hash)
                        }
                        other => {
                            return Object::Error(RuntimeError::InvalidOperation(
                                format!("Cannot index into {}", other.type_name())
                            ));
                        }
                    };
                    
                    self_clone.env.lock().unwrap().set(var_name, updated_value);
                    value
                }
                Expr::ThisExpr => {
                    let current_this = self_clone.env.lock().unwrap().get("this");
                    match current_this {
                        Some(Object::Array(mut arr)) => {
                            match self_clone.obj_to_int(index.clone()) {
                                Ok(idx_num) => {
                                    if idx_num < 0 {
                                        return Object::Error(RuntimeError::IndexOutOfBounds {
                                            index: idx_num,
                                            length: arr.len(),
                                        });
                                    }
                                    let idx = idx_num as usize;
                                    if idx >= arr.len() {
                                        return Object::Error(RuntimeError::IndexOutOfBounds {
                                            index: idx_num,
                                            length: arr.len(),
                                        });
                                    }
                                    arr[idx] = value.clone();
                                    self_clone.env.lock().unwrap().set("this", Object::Array(arr));
                                    value
                                }
                                Err(err) => err,
                            }
                        }
                        Some(Object::Hash(mut hash)) => {
                            let key = self_clone.obj_to_hash(index.clone());
                            if let Object::Error(_) = key {
                                return key;
                            }
                            hash.insert(key, value.clone());
                            self_clone.env.lock().unwrap().set("this", Object::Hash(hash));
                            value
                        }
                        Some(other) => {
                            Object::Error(RuntimeError::InvalidOperation(
                                format!("Cannot index into {}", other.type_name())
                            ))
                        }
                        None => {
                            Object::Error(RuntimeError::InvalidOperation(
                                "'this' is not defined in current scope".to_string()
                            ))
                        }
                    }
                }
                _ => {
                    Object::Error(RuntimeError::InvalidOperation(
                        "Can only assign to variable[index] or this[index], not complex expressions".to_string()
                    ))
                }
            }
        }
    }

    pub fn eval_array(&mut self, exprs: Vec<Expr>) -> impl Future<Output = Object> + Send + '_  {
        let mut self_clone = self.clone();
        async move {
            let mut new_vec = Vec::new();
            for e in exprs {
                new_vec.push(self_clone.eval_expr(e).await);
            }
            Object::Array(new_vec)
        }
    }

    pub fn eval_hash(&mut self, hs: Vec<(Expr, Expr)>) -> impl Future<Output = Object> + Send + '_  {
        let mut self_clone = self.clone();
        async move {
            let mut hashmap = HashMap::new();
            
            for (key_expr, val_expr) in hs {
                let key = self_clone.eval_expr(key_expr).await;
                let val = self_clone.eval_expr(val_expr).await;
                
                match &key {
                    Object::Integer(_) | Object::Boolean(_) | Object::String(_) => {
                        hashmap.insert(key, val);
                    }
                    Object::Error(e) => return Object::Error(e.clone()),
                    _ => return Object::Error(RuntimeError::NotHashable(key.type_name())),
                }
            }
            
            Object::Hash(hashmap)
        }
    }

    pub fn eval_index(&mut self, target_exp: Expr, id_exp: Expr) -> impl Future<Output = Object> + Send + '_  {
        let mut self_clone = self.clone();
        async move {
            let target = self_clone.eval_expr(target_exp).await;
            let index = self_clone.eval_expr(id_exp).await;
            match target {
                Object::Array(arr) => match self_clone.obj_to_int(index) {
                    Ok(index_number) => {
                        if index_number < 0 {
                            return Object::Error(RuntimeError::IndexOutOfBounds {
                                index: index_number,
                                length: arr.len(),
                            });
                        }
                        let idx = index_number as usize;
                        if idx >= arr.len() {
                            return Object::Error(RuntimeError::IndexOutOfBounds {
                                index: index_number,
                                length: arr.len(),
                            });
                        }
                        arr.into_iter().nth(idx).unwrap_or(Object::Null)
                    }
                    Err(err) => err,
                },
                Object::Hash(mut hash) => {
                    let name = self_clone.obj_to_hash(index);
                    match name {
                        Object::Error(_) => name,
                        _ => hash.remove(&name).unwrap_or(Object::Null),
                    }
                }
                o => Object::Error(RuntimeError::NotHashable(o.type_name())),
            }
        }
    }
}