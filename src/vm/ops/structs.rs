//! Struct operations: build, get field, set field, method call.

use crate::vm::chunk::Chunk;
use crate::vm::runtime::builtins::methods::BuiltinMethods;
use crate::vm::runtime::runtime_errors::RuntimeError;
use crate::vm::obj::{EnumObject, EnumPayload, HashMap, Object, StructObject};
use ahash::HashMapExt;

/// Result of method call execution
pub enum MethodCallResult {
    /// Method needs to be called: its function object is on the stack
    /// The usize is the argument count (including `this`)
    NeedsCall(usize),
    /// Method result is already computed and on the stack
    Done,
    /// Error occurred
    Error(Object),
}

pub fn execute_build_struct(
    stack: &mut Vec<Object>,
    field_count: u8,
) {
    let field_count = field_count as usize;
    if stack.len() < field_count + 2 {
        stack.push(Object::Error(Box::new(RuntimeError::InvalidOperation(
            "Stack underflow on BuildStruct".to_string(),
        ))));
        return;
    }

    let template = match stack.pop() {
        Some(Object::Struct(t)) => t,
        Some(_) | None => {
            stack.push(Object::Error(Box::new(RuntimeError::InvalidOperation(
                "Struct template must be a Struct constant".to_string(),
            ))));
            return;
        }
    };

    let name = template.name.clone();
    let default_fields = &template.fields;
    let methods = template.methods;

    let mut fields = HashMap::new();
    for _ in 0..field_count {
        let value = stack.pop().unwrap();
        let field_name_obj = stack.pop().unwrap();
        let field_name = match field_name_obj {
            Object::String(s) => s,
            _ => {
                stack.push(Object::Error(Box::new(RuntimeError::InvalidOperation(
                    "Struct field name must be a string".to_string(),
                ))));
                return;
            }
        };
        fields.insert(field_name, value);
    }

    for (field_name, value) in default_fields {
        if !fields.contains_key(field_name) {
            fields.insert(field_name.clone(), value.clone());
        }
    }

    stack.push(Object::Struct(Box::new(StructObject {
        name,
        fields,
        methods,
    })));
}

pub fn execute_get_field(stack: &mut Vec<Object>) {
    let field_name_obj = match stack.pop() {
        Some(v) => v,
        None => {
            return stack.push(Object::Error(Box::new(RuntimeError::InvalidOperation(
                "Stack underflow on GetField".to_string(),
            ))))
        }
    };
    let field_name = match field_name_obj {
        Object::String(s) => s,
        _ => {
            return stack.push(Object::Error(Box::new(RuntimeError::InvalidOperation(
                "Field name must be a string".to_string(),
            ))))
        }
    };
    let struct_obj = match stack.pop() {
        Some(v) => v,
        None => {
            return stack.push(Object::Error(Box::new(RuntimeError::InvalidOperation(
                "Stack underflow on GetField".to_string(),
            ))))
        }
    };

    let result = match struct_obj {
        Object::Struct(s) => s.fields.get(&field_name).cloned().unwrap_or(Object::Null),
        Object::Module(m) => m.exports.get(&field_name).cloned().unwrap_or(Object::Null),
        other => Object::Error(Box::new(RuntimeError::InvalidOperation(format!(
            "Cannot get field from {}",
            other.type_name(),
        )))),
    };

    stack.push(result);
}

pub fn execute_set_field(stack: &mut Vec<Object>) {
    let value = match stack.pop() {
        Some(v) => v,
        None => {
            return stack.push(Object::Error(Box::new(RuntimeError::InvalidOperation(
                "Stack underflow on SetField".to_string(),
            ))))
        }
    };
    let field_name_obj = match stack.pop() {
        Some(v) => v,
        None => {
            return stack.push(Object::Error(Box::new(RuntimeError::InvalidOperation(
                "Stack underflow on SetField".to_string(),
            ))))
        }
    };
    let field_name = match field_name_obj {
        Object::String(s) => s,
        _ => {
            return stack.push(Object::Error(Box::new(RuntimeError::InvalidOperation(
                "Field name must be a string".to_string(),
            ))))
        }
    };
    let struct_obj = match stack.pop() {
        Some(v) => v,
        None => {
            return stack.push(Object::Error(Box::new(RuntimeError::InvalidOperation(
                "Stack underflow on SetField".to_string(),
            ))))
        }
    };

    let result = match struct_obj {
        Object::Struct(mut s) => {
            s.fields.insert(field_name, value);
            Object::Struct(s)
        }
        other => Object::Error(Box::new(RuntimeError::InvalidOperation(format!(
            "Cannot set field on {}",
            other.type_name(),
        )))),
    };

    stack.push(result);
}

pub fn execute_call_method(
    stack: &mut Vec<Object>,
    argc: usize,
) -> Result<MethodCallResult, RuntimeError> {
    // Stack layout before this function:
    // [... object, method_name, arg1, arg2, ..., argN]

    // We need to pop arguments first (they're on top), then method_name, then object
    let mut args = Vec::new();
    for _ in 0..argc {
        match stack.pop() {
            Some(v) => args.push(v),
            None => {
                return Ok(MethodCallResult::Error(Object::Error(Box::new(
                    RuntimeError::InvalidOperation("Stack underflow: missing argument".to_string()),
                ))))
            }
        }
    }
    args.reverse(); // Restore original order

    // Now pop method_name and object
    let method_name_obj = match stack.pop() {
        Some(v) => v,
        None => {
            return Ok(MethodCallResult::Error(Object::Error(Box::new(
                RuntimeError::InvalidOperation("Stack underflow: missing method name".to_string()),
            ))))
        }
    };

    let method_name = match method_name_obj {
        Object::String(s) => s,
        _ => {
            return Ok(MethodCallResult::Error(Object::Error(Box::new(
                RuntimeError::InvalidOperation("Method name must be a string".to_string()),
            ))))
        }
    };

    let struct_obj = match stack.pop() {
        Some(v) => v,
        None => {
            return Ok(MethodCallResult::Error(Object::Error(Box::new(
                RuntimeError::InvalidOperation("Stack underflow: missing object".to_string()),
            ))))
        }
    };

    match &struct_obj {
        Object::Struct(s) => {
            if let Some(method) = s.methods.get(&method_name) {
                stack.push(method.clone());
                // Prepend 'this' (the struct instance) to the argument list.
                stack.push(struct_obj.clone());
                for arg in args {
                    stack.push(arg);
                }
                Ok(MethodCallResult::NeedsCall(argc + 1))
            } else {
                Ok(MethodCallResult::Error(Object::Error(Box::new(
                    RuntimeError::InvalidOperation(format!("Method '{}' not found", method_name)),
                ))))
            }
        }
        Object::Module(m) => {
            if let Some(method) = m.exports.get(&method_name) {
                stack.push(method.clone());
                for arg in args {
                    stack.push(arg);
                }
                Ok(MethodCallResult::NeedsCall(argc))
            } else {
                Ok(MethodCallResult::Error(Object::Error(Box::new(
                    RuntimeError::InvalidOperation(format!(
                        "Method '{}' not found on module",
                        method_name
                    )),
                ))))
            }
        }
        _ => {
            // Handle built-in methods for other types
            match BuiltinMethods::call_method(
                struct_obj,
                &method_name,
                args,
            ) {
                Ok(result) => {
                    stack.push(result);
                    Ok(MethodCallResult::Done)
                }
                Err(e) => Ok(MethodCallResult::Error(Object::Error(Box::new(e)))),
            }
        }
    }
}

pub fn execute_build_enum_struct(
    stack: &mut Vec<Object>,
    field_count: u8,
) {
    let field_count = field_count as usize;
    if stack.len() < field_count * 2 + 2 {
        stack.push(Object::Error(Box::new(RuntimeError::InvalidOperation(
            "Stack underflow on BuildEnumStruct".to_string(),
        ))));
        return;
    }

    let variant_name = match stack.pop() {
        Some(Object::String(s)) => s,
        _ => {
            stack.push(Object::Error(Box::new(RuntimeError::InvalidOperation(
                "Enum variant name must be a string".to_string(),
            ))));
            return;
        }
    };

    let enum_name = match stack.pop() {
        Some(Object::String(s)) => s,
        _ => {
            stack.push(Object::Error(Box::new(RuntimeError::InvalidOperation(
                "Enum name must be a string".to_string(),
            ))));
            return;
        }
    };

    let mut fields = HashMap::new();
    for _ in 0..field_count {
        let value = stack.pop().unwrap();
        let field_name_obj = stack.pop().unwrap();
        let field_name = match field_name_obj {
            Object::String(s) => s,
            _ => {
                stack.push(Object::Error(Box::new(RuntimeError::InvalidOperation(
                    "Enum struct field name must be a string".to_string(),
                ))));
                return;
            }
        };
        fields.insert(field_name, value);
    }

    stack.push(Object::Enum(Box::new(EnumObject {
        enum_name,
        variant_name,
        payload: EnumPayload::Struct(fields),
    })));
}

pub fn execute_build_enum_tuple(
    stack: &mut Vec<Object>,
    arg_count: u8,
) {
    let arg_count = arg_count as usize;
    if stack.len() < arg_count + 2 {
        stack.push(Object::Error(Box::new(RuntimeError::InvalidOperation(
            "Stack underflow on BuildEnumTuple".to_string(),
        ))));
        return;
    }

    let variant_name = match stack.pop() {
        Some(Object::String(s)) => s,
        _ => {
            stack.push(Object::Error(Box::new(RuntimeError::InvalidOperation(
                "Enum variant name must be a string".to_string(),
            ))));
            return;
        }
    };

    let enum_name = match stack.pop() {
        Some(Object::String(s)) => s,
        _ => {
            stack.push(Object::Error(Box::new(RuntimeError::InvalidOperation(
                "Enum name must be a string".to_string(),
            ))));
            return;
        }
    };

    let mut args = Vec::new();
    for _ in 0..arg_count {
        args.push(stack.pop().unwrap());
    }
    args.reverse();

    stack.push(Object::Enum(Box::new(EnumObject {
        enum_name,
        variant_name,
        payload: EnumPayload::Tuple(args),
    })));
}

pub fn execute_match_enum(
    stack: &mut Vec<Object>,
    chunk: &Chunk,
    enum_name_idx: u16,
    variant_name_idx: u16,
) {
    let value = match stack.pop() {
        Some(v) => v,
        None => {
            stack.push(Object::Error(Box::new(RuntimeError::InvalidOperation(
                "Stack underflow on MatchEnum".to_string(),
            ))));
            return;
        }
    };

    let variant_name = match chunk.constants.get(variant_name_idx as usize) {
        Some(Object::String(s)) => s,
        _ => {
            stack.push(Object::Error(Box::new(RuntimeError::InvalidOperation(
                "MatchEnum variant name must be a string".to_string(),
            ))));
            return;
        }
    };

    let enum_name = if enum_name_idx != u16::MAX {
        match chunk.constants.get(enum_name_idx as usize) {
            Some(Object::String(s)) => Some(s),
            _ => {
                stack.push(Object::Error(Box::new(RuntimeError::InvalidOperation(
                    "MatchEnum enum name must be a string".to_string(),
                ))));
                return;
            }
        }
    } else {
        None
    };

    let matches = match value {
        Object::Enum(e) => {
            let variant_matches = e.variant_name == *variant_name;
            let enum_matches = match enum_name {
                Some(name) => e.enum_name == *name,
                None => true,
            };
            variant_matches && enum_matches
        }
        _ => false,
    };

    stack.push(Object::Boolean(matches));
}

pub fn execute_destructure_enum_tuple(
    stack: &mut Vec<Object>,
    field_index: u8,
) {
    let value = match stack.pop() {
        Some(v) => v,
        None => {
            stack.push(Object::Error(Box::new(RuntimeError::InvalidOperation(
                "Stack underflow on DestructureEnumTuple".to_string(),
            ))));
            return;
        }
    };

    let result = match value {
        Object::Enum(e) => match &e.payload {
            EnumPayload::Tuple(args) => {
                args.get(field_index as usize).cloned().unwrap_or(Object::Null)
            }
            _ => Object::Error(Box::new(RuntimeError::InvalidOperation(
                "Cannot destructure non-tuple enum variant as tuple".to_string(),
            ))),
        },
        other => Object::Error(Box::new(RuntimeError::InvalidOperation(format!(
            "Cannot destructure non-enum value {} as tuple",
            other.type_name(),
        )))),
    };

    stack.push(result);
}

pub fn execute_destructure_enum_struct(
    stack: &mut Vec<Object>,
    chunk: &Chunk,
    field_name_idx: u16,
) {
    let value = match stack.pop() {
        Some(v) => v,
        None => {
            stack.push(Object::Error(Box::new(RuntimeError::InvalidOperation(
                "Stack underflow on DestructureEnumStruct".to_string(),
            ))));
            return;
        }
    };

    let field_name = match chunk.constants.get(field_name_idx as usize) {
        Some(Object::String(s)) => s,
        _ => {
            stack.push(Object::Error(Box::new(RuntimeError::InvalidOperation(
                "DestructureEnumStruct field name must be a string".to_string(),
            ))));
            return;
        }
    };

    let result = match value {
        Object::Enum(e) => match &e.payload {
            EnumPayload::Struct(fields) => {
                fields.get(field_name).cloned().unwrap_or(Object::Null)
            }
            _ => Object::Error(Box::new(RuntimeError::InvalidOperation(
                "Cannot destructure non-struct enum variant as struct".to_string(),
            ))),
        },
        other => Object::Error(Box::new(RuntimeError::InvalidOperation(format!(
            "Cannot destructure non-enum value {} as struct",
            other.type_name(),
        )))),
    };

    stack.push(result);
}