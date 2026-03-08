pub mod type_conversions;
pub mod wasm_runtime;

pub use type_conversions::*;
pub use wasm_runtime::*;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::errors::RuntimeError;
    use crate::interpreter::obj::Object;
    use type_conversions::{giulio_to_wasm_val, wasm_val_to_giulio, TypeMapping, WasmType};
    use wasmtime::{Store, Val};

    #[test]
    fn test_type_mapping_creation() {
        let mapping = TypeMapping::new();

        assert_eq!(mapping.get_wasm_type("Int"), Some(WasmType::I32));
        assert_eq!(mapping.get_wasm_type("Float"), Some(WasmType::F64));
        assert_eq!(mapping.get_wasm_type("Bool"), Some(WasmType::I32));

        assert_eq!(
            mapping.get_giulio_type(WasmType::I32),
            Some("Int".to_string())
        );
        assert_eq!(
            mapping.get_giulio_type(WasmType::F64),
            Some("Float".to_string())
        );
    }

    #[test]
    fn test_giulio_int_to_wasm() {
        let obj = Object::Integer(42);
        let mut store: Store<()> = Store::default();
        let val = giulio_to_wasm_val(&obj, None, &mut store);

        assert!(val.is_ok());
        assert_eq!(val.unwrap().i32(), Some(42));
    }

    #[test]
    fn test_giulio_float_to_wasm() {
        let obj = Object::Float(3.14);
        let mut store: Store<()> = Store::default();
        let val = giulio_to_wasm_val(&obj, None, &mut store);

        assert!(val.is_ok());
        let bits = val.unwrap().f64();
        assert!(bits.is_some());
    }

    #[test]
    fn test_giulio_bool_to_wasm() {
        let obj_true = Object::Boolean(true);
        let obj_false = Object::Boolean(false);
        let mut store: Store<()> = Store::default();

        let val_true = giulio_to_wasm_val(&obj_true, None, &mut store).unwrap();
        let val_false = giulio_to_wasm_val(&obj_false, None, &mut store).unwrap();

        assert_eq!(val_true.i32(), Some(1));
        assert_eq!(val_false.i32(), Some(0));
    }

    #[test]
    fn test_wasm_val_to_giulio_i32() {
        let val = Val::I32(42);
        let obj = wasm_val_to_giulio(&val);

        assert!(obj.is_ok());
        assert_eq!(obj.unwrap(), Object::Integer(42));
    }

    #[test]
    fn test_wasm_val_to_giulio_f64() {
        let val = Val::F64(3.14_f64.to_bits());
        let obj = wasm_val_to_giulio(&val);

        assert!(obj.is_ok());
        assert_eq!(obj.unwrap(), Object::Float(3.14));
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
    fn test_wasm_module_from_wat() {
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
    fn test_wasm_function_call() {
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
            .call_func_with_args(&mut store, "add", &[Val::I32(5), Val::I32(3)])
            .unwrap();

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].i32(), Some(8));

        let answer = instance
            .call_func_with_args(&mut store, "get_answer", &[])
            .unwrap();

        assert_eq!(answer.len(), 1);
        assert_eq!(answer[0].i32(), Some(42));
    }

    #[test]
    fn test_wasm_memory() {
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
    fn test_wasm_string_return() {
        let wat = r#"
            (module
                (memory 1)
                (func $get_greeting (result i32)
                    i32.const 0)
                (data (i32.const 0) "Hello from WASM!")
                (export "get_greeting" (func $get_greeting))
                (export "memory" (memory 0))
            )
        "#;

        let runtime = WasmRuntime::new().unwrap();
        let module =
            WasmModule::load_from_bytes(runtime.engine(), "test_string", wat.as_bytes()).unwrap();

        let mut store = runtime.create_store();
        let instance = module.instantiate(&mut store).unwrap();

        let ptr = instance
            .call_func_with_args(&mut store, "get_greeting", &[])
            .unwrap()[0]
            .i32()
            .unwrap();

        let memory = instance.get_memory().unwrap();
        let mut data = vec![0u8; 20];
        memory.read(&mut store, ptr as usize, &mut data).unwrap();
        let data_string = std::str::from_utf8(&data).unwrap();
        let trimmed = data_string.trim_end_matches('\0');
        assert_eq!(trimmed, "Hello from WASM!");
    }

    #[test]
    fn test_wasm_call_with_giulio_objects() {
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
            let memory = instance.get_memory();
            let args = vec![Object::Integer(5), Object::Integer(3)];
            let wasm_args: Result<Vec<Val>, RuntimeError> = args
                .iter()
                .map(|arg| giulio_to_wasm_val(arg, memory, &mut store))
                .collect();
            let wasm_args = wasm_args.unwrap();
            instance
                .call_func_with_args(&mut store, "add", &wasm_args)
                .unwrap()
        };

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].i32(), Some(8));

        let mul_result = {
            let memory = instance.get_memory();
            let args = vec![Object::Integer(4), Object::Integer(7)];
            let wasm_args: Result<Vec<Val>, RuntimeError> = args
                .iter()
                .map(|arg| giulio_to_wasm_val(arg, memory, &mut store))
                .collect();
            let wasm_args = wasm_args.unwrap();
            instance
                .call_func_with_args(&mut store, "multiply", &wasm_args)
                .unwrap()
        };

        assert_eq!(mul_result.len(), 1);
        assert_eq!(mul_result[0].i32(), Some(28));
    }

    #[test]
    fn test_call_with_float() {
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
            let memory = instance.get_memory();
            let args = vec![Object::Float(1.5), Object::Float(2.5)];
            let wasm_args: Result<Vec<Val>, RuntimeError> = args
                .iter()
                .map(|arg| giulio_to_wasm_val(arg, memory, &mut store))
                .collect();
            let wasm_args = wasm_args.unwrap();
            instance
                .call_func_with_args(&mut store, "add_float", &wasm_args)
                .unwrap()
        };

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].f64(), Some(4.0));
    }

    #[test]
    fn test_call_with_boolean() {
        let wat = r#"
            (module
                (func $test_bool (param $a i32) (result i32)
                    local.get $a
                    i32.const 1
                    i32.add)
                (export "test_bool" (func $test_bool))
            )
        "#;

        let runtime = WasmRuntime::new().unwrap();
        let module =
            WasmModule::load_from_bytes(runtime.engine(), "bool_test", wat.as_bytes()).unwrap();

        let mut store = runtime.create_store();
        let instance = module.instantiate(&mut store).unwrap();

        let result = {
            let memory = instance.get_memory();
            let args = vec![Object::Boolean(true)];
            let wasm_args: Result<Vec<Val>, RuntimeError> = args
                .iter()
                .map(|arg| giulio_to_wasm_val(arg, memory, &mut store))
                .collect();
            let wasm_args = wasm_args.unwrap();
            instance
                .call_func_with_args(&mut store, "test_bool", &wasm_args)
                .unwrap()
        };

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].i32(), Some(2));
    }
}
