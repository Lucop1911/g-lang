use num_bigint::BigInt;
use std::collections::HashMap;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex};

use crate::ast::ast::{Ident, Program};
use crate::errors::RuntimeError;
use crate::interpreter::env::Environment;

#[cfg(feature = "wasm")]
use crate::wasm::WasmInstance;

#[derive(Clone)]
pub enum Object {
    Integer(i64),
    BigInteger(BigInt),
    Float(f64),
    Boolean(bool),
    String(String),
    Array(Vec<Object>),
    Hash(HashMap<Object, Object>),
    Function(Vec<Ident>, Program, Arc<Mutex<Environment>>),
    AsyncFunction(Vec<Ident>, Program, Arc<Mutex<Environment>>),
    Builtin(String, usize, usize, BuiltinFunction),
    BuiltinStd(String, usize, usize, StdFunction),
    BuiltinStdAsync(String, usize, usize, AsyncStdFunction),
    WasmImportedFunction {
        module_name: String,
        func_name: String,
        instance: Arc<Mutex<Option<WasmInstance>>>,
    },
    Struct {
        name: String,
        fields: HashMap<String, Object>,
        methods: HashMap<String, Object>,
    },
    Module {
        name: String,
        exports: HashMap<String, Object>,
    },
    Null,
    ReturnValue(Box<Object>),
    Error(RuntimeError),
    Method(Vec<Ident>, Program, Arc<Mutex<Environment>>),
    Break,
    Continue,
    ThrownValue(Box<Object>),
    Future(Arc<Mutex<Option<std::pin::Pin<Box<dyn std::future::Future<Output = Result<Object, RuntimeError>> + Send + 'static>>>>>),
    #[cfg(feature = "wasm")]
    WasmModule {
        name: String,
        exports: HashMap<String, Object>,
        instance: Arc<Mutex<Option<WasmInstance>>>,
    },
}

pub type BuiltinFunction = fn(Vec<Object>) -> Result<Object, String>;
pub type StdFunction = fn(Vec<Object>) -> Result<Object, RuntimeError>;
pub type AsyncStdFunction = fn(Vec<Object>) -> Result<Object, RuntimeError>;

impl fmt::Debug for Object {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Object::Integer(i) => write!(f, "Integer({})", i),
            Object::BigInteger(b) => write!(f, "BigInteger({})", b),
            Object::Float(fl) => write!(f, "Float({})", fl),
            Object::Boolean(b) => write!(f, "Boolean({})", b),
            Object::String(s) => write!(f, "String(\"{}\")", s),
            Object::Array(a) => write!(f, "Array({:?})", a),
            Object::Hash(h) => write!(f, "Hash({:?})", h),
            Object::Function(p, b, _) => write!(f, "Function(params:{:?}, body:{:?})", p, b),
            Object::AsyncFunction(p, b, _) => {
                write!(f, "AsyncFunction(params:{:?}, body:{:?})", p, b)
            }
            Object::WasmImportedFunction {
                module_name,
                func_name,
                ..
            } => {
                write!(f, "WasmImportedFunction({}::{})", module_name, func_name)
            }
            Object::Builtin(n, _, _, _) => write!(f, "Builtin(\"{}\")", n),
            Object::BuiltinStd(n, _, _, _) => write!(f, "BuiltinStd(\"{}\")", n),
            Object::BuiltinStdAsync(n, _, _, _) => write!(f, "BuiltinStdAsync(\"{}\")", n),
            Object::Struct {
                name,
                fields,
                methods,
            } => write!(
                f,
                "Struct(name:{}, fields:{:?}, methods:{:?})",
                name, fields, methods
            ),
            Object::Module { name, exports } => write!(
                f,
                "Module(name:{}, exports:{:?})",
                name,
                exports.keys().collect::<Vec<_>>()
            ),
            Object::Null => write!(f, "Null"),
            Object::ReturnValue(o) => write!(f, "ReturnValue({:?})", o),
            Object::Error(e) => write!(f, "Error({:?})", e),
            Object::Method(p, b, _) => write!(f, "Method(params:{:?}, body:{:?})", p, b),
            Object::Break => write!(f, "Break"),
            Object::Continue => write!(f, "Continue"),
            Object::ThrownValue(o) => write!(f, "ThrownValue({:?})", o),
            Object::Future(_) => write!(f, "Future(_)"),
            #[cfg(feature = "wasm")]
            Object::WasmModule { name, exports, .. } => write!(
                f,
                "WasmModule(name:{}, exports:{:?})",
                name,
                exports.keys().collect::<Vec<_>>()
            ),
        }
    }
}

impl PartialEq for Object {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Object::Integer(a), Object::Integer(b)) => a == b,
            (Object::BigInteger(a), Object::BigInteger(b)) => a == b,
            (Object::Float(a), Object::Float(b)) => a == b,
            (Object::Boolean(a), Object::Boolean(b)) => a == b,
            (Object::String(a), Object::String(b)) => a == b,
            (Object::Array(a), Object::Array(b)) => a == b,
            (Object::Hash(a), Object::Hash(b)) => a == b,
            (Object::Null, Object::Null) => true,
            (Object::ReturnValue(a), Object::ReturnValue(b)) => a == b,
            (Object::Error(a), Object::Error(b)) => a == b,
            (Object::ThrownValue(a), Object::ThrownValue(b)) => a == b,
            (
                Object::Builtin(name_a, params_a, params_a1, _),
                Object::Builtin(name_b, params_b, params_b1, _),
            ) => name_a == name_b && params_a == params_b && params_a1 == params_b1,
            (
                Object::BuiltinStd(name_a, params_a, params_a1, _),
                Object::BuiltinStd(name_b, params_b, params_b1, _),
            ) => name_a == name_b && params_a == params_b && params_a1 == params_b1,
            (
                Object::BuiltinStdAsync(name_a, params_a, params_a1, _),
                Object::BuiltinStdAsync(name_b, params_b, params_b1, _),
            ) => name_a == name_b && params_a == params_b && params_a1 == params_b1,
            (Object::Function(params_a, body_a, _), Object::Function(params_b, body_b, _)) => {
                params_a == params_b && body_a == body_b
            }
            (
                Object::AsyncFunction(params_a, body_a, _),
                Object::AsyncFunction(params_b, body_b, _),
            ) => params_a == params_b && body_a == body_b,
            (
                Object::WasmImportedFunction {
                    module_name: a,
                    func_name: b,
                    ..
                },
                Object::WasmImportedFunction {
                    module_name: c,
                    func_name: d,
                    ..
                },
            ) => a == c && b == d,
            (Object::Break, Object::Break) => true,
            (Object::Continue, Object::Continue) => true,
            (Object::Future(_), Object::Future(_)) => false,
            (
                Object::Module {
                    name: a,
                    exports: e_a,
                },
                Object::Module {
                    name: b,
                    exports: e_b,
                },
            ) => a == b && e_a.keys().collect::<Vec<_>>() == e_b.keys().collect::<Vec<_>>(),
            #[cfg(feature = "wasm")]
            (
                Object::WasmModule {
                    name: a,
                    exports: e_a,
                    ..
                },
                Object::WasmModule {
                    name: b,
                    exports: e_b,
                    ..
                },
            ) => a == b && e_a.keys().collect::<Vec<_>>() == e_b.keys().collect::<Vec<_>>(),
            _ => false,
        }
    }
}

impl Object {
    pub fn is_returned(&self) -> bool {
        matches!(*self, Object::ReturnValue(_))
    }

    pub fn returned(self) -> Self {
        match self {
            Object::ReturnValue(o) => *o,
            o => o,
        }
    }

    pub fn type_name(&self) -> String {
        match self {
            Object::Integer(_) => "integer".to_string(),
            Object::BigInteger(_) => "bigInteger".to_string(),
            Object::Float(_) => "float".to_string(),
            Object::Boolean(_) => "boolean".to_string(),
            Object::String(_) => "string".to_string(),
            Object::Array(_) => "array".to_string(),
            Object::Hash(_) => "hash".to_string(),
            Object::Function(_, _, _) => "function".to_string(),
            Object::AsyncFunction(_, _, _) => "async function".to_string(),
            Object::WasmImportedFunction { .. } => "wasm imported function".to_string(),
            Object::Builtin(_, _, _, _) => "builtin function".to_string(),
            Object::BuiltinStd(_, _, _, _) => "builtin function".to_string(),
            Object::BuiltinStdAsync(_, _, _, _) => "async builtin function".to_string(),
            Object::Null => "null".to_string(),
            Object::ReturnValue(_) => "return value".to_string(),
            Object::Error(_) => "error".to_string(),
            Object::Method(_, _, _) => "method".to_string(),
            Object::Struct { name, .. } => format!("struct {}", name),
            Object::Module { name, .. } => format!("module {}", name),
            Object::Break => "break".to_string(),
            Object::Continue => "continue".to_string(),
            Object::ThrownValue(_) => "thrown value".to_string(),
            Object::Future(_) => "future".to_string(),
            #[cfg(feature = "wasm")]
            Object::WasmModule { name, .. } => format!("wasm module {}", name),
        }
    }
}

impl fmt::Display for Object {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            Object::Integer(ref i) => write!(f, "{}", i),
            Object::BigInteger(ref i) => write!(f, "{}", i),
            Object::Float(ref i) => write!(f, "{}", i),
            Object::Boolean(ref b) => {
                if *b {
                    write!(f, "true")
                } else {
                    write!(f, "false")
                }
            }
            Object::String(ref s) => write!(f, "{}", s),
            Object::Array(ref v) => {
                let mut fmt_string = String::new();
                fmt_string.push('[');
                for (i, o) in v.iter().enumerate() {
                    fmt_string.push_str(format!("{}", o).as_str());
                    if i < v.len() - 1 {
                        fmt_string.push_str(", ");
                    }
                }
                fmt_string.push(']');
                write!(f, "{}", fmt_string)
            }
            Object::Hash(ref hashmap) => {
                let mut fmt_string = String::new();
                fmt_string.push('{');
                for (i, (k, v)) in hashmap.iter().enumerate() {
                    fmt_string.push_str(format!("{} : {}", k, v).as_str());
                    if i < hashmap.len() - 1 {
                        fmt_string.push_str(", ");
                    }
                }
                fmt_string.push('}');
                write!(f, "{}", fmt_string)
            }
            Object::Function(_, _, _) => write!(f, "[function]"),
            Object::AsyncFunction(_, _, _) => write!(f, "[async function]"),
            Object::WasmImportedFunction {
                ref module_name,
                ref func_name,
                ..
            } => write!(f, "[wasm function: {}::{}]", module_name, func_name),
            Object::Builtin(ref name, _, _, _) => write!(f, "[built-in function: {}]", *name),
            Object::BuiltinStd(ref name, _, _, _) => write!(f, "[built-in function: {}]", *name),
            Object::BuiltinStdAsync(ref name, _, _, _) => {
                write!(f, "[async built-in function: {}]", *name)
            }
            Object::Null => write!(f, "null"),
            Object::ReturnValue(ref o) => write!(f, "{}", *o),
            Object::Error(ref e) => write!(f, "{}", e),
            Object::Method(_, _, _) => write!(f, "[method]"),
            Object::Struct {
                ref name,
                ref fields,
                ..
            } => {
                write!(f, "{}{{ ", name)?;
                for (i, (field_name, field_value)) in fields.iter().enumerate() {
                    write!(f, "{}: {}", field_name, field_value)?;
                    if i < fields.len() - 1 {
                        write!(f, ", ")?;
                    }
                }
                write!(f, " }}")
            }
            Object::Break => write!(f, "break"),
            Object::Continue => write!(f, "continue"),
            Object::ThrownValue(ref o) => write!(f, "Thrown: {}", *o),
            Object::Future(_) => write!(f, "[future]"),
            Object::Module { ref name, .. } => write!(f, "[module: {}]", name),
            #[cfg(feature = "wasm")]
            Object::WasmModule { ref name, .. } => write!(f, "[wasm module: {}]", name),
        }
    }
}

impl Eq for Object {}

#[allow(clippy::all)]
impl Hash for Object {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match *self {
            Object::Integer(ref i) => i.hash(state),
            Object::BigInteger(ref i) => i.hash(state),
            Object::Boolean(ref b) => b.hash(state),
            Object::String(ref s) => s.hash(state),
            Object::Function(ref params, ref body, _) => {
                params.hash(state);
                body.hash(state);
            }
            Object::AsyncFunction(ref params, ref body, _) => {
                params.hash(state);
                body.hash(state);
            }
            Object::Method(ref params, ref body, _) => {
                params.hash(state);
                body.hash(state);
            }
            _ => "".hash(state),
        }
    }
}
