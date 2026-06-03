//! Runtime value representation for the G-lang interpreter.
//!
//! [`Object`] is the central type that every expression evaluates to.
//! It covers primitives (integers, floats, strings, booleans), collections
//! (arrays, hash maps), first-class functions (sync and async), builtin
//! functions, structs, modules, and control-flow sentinels (Break, Continue,
//! ReturnValue, ThrownValue).

use ahash::AHasher;
use num_bigint::BigInt;
use std::fmt;
use std::hash::{BuildHasherDefault, Hash, Hasher};
use std::sync::{Arc, Mutex};

use crate::ast::ast::Ident;
use crate::vm::runtime::env::Environment;
use crate::vm::runtime::runtime_errors::RuntimeError;

#[cfg(feature = "wasm")]
use crate::wasm::WasmInstance;

pub type HashMap<K, V> = std::collections::HashMap<K, V, BuildHasherDefault<AHasher>>;

/// Struct instance with fields and methods. Boxed to reduce the size of the Object enum.
#[derive(Clone)]
pub struct StructObject {
    pub name: String,
    pub fields: HashMap<String, Object>,
    pub methods: HashMap<String, Object>,
}

/// Represents an instance of an enum variant.
///
/// Contains the name of the enum, the name of the variant, and any
/// associated data (payload).
#[derive(Clone, PartialEq)]
pub struct EnumObject {
    pub enum_name: String,
    pub variant_name: String,
    pub payload: EnumPayload,
}

/// The data associated with an enum variant instance.
#[derive(Clone, Debug, PartialEq)]
pub enum EnumPayload {
    /// A simple variant without any data
    Unit,
    /// A tuple-like variant
    Tuple(Vec<Object>),
    /// A struct-like variant
    Struct(HashMap<String, Object>),
}

/// Loaded module with its exported bindings.
#[derive(Clone)]
pub struct ModuleObject {
    pub name: String,
    pub exports: HashMap<String, Object>,
}

/// Data for a user-defined function.
#[derive(Clone)]
pub struct FunctionData {
    pub params: Vec<Ident>,
    pub chunk: Arc<crate::vm::chunk::Chunk>,
    pub env: Arc<Mutex<Environment>>,
    pub local_names: Vec<String>,
}

/// Data for a simple builtin function.
#[derive(Clone)]
pub struct BuiltinData {
    pub name: String,
    pub min_params: usize,
    pub max_params: usize,
    pub func: BuiltinFunction,
}

/// Data for a standard builtin function.
#[derive(Clone)]
pub struct BuiltinStdData {
    pub name: String,
    pub min_params: usize,
    pub max_params: usize,
    pub func: StdFunction,
}

/// Data for an async standard builtin function.
#[derive(Clone)]
pub struct BuiltinStdAsyncData {
    pub name: String,
    pub min_params: usize,
    pub max_params: usize,
    pub func: AsyncStdFunction,
}

/// Data for a WASM imported function.
#[derive(Clone)]
pub struct WasmFunctionData {
    pub module_name: String,
    pub func_name: String,
    pub instance: Arc<Mutex<Option<WasmInstance>>>,
}

/// Data for a WASM module.
#[cfg(feature = "wasm")]
#[derive(Clone)]
pub struct WasmModuleData {
    pub name: String,
    pub exports: HashMap<String, Object>,
    pub instance: Arc<Mutex<Option<WasmInstance>>>,
}

/// The universal value type of the G-lang runtime.
///
/// Every expression in a G-lang program evaluates to one of these variants.
/// Large variants are boxed to keep the enum size small (typically 24-32 bytes
/// on 64-bit systems instead of 72+ bytes).
#[derive(Clone)]
pub enum Object {
    Integer(i64),
    /// Arbitrary-precision integer. Boxed to reduce enum size.
    BigInteger(Box<BigInt>),
    Float(f64),
    Boolean(bool),
    String(String),
    /// Dynamic array. Boxed to reduce enum size (Vec is 24 bytes).
    Array(Box<Vec<Object>>),
    /// Hash map with Object keys. Boxed to reduce enum size (HashMap is ~48+ bytes).
    Hash(Box<HashMap<Object, Object>>),
    /// User-defined function. Boxed to reduce size.
    Function(Box<FunctionData>),
    /// Async user-defined function. Boxed to reduce size.
    AsyncFunction(Box<FunctionData>),
    /// Builtin function implemented in Rust.
    Builtin(Box<BuiltinData>),
    /// Builtin function with RuntimeError-based error handling.
    BuiltinStd(Box<BuiltinStdData>),
    /// Async builtin function.
    BuiltinStdAsync(Box<BuiltinStdAsyncData>),
    /// WASM imported function.
    WasmImportedFunction(Box<WasmFunctionData>),
    /// Struct instance with fields and methods.
    Struct(Box<StructObject>),
    /// Enum variant instance.
    Enum(Box<EnumObject>),
    /// Loaded module with exported bindings.
    Module(Box<ModuleObject>),
    Null,
    /// Wraps a value returned from a function.
    ReturnValue(Box<Object>),
    /// Runtime error. Boxed to reduce enum size.
    Error(Box<RuntimeError>),
    /// Method bound to a struct instance.
    Method(Box<FunctionData>),
    /// Control-flow sentinel: exits the innermost loop.
    Break,
    /// Control-flow sentinel: skips to the next loop iteration.
    Continue,
    /// Value passed to `throw`.
    ThrownValue(Box<Object>),
    /// Unresolved async computation.
    Future(
        Arc<
            Mutex<
                Option<
                    std::pin::Pin<
                        Box<
                            dyn std::future::Future<Output = Result<Object, RuntimeError>>
                                + Send
                                + 'static,
                        >,
                    >,
                >,
            >,
        >,
    ),
    /// Compiled WASM module.
    #[cfg(feature = "wasm")]
    WasmModule(Box<WasmModuleData>),
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
            Object::Function(d) => write!(f, "Function(params:{:?})", d.params),
            Object::AsyncFunction(d) => write!(f, "AsyncFunction(params:{:?})", d.params),
            Object::WasmImportedFunction(d) => {
                write!(f, "WasmImportedFunction({}::{})", d.module_name, d.func_name)
            }
            Object::Builtin(d) => write!(f, "Builtin(\"{}\")", d.name),
            Object::BuiltinStd(d) => write!(f, "BuiltinStd(\"{}\")", d.name),
            Object::BuiltinStdAsync(d) => write!(f, "BuiltinStdAsync(\"{}\")", d.name),
            Object::Struct(s) => write!(
                f,
                "Struct(name:{}, fields:{:?}, methods:{:?})",
                s.name, s.fields, s.methods
            ),
            Object::Enum(e) => write!(
                f,
                "Enum(enum:{}, variant:{}, payload:{:?})",
                e.enum_name, e.variant_name, e.payload
            ),
            Object::Module(m) => write!(
                f,
                "Module(name:{}, exports:{:?})",
                m.name,
                m.exports.keys().collect::<Vec<_>>()
            ),
            Object::Null => write!(f, "Null"),
            Object::ReturnValue(o) => write!(f, "ReturnValue({:?})", o),
            Object::Error(e) => write!(f, "Error({:?})", e),
            Object::Method(d) => write!(f, "Method(params:{:?})", d.params),
            Object::Break => write!(f, "Break"),
            Object::Continue => write!(f, "Continue"),
            Object::ThrownValue(o) => write!(f, "ThrownValue({:?})", o),
            Object::Future(_) => write!(f, "Future(_)"),
            #[cfg(feature = "wasm")]
            Object::WasmModule(d) => write!(
                f,
                "WasmModule(name:{}, exports:{:?})",
                d.name,
                d.exports.keys().collect::<Vec<_>>()
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
            (Object::Builtin(a), Object::Builtin(b)) => {
                a.name == b.name && a.min_params == b.min_params && a.max_params == b.max_params
            }
            (Object::BuiltinStd(a), Object::BuiltinStd(b)) => {
                a.name == b.name && a.min_params == b.min_params && a.max_params == b.max_params
            }
            (Object::BuiltinStdAsync(a), Object::BuiltinStdAsync(b)) => {
                a.name == b.name && a.min_params == b.min_params && a.max_params == b.max_params
            }
            (Object::Function(a), Object::Function(b)) => {
                a.params == b.params && Arc::ptr_eq(&a.chunk, &b.chunk) && a.local_names == b.local_names
            }
            (Object::AsyncFunction(a), Object::AsyncFunction(b)) => {
                a.params == b.params && Arc::ptr_eq(&a.chunk, &b.chunk) && a.local_names == b.local_names
            }
            (Object::WasmImportedFunction(a), Object::WasmImportedFunction(b)) => {
                a.module_name == b.module_name && a.func_name == b.func_name
            }
            (Object::Break, Object::Break) => true,
            (Object::Continue, Object::Continue) => true,
            (Object::Future(_), Object::Future(_)) => false,
            (Object::Module(a), Object::Module(b)) => {
                a.name == b.name && a.exports.keys().collect::<Vec<_>>() == b.exports.keys().collect::<Vec<_>>()
            }
            (Object::Struct(a), Object::Struct(b)) => {
                a.name == b.name && a.fields == b.fields && a.methods == b.methods
            }
            (Object::Enum(a), Object::Enum(b)) => a == b,
            #[cfg(feature = "wasm")]
            (Object::WasmModule(a), Object::WasmModule(b)) => {
                a.name == b.name && a.exports.keys().collect::<Vec<_>>() == b.exports.keys().collect::<Vec<_>>()
            }
            _ => false,
        }
    }
}

impl Object {
    pub fn type_name(&self) -> String {
        match self {
            Object::Integer(_) => "integer".to_string(),
            Object::BigInteger(_) => "bigInteger".to_string(),
            Object::Float(_) => "float".to_string(),
            Object::Boolean(_) => "boolean".to_string(),
            Object::String(_) => "string".to_string(),
            Object::Array(_) => "array".to_string(),
            Object::Hash(_) => "hash".to_string(),
            Object::Function(_) => "function".to_string(),
            Object::AsyncFunction(_) => "async function".to_string(),
            Object::WasmImportedFunction(_) => "wasm imported function".to_string(),
            Object::Builtin(_) => "builtin function".to_string(),
            Object::BuiltinStd(_) => "builtin function".to_string(),
            Object::BuiltinStdAsync(_) => "async builtin function".to_string(),
            Object::Null => "null".to_string(),
            Object::ReturnValue(_) => "return value".to_string(),
            Object::Error(_) => "error".to_string(),
            Object::Method(_) => "method".to_string(),
            Object::Struct(s) => format!("struct {}", s.name),
            Object::Enum(e) => format!("enum {}", e.enum_name),
            Object::Module(m) => format!("module {}", m.name),
            Object::Break => "break".to_string(),
            Object::Continue => "continue".to_string(),
            Object::ThrownValue(_) => "thrown value".to_string(),
            Object::Future(_) => "future".to_string(),
            #[cfg(feature = "wasm")]
            Object::WasmModule(d) => format!("wasm module {}", d.name),
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
            Object::Function(_) => write!(f, "[function]"),
            Object::AsyncFunction(_) => write!(f, "[async function]"),
            Object::WasmImportedFunction(ref d) => write!(f, "[wasm function: {}::{}]", d.module_name, d.func_name),
            Object::Builtin(ref d) => write!(f, "[built-in function: {}]", d.name),
            Object::BuiltinStd(ref d) => write!(f, "[built-in function: {}]", d.name),
            Object::BuiltinStdAsync(ref d) => {
                write!(f, "[async built-in function: {}]", d.name)
            }
            Object::Null => write!(f, "null"),
            Object::ReturnValue(ref o) => write!(f, "{}", *o),
            Object::Error(ref e) => write!(f, "{}", e),
            Object::Method(_) => write!(f, "[method]"),
            Object::Struct(ref s) => {
                write!(f, "{} {{ ", s.name)?;
                let mut sorted_fields: Vec<_> = s.fields.iter().collect();
                sorted_fields.sort_by_key(|&(name, _)| name);
                for (i, (field_name, field_value)) in sorted_fields.iter().enumerate() {
                    write!(f, "{}: {}", field_name, field_value)?;
                    if i < s.fields.len() - 1 {
                        write!(f, ", ")?;
                    }
                }
                write!(f, " }}")
            }
            Object::Enum(ref e) => {
                match &e.payload {
                    EnumPayload::Unit => write!(f, "{}::{}", e.enum_name, e.variant_name),
                    EnumPayload::Tuple(args) => {
                        write!(f, "{}::{}(", e.enum_name, e.variant_name)?;
                        for (i, arg) in args.iter().enumerate() {
                            write!(f, "{}", arg)?;
                            if i < args.len() - 1 {
                                write!(f, ", ")?;
                            }
                        }
                        write!(f, ")")
                    }
                    EnumPayload::Struct(fields) => {
                        write!(f, "{}::{} {{ ", e.enum_name, e.variant_name)?;
                        let mut sorted_fields: Vec<_> = fields.iter().collect();
                        sorted_fields.sort_by_key(|&(name, _)| name);
                        for (i, (field_name, field_value)) in sorted_fields.iter().enumerate() {
                            write!(f, "{}: {}", field_name, field_value)?;
                            if i < fields.len() - 1 {
                                write!(f, ", ")?;
                            }
                        }
                        write!(f, " }}")
                    }
                }
            }
            Object::Break => write!(f, "break"),
            Object::Continue => write!(f, "continue"),
            Object::ThrownValue(ref o) => write!(f, "Thrown: {}", *o),
            Object::Future(_) => write!(f, "[future]"),
            Object::Module(ref m) => write!(f, "[module: {}]", m.name),
            #[cfg(feature = "wasm")]
            Object::WasmModule(ref d) => write!(f, "[wasm module: {}]", d.name),
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
            Object::Function(ref d) => {
                d.params.hash(state);
                d.chunk.code.hash(state);
                d.local_names.hash(state);
            }
            Object::AsyncFunction(ref d) => {
                d.params.hash(state);
                d.chunk.code.hash(state);
                d.local_names.hash(state);
            }
            Object::Method(ref d) => {
                d.params.hash(state);
                d.chunk.code.hash(state);
                d.local_names.hash(state);
            }
            _ => "".hash(state),
        }
    }
}
