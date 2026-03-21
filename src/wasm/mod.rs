pub mod type_conversions;
pub mod wasm_runtime;

pub use type_conversions::*;
pub use wasm_runtime::*;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::errors::RuntimeError;
    use crate::interpreter::obj::Object;
    use type_conversions::{component_val_to_g, g_to_component_val, TypeMapping, WasmType};
    use wasmtime::component::Val;

    #[test]
    fn test_type_mapping_creation() {
        let mapping = TypeMapping::new();

        assert_eq!(mapping.get_wasm_type("Int"), Some(WasmType::I32));
        assert_eq!(mapping.get_wasm_type("Float"), Some(WasmType::F64));
        assert_eq!(mapping.get_wasm_type("Bool"), Some(WasmType::I32));

        assert_eq!(mapping.get_g_type(WasmType::I32), Some("Int".to_string()));
        assert_eq!(mapping.get_g_type(WasmType::F64), Some("Float".to_string()));
    }

    #[test]
    fn test_g_int_to_component() {
        let obj = Object::Integer(42);
        let val = g_to_component_val(&obj);

        assert!(val.is_ok());
        match val.unwrap() {
            Val::S32(n) => assert_eq!(n, 42),
            _ => panic!("Expected S32"),
        }
    }

    #[test]
    fn test_g_float_to_component() {
        let obj = Object::Float(3.14);
        let val = g_to_component_val(&obj);

        assert!(val.is_ok());
        match val.unwrap() {
            Val::Float64(n) => assert!((n - 3.14).abs() < 0.001),
            _ => panic!("Expected Float64"),
        }
    }

    #[test]
    fn test_g_bool_to_component() {
        let obj_true = Object::Boolean(true);
        let obj_false = Object::Boolean(false);

        let val_true = g_to_component_val(&obj_true).unwrap();
        let val_false = g_to_component_val(&obj_false).unwrap();

        match (val_true, val_false) {
            (Val::S32(t), Val::S32(f)) => {
                assert_eq!(t, 1);
                assert_eq!(f, 0);
            }
            _ => panic!("Expected S32"),
        }
    }

    #[test]
    fn test_component_val_to_g_i32() {
        let val = Val::S32(42);
        let obj = component_val_to_g(&val);

        assert!(obj.is_ok());
        assert_eq!(obj.unwrap(), Object::Integer(42));
    }

    #[test]
    fn test_component_val_to_g_f64() {
        let val = Val::Float64(3.14);
        let obj = component_val_to_g(&val);

        assert!(obj.is_ok());
        assert_eq!(obj.unwrap(), Object::Float(3.14));
    }

    #[test]
    fn test_component_val_to_g_bool() {
        let val = Val::Bool(true);
        let obj = component_val_to_g(&val);

        assert!(obj.is_ok());
        assert_eq!(obj.unwrap(), Object::Boolean(true));
    }

    #[test]
    fn test_wasm_type_from_str() {
        assert_eq!(WasmType::from_str("i32"), Some(WasmType::I32));
        assert_eq!(WasmType::from_str("i64"), Some(WasmType::I64));
        assert_eq!(WasmType::from_str("f32"), Some(WasmType::F32));
        assert_eq!(WasmType::from_str("f64"), Some(WasmType::F64));
        assert_eq!(WasmType::from_str("unknown"), None);
    }

    #[test]
    fn test_wasm_runtime_creation() {
        let runtime = WasmRuntime::new();
        assert!(runtime.is_ok());
    }

    #[test]
    fn test_wasm_store_creation() {
        let runtime = WasmRuntime::new().unwrap();
        let _store = runtime.create_store();
    }

    #[test]
    fn test_classic_module_from_wat() {
        let wat = r#"
            (module
                (func $add (param $a i32) (param $b i32) (result i32)
                    local.get $a
                    local.get $b
                    i32.add)
                (export "add" (func $add))
            )
        "#;

        let runtime = WasmRuntime::new().unwrap();
        let module = WasmModule::load_from_bytes(runtime.engine(), "test_add", wat.as_bytes());

        assert!(module.is_ok());

        let mut store = runtime.create_store();
        let instance = module.unwrap().instantiate(&mut store);
        assert!(instance.is_ok());
    }

    #[test]
    fn test_classic_function_call() {
        let wat = r#"
            (module
                (func $add (param $a i32) (param $b i32) (result i32)
                    local.get $a
                    local.get $b
                    i32.add)
                (export "add" (func $add))
                
                (func $get_answer (result i32)
                    i32.const 42)
                (export "get_answer" (func $get_answer))
            )
        "#;

        let runtime = WasmRuntime::new().unwrap();
        let module =
            WasmModule::load_from_bytes(runtime.engine(), "test_add", wat.as_bytes()).unwrap();

        let mut store = runtime.create_store();
        let instance = module.instantiate(&mut store).unwrap();

        let result = instance
            .call_func_with_args(&mut store, "add", &[Val::S32(5), Val::S32(3)])
            .unwrap();

        assert_eq!(result.len(), 1);
        match result[0] {
            Val::S32(n) => assert_eq!(n, 8),
            _ => panic!("Expected S32"),
        }

        let answer = instance
            .call_func_with_args(&mut store, "get_answer", &[])
            .unwrap();

        assert_eq!(answer.len(), 1);
        match answer[0] {
            Val::S32(n) => assert_eq!(n, 42),
            _ => panic!("Expected S32"),
        }
    }

    #[test]
    fn test_classic_memory() {
        let wat = r#"
            (module
                (memory 1)
                (data (i32.const 0) "Hello, World!")
                (export "memory" (memory 0))
            )
        "#;

        let runtime = WasmRuntime::new().unwrap();
        let module =
            WasmModule::load_from_bytes(runtime.engine(), "test_memory", wat.as_bytes()).unwrap();

        let mut store = runtime.create_store();
        let instance = module.instantiate(&mut store).unwrap();

        let memory = instance.get_memory();
        assert!(memory.is_some());

        let mem = memory.unwrap();
        let mut data = vec![0u8; 13];
        mem.read(&mut store, 0, &mut data).unwrap();
        assert_eq!(std::str::from_utf8(&data).unwrap(), "Hello, World!");
    }

    #[test]
    fn test_classic_call_with_g_objects() {
        let wat = r#"
            (module
                (func $add (param $a i32) (param $b i32) (result i32)
                    local.get $a
                    local.get $b
                    i32.add)
                (export "add" (func $add))
                
                (func $multiply (param $a i32) (param $b i32) (result i32)
                    local.get $a
                    local.get $b
                    i32.mul)
                (export "multiply" (func $multiply))
            )
        "#;

        let runtime = WasmRuntime::new().unwrap();
        let module =
            WasmModule::load_from_bytes(runtime.engine(), "test_add", wat.as_bytes()).unwrap();

        let mut store = runtime.create_store();
        let instance = module.instantiate(&mut store).unwrap();

        let result = {
            let args = vec![Object::Integer(5), Object::Integer(3)];
            let wasm_args: Result<Vec<Val>, RuntimeError> =
                args.iter().map(|arg| g_to_component_val(arg)).collect();
            let wasm_args = wasm_args.unwrap();
            instance
                .call_func_with_args(&mut store, "add", &wasm_args)
                .unwrap()
        };

        assert_eq!(result.len(), 1);
        match result[0] {
            Val::S32(n) => assert_eq!(n, 8),
            _ => panic!("Expected S32"),
        }

        let mul_result = {
            let args = vec![Object::Integer(4), Object::Integer(7)];
            let wasm_args: Result<Vec<Val>, RuntimeError> =
                args.iter().map(|arg| g_to_component_val(arg)).collect();
            let wasm_args = wasm_args.unwrap();
            instance
                .call_func_with_args(&mut store, "multiply", &wasm_args)
                .unwrap()
        };

        assert_eq!(mul_result.len(), 1);
        match mul_result[0] {
            Val::S32(n) => assert_eq!(n, 28),
            _ => panic!("Expected S32"),
        }
    }

    #[test]
    fn test_classic_call_with_float() {
        let wat = r#"
            (module
                (func $add_float (param $a f64) (param $b f64) (result f64)
                    local.get $a
                    local.get $b
                    f64.add)
                (export "add_float" (func $add_float))
            )
        "#;

        let runtime = WasmRuntime::new().unwrap();
        let module =
            WasmModule::load_from_bytes(runtime.engine(), "float_test", wat.as_bytes()).unwrap();

        let mut store = runtime.create_store();
        let instance = module.instantiate(&mut store).unwrap();

        let result = {
            let args = vec![Object::Float(1.5), Object::Float(2.5)];
            let wasm_args: Result<Vec<Val>, RuntimeError> =
                args.iter().map(|arg| g_to_component_val(arg)).collect();
            let wasm_args = wasm_args.unwrap();
            instance
                .call_func_with_args(&mut store, "add_float", &wasm_args)
                .unwrap()
        };

        assert_eq!(result.len(), 1);
        match result[0] {
            Val::Float64(n) => assert!((n - 4.0).abs() < 0.001),
            _ => panic!("Expected Float64"),
        }
    }
}
