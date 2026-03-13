use crate::errors::RuntimeError;
use crate::interpreter::obj::{HashMap, Object};
use ahash::HashMapExt;
use num_bigint::BigInt;
use num_traits::ToPrimitive;
use serde_json::{self, Number, Value};

fn object_to_json(obj: &Object) -> Result<Value, RuntimeError> {
    match obj {
        Object::Integer(i) => Ok(Value::Number(Number::from(*i))),

        Object::BigInteger(b) => {
            if let Some(i) = b.to_i64() {
                Ok(Value::Number(Number::from(i)))
            } else if let Some(u) = b.to_u64() {
                Ok(Value::Number(Number::from(u)))
            } else {
                match b.to_f64() {
                    Some(f) if f.is_finite() => {
                        Number::from_f64(f)
                            .map(Value::Number)
                            .ok_or_else(|| RuntimeError::InvalidOperation(format!(
                                "BigInteger {} cannot be accurately represented in JSON (precision loss)", 
                                b
                            )))
                    }
                    Some(_) => Err(RuntimeError::InvalidOperation(format!("BigInteger {} converts to non-finite float", b))),
                    None => Err(RuntimeError::InvalidOperation(format!("BigInteger {} is too large for JSON representation", b))),
                }
            }
        }

        Object::Float(f) => {
            if !f.is_finite() {
                return Err(RuntimeError::InvalidOperation(format!(
                    "Cannot serialize {} to JSON (JSON doesn't support infinity or NaN)",
                    if f.is_nan() {
                        "NaN"
                    } else if f.is_infinite() {
                        "infinity"
                    } else {
                        "invalid float"
                    }
                )));
            }

            Number::from_f64(*f).map(Value::Number).ok_or_else(|| {
                RuntimeError::InvalidOperation(format!(
                    "Float {} cannot be represented as JSON number",
                    f
                ))
            })
        }

        Object::Boolean(b) => Ok(Value::Bool(*b)),
        Object::String(s) => Ok(Value::String(s.clone())),

        Object::Array(arr) => {
            let mut json_arr = Vec::with_capacity(arr.len());
            for (_idx, item) in arr.iter().enumerate() {
                // I just propagate the error, adding context might be nice but RuntimeError structure is fixed
                // for now i just propagate
                json_arr.push(object_to_json(item)?);
            }
            Ok(Value::Array(json_arr))
        }

        Object::Hash(map) => {
            let mut json_map = serde_json::Map::new();
            for (k, v) in map {
                let key_str = match k {
                    Object::String(s) => s.clone(),
                    Object::Integer(i) => i.to_string(),
                    Object::BigInteger(b) => b.to_string(),
                    Object::Boolean(b) => b.to_string(),
                    _ => {
                        return Err(RuntimeError::InvalidOperation(format!(
                            "Hash key of type '{}' cannot be converted to JSON string key",
                            k.type_name()
                        )));
                    }
                };

                json_map.insert(key_str.clone(), object_to_json(v)?);
            }
            Ok(Value::Object(json_map))
        }

        Object::Struct {
            name: _, fields, ..
        } => {
            let mut json_map = serde_json::Map::new();
            for (k, v) in fields {
                json_map.insert(k.clone(), object_to_json(v)?);
            }
            Ok(Value::Object(json_map))
        }

        Object::Null => Ok(Value::Null),

        _ => Err(RuntimeError::InvalidOperation(format!(
            "Cannot serialize type '{}' to JSON (unsupported type)",
            obj.type_name()
        ))),
    }
}

fn json_to_object(val: Value) -> Object {
    match val {
        Value::Null => Object::Null,
        Value::Bool(b) => Object::Boolean(b),

        Value::Number(n) => {
            // Handle different types
            if let Some(i) = n.as_i64() {
                // Prefer i64 for integers that fit
                Object::Integer(i)
            } else if let Some(u) = n.as_u64() {
                // Handle unsigned integers
                if u <= i64::MAX as u64 {
                    Object::Integer(u as i64)
                } else {
                    // Large unsigned integer becomes BigInteger
                    Object::BigInteger(BigInt::from(u))
                }
            } else if let Some(f) = n.as_f64() {
                // Floating point number
                Object::Float(f)
            } else {
                // Fallback - wont happen often
                Object::Float(0.0)
            }
        }

        Value::String(s) => Object::String(s),

        Value::Array(arr) => {
            let objects: Vec<Object> = arr.into_iter().map(json_to_object).collect();
            Object::Array(objects)
        }

        Value::Object(map) => {
            let mut hash = HashMap::with_capacity(map.len());
            for (k, v) in map {
                hash.insert(Object::String(k), json_to_object(v));
            }
            Object::Hash(hash)
        }
    }
}

pub fn json_serialize(args: Vec<Object>) -> Result<Object, RuntimeError> {
    if args.len() != 1 {
        return Err(RuntimeError::WrongNumberOfArguments {
            min: 1,
            max: 1,
            got: args.len(),
        });
    }

    match object_to_json(&args[0]) {
        Ok(val) => Ok(Object::String(val.to_string())),
        Err(e) => Err(e),
    }
}

pub fn json_deserialize(args: Vec<Object>) -> Result<Object, RuntimeError> {
    if args.len() != 1 {
        return Err(RuntimeError::WrongNumberOfArguments {
            min: 1,
            max: 1,
            got: args.len(),
        });
    }

    match &args[0] {
        Object::String(s) => match serde_json::from_str::<Value>(s) {
            Ok(val) => Ok(json_to_object(val)),
            Err(e) => Err(RuntimeError::InvalidArguments(format!(
                "JSON parse error: {}",
                e
            ))),
        },
        o => Err(RuntimeError::TypeMismatch {
            expected: "string".to_string(),
            got: o.type_name(),
        }),
    }
}

pub fn json_prettify(args: Vec<Object>) -> Result<Object, RuntimeError> {
    if args.len() != 1 {
        return Err(RuntimeError::WrongNumberOfArguments {
            min: 1,
            max: 1,
            got: args.len(),
        });
    }

    match &args[0] {
        Object::String(s) => match serde_json::from_str::<Value>(s) {
            Ok(val) => match serde_json::to_string_pretty(&val) {
                Ok(pretty_s) => Ok(Object::String(pretty_s)),
                Err(e) => Err(RuntimeError::InvalidOperation(format!(
                    "JSON prettify error: {}",
                    e
                ))),
            },
            Err(e) => Err(RuntimeError::InvalidArguments(format!(
                "JSON parse error: {}",
                e
            ))),
        },
        o => Err(RuntimeError::TypeMismatch {
            expected: "string".to_string(),
            got: o.type_name(),
        }),
    }
}

pub fn json_validate(args: Vec<Object>) -> Result<Object, RuntimeError> {
    if args.len() != 1 {
        return Err(RuntimeError::WrongNumberOfArguments {
            min: 1,
            max: 1,
            got: args.len(),
        });
    }

    match &args[0] {
        Object::String(s) => Ok(Object::Boolean(serde_json::from_str::<Value>(s).is_ok())),
        o => Err(RuntimeError::TypeMismatch {
            expected: "string".to_string(),
            got: o.type_name(),
        }),
    }
}
