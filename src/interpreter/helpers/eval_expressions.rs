use num_bigint::BigInt;
use crate::{
    ast::ast::{Expr, Ident, Infix, Literal, Prefix},
    errors::RuntimeError,
    interpreter::obj::Object
};

use super::super::eval::Evaluator;

impl Evaluator {
    pub fn eval_this(&mut self) -> Object {
        match self.env.lock().unwrap().get("this") {
            Some(obj) => obj,
            None => Object::Error(RuntimeError::InvalidOperation(
                "'this' can only be used inside a method".to_string()
            )),
        }
    }

    pub fn eval_ident(&mut self, ident: Ident) -> Object {
        let Ident(name) = ident;
        let borrow_env = self.env.lock().unwrap();
        let var = borrow_env.get(&name);
        match var {
            Some(o) => o,
            None => Object::Error(RuntimeError::UndefinedVariable(name)),
        }
    }

    pub fn eval_literal(&mut self, literal: Literal) -> Object {
        match literal {
            Literal::IntLiteral(i) => Object::Integer(i),
            Literal::BigIntLiteral(big) => Object::BigInteger(big),
            Literal::FloatLitera(f) => Object::Float(f),
            Literal::BoolLiteral(b) => Object::Boolean(b),
            Literal::StringLiteral(s) => Object::String(s),
            Literal::NullLiteral => Object::Null,
        }
    }

    pub fn eval_prefix(&mut self, prefix: Prefix, expr: Expr) -> impl Future<Output = Object> + Send + '_  {
        let mut self_clone = self.clone();
        async move {
            let object = self_clone.eval_expr(expr).await;
            match prefix {
                Prefix::Not => match self_clone.obj_to_bool(object) {
                    Ok(b) => Object::Boolean(!b),
                    Err(err) => err,
                },
                Prefix::PrefixPlus => {
                    match object {
                        Object::Integer(_) | Object::BigInteger(_) | Object::Float(_) => object,
                        Object::Error(e) => Object::Error(e),
                        o => Object::Error(RuntimeError::TypeMismatch {
                            expected: "integer".to_string(),
                            got: o.type_name(),
                        })
                    }
                },
                Prefix::PrefixMinus => {
                    match object {
                        Object::Integer(i) => {
                            match i.checked_neg() {
                                Some(result) => Object::Integer(result),
                                None => Object::BigInteger(-BigInt::from(i))
                            }
                        }
                        Object::BigInteger(big) => self_clone.normalize_int(-big),
                        Object::Float(f) => Object::Float(-f),
                        Object::Error(e) => Object::Error(e),
                        o => Object::Error(RuntimeError::TypeMismatch {
                            expected: "integer".to_string(),
                            got: o.type_name(),
                        })
                    }
                },
            }
        }
    }

    pub fn eval_infix(&mut self, infix: Infix, expr1: Expr, expr2: Expr) -> impl Future<Output = Object> + Send + '_  {
        let mut self_clone = self.clone();
        async move {
            let object1 = self_clone.eval_expr(expr1).await;
            let object2 = self_clone.eval_expr(expr2).await;
            
            match infix {
                Infix::Plus => self_clone.object_add(object1, object2),
                Infix::Minus => self_clone.object_subtract(object1, object2),
                Infix::Divide => self_clone.object_divide(object1, object2),
                Infix::Multiply => self_clone.object_multiply(object1, object2),
                Infix::Modulo => self_clone.object_modulo(object1, object2),
                Infix::Equal => Object::Boolean(object1 == object2),
                Infix::NotEqual => Object::Boolean(object1 != object2),
                Infix::GreaterThanEqual => self_clone.object_compare_gte(object1, object2),
                Infix::GreaterThan => self_clone.object_compare_gt(object1, object2),
                Infix::LessThanEqual => self_clone.object_compare_lte(object1, object2),
                Infix::LessThan => self_clone.object_compare_lt(object1, object2),
                Infix::And => {
                    let b1 = self_clone.obj_to_bool(object1);
                    let b2 = self_clone.obj_to_bool(object2);
                    match (b1, b2) {
                        (Ok(b1), Ok(b2)) => Object::Boolean(b1 && b2),
                        (Err(err), _) | (_, Err(err)) => err,
                    }
                }
                Infix::Or => {
                    let b1 = self_clone.obj_to_bool(object1);
                    let b2 = self_clone.obj_to_bool(object2);
                    match (b1, b2) {
                        (Ok(b1), Ok(b2)) => Object::Boolean(b1 || b2),
                        (Err(err), _) | (_, Err(err)) => err,
                    }
                }
            }
        }
    }
}
