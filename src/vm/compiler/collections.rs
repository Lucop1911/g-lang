//! Collection compilation: arrays, hashes, indexing, struct literals, field access,
//! method calls, and struct declarations.
use crate::ast::ast::{Expr, Ident, Literal, EnumVariant};
use crate::vm::obj::{EnumObject, EnumPayload, FunctionData, Object, StructObject};
use crate::vm::compiler::Compiler;
use crate::vm::instruction::Instruction;

/// Compiles an array literal: `[e1, e2, e3]`.
pub fn compile_array_expr(compiler: &mut Compiler, elements: &[Expr], line: u16) {
    for element in elements {
        compiler.compile_expression(element, line);
    }
    compiler.emit(Instruction::BuildArray(elements.len() as u16), line);
}

/// Compiles a hash literal: `{k1: v1, k2: v2}`.
pub fn compile_hash_expr(compiler: &mut Compiler, pairs: &[(Expr, Expr)], line: u16) {
    for (key, value) in pairs {
        compiler.compile_expression(key, line);
        compiler.compile_expression(value, line);
    }
    compiler.emit(Instruction::BuildHash(pairs.len() as u16), line);
}

/// Compiles an index expression: `arr[i]` or `hash[key]`.
pub fn compile_index_expr(compiler: &mut Compiler, array: &Expr, index: &Expr, line: u16) {
    compiler.compile_expression(array, line);
    compiler.compile_expression(index, line);
    compiler.emit(Instruction::Index, line);
}

/// Compiles a method call: `obj.method(args...)`.
pub fn compile_method_call(
    compiler: &mut Compiler,
    object: &Expr,
    method: &str,
    arguments: &[Expr],
    line: u16,
) {
    compiler.compile_expression(object, line);

    let method_idx = compiler
        .chunk
        .add_constant(Object::String(method.to_string()));
    if let Some(method_idx) = method_idx {
        compiler.emit(Instruction::Constant(method_idx), line);
    }

    for arg in arguments {
        compiler.compile_expression(arg, line);
    }

    compiler.emit(Instruction::CallMethod(arguments.len() as u8), line);
}

/// Compiles a struct literal: `Name { field1: e1, field2: e2 }`.
pub fn compile_struct_literal(
    compiler: &mut Compiler,
    name: &Ident,
    fields: &[(Ident, Expr)],
    line: u16,
) {
    for (ident, expr) in fields {
        let field_name_idx = compiler
            .chunk
            .add_constant(Object::String(ident.name.clone()));
        if let Some(idx) = field_name_idx {
            compiler.emit(Instruction::Constant(idx), line);
        }
        compiler.compile_expression(expr, line);
    }

    let template_idx = if let Some(template) = compiler.struct_templates.get(&name.name) {
        compiler.chunk.add_constant(template.clone())
    } else {
        None
    };

    if let Some(idx) = template_idx {
        compiler.emit(Instruction::Constant(idx), line);
    }

    compiler.emit(Instruction::BuildStruct(fields.len() as u8), line);
}

/// Compiles a field access: `obj.field`.
pub fn compile_field_access(compiler: &mut Compiler, object: &Expr, field: &str, line: u16) {
    compiler.compile_expression(object, line);
    let field_idx = compiler
        .chunk
        .add_constant(Object::String(field.to_string()));
    if let Some(field_idx) = field_idx {
        compiler.emit(Instruction::Constant(field_idx), line);
        compiler.emit(Instruction::GetField, line);
    }
}

/// Compiles a struct declaration: `struct Name { fields..., methods... }`.
pub fn compile_struct_stmt(
    compiler: &mut Compiler,
    name: &Ident,
    fields: &[(Ident, Expr)],
    methods: &[(Ident, Expr)],
    line: u16,
) {
    use ahash::AHasher;
    use std::hash::BuildHasherDefault;
    type HashMap<K, V> = std::collections::HashMap<K, V, BuildHasherDefault<AHasher>>;

    let mut field_map: HashMap<String, Object> = HashMap::default();
    let mut method_map: HashMap<String, Object> = HashMap::default();

    for (ident, expr) in fields {
        // Store the default value - will be used when creating instances
        let value = match expr {
            Expr::LitExpr(lit) => match lit {
                Literal::IntLiteral(i) => Object::Integer(*i),
                Literal::BigIntLiteral(b) => {
                    Object::BigInteger(Box::new(b.clone()))
                }
                Literal::FloatLiteral(f) => Object::Float(*f),
                Literal::BoolLiteral(b) => Object::Boolean(*b),
                Literal::StringLiteral(s) => Object::String(s.clone()),
                Literal::NullLiteral => Object::Null,
            },
            _ => Object::Null,
        };
        field_map.insert(ident.name.clone(), value);
    }

    for (ident, expr) in methods {
        if let Expr::FnExpr { params, body } = expr {
            // Prepend 'this' parameter to the method signature.
            let mut new_params = vec![Ident {
                name: "this".to_string(),
                slot: crate::ast::ast::SlotIndex(0),
            }];
            new_params.extend(params.clone());

            let (fn_chunk, _param_count, local_names) =
                crate::vm::compiler::Compiler::compile_function_body(&new_params, body, false);

            let fn_obj = Object::Function(Box::new(FunctionData {
                params: new_params,
                chunk: std::sync::Arc::new(fn_chunk),
                env: std::sync::Arc::new(std::sync::Mutex::new(
                    crate::vm::runtime::env::Environment::new(),
                )),
                local_names,
            }));

            method_map.insert(ident.name.clone(), fn_obj);
        }
    }

    let struct_obj = Object::Struct(Box::new(StructObject {
        name: name.name.clone(),
        fields: field_map,
        methods: method_map,
    }));

    let struct_idx = compiler.chunk.add_constant(struct_obj.clone());
    if let Some(struct_idx) = struct_idx {
        compiler.emit(Instruction::Constant(struct_idx), line);
    }

    compiler.struct_templates.insert(name.name.clone(), struct_obj);

    let name_idx = compiler
        .chunk
        .add_constant(Object::String(name.name.clone()));
    if let Some(name_idx) = name_idx {
        compiler.emit(Instruction::SetGlobal(name_idx), line);
    }
}

/// Compiles an enum struct variant literal.
pub fn compile_enum_struct_literal(
    compiler: &mut Compiler,
    enum_name: &Ident,
    variant_name: &Ident,
    fields: &[(Ident, Expr)],
    line: u16,
) {
    for (ident, expr) in fields {
        let field_name_idx = compiler
            .chunk
            .add_constant(Object::String(ident.name.clone()));
        if let Some(idx) = field_name_idx {
            compiler.emit(Instruction::Constant(idx), line);
        }
        compiler.compile_expression(expr, line);
    }

    let enum_name_idx = compiler
        .chunk
        .add_constant(Object::String(enum_name.name.clone()));
    if let Some(idx) = enum_name_idx {
        compiler.emit(Instruction::Constant(idx), line);
    }

    let variant_name_idx = compiler
        .chunk
        .add_constant(Object::String(variant_name.name.clone()));
    if let Some(idx) = variant_name_idx {
        compiler.emit(Instruction::Constant(idx), line);
    }

    compiler.emit(Instruction::BuildEnumStruct(fields.len() as u8), line);
}

/// Compiles an enum statement
pub fn compile_enum_stmt(
    compiler: &mut Compiler,
    name: &Ident,
    variants: &[EnumVariant],
    line: u16,
) {
    use ahash::AHasher;
    use std::hash::BuildHasherDefault;
    type HashMap<K, V> = std::collections::HashMap<K, V, BuildHasherDefault<AHasher>>;

    let mut field_map: HashMap<String, Object> = HashMap::default();
    let mut method_map: HashMap<String, Object> = HashMap::default();

    for variant in variants {
        match variant {
            EnumVariant::Unit(v_name) => {
                let enum_obj = Object::Enum(Box::new(EnumObject {
                    enum_name: name.name.clone(),
                    variant_name: v_name.name.clone(),
                    payload: EnumPayload::Unit,
                }));
                field_map.insert(v_name.name.clone(), enum_obj);
            }
            EnumVariant::Tuple(v_name, params) => {
                let mut new_params = vec![Ident::new("this".to_string())];
                new_params.extend(params.clone());
                for (i, p) in new_params.iter_mut().enumerate() {
                    p.slot = crate::ast::ast::SlotIndex(i as u16);
                }

                let mut chunk = crate::vm::chunk::Chunk::new();
                for i in 1..=params.len() {
                    chunk.write_instruction(Instruction::GetLocal(i as u8), 0);
                }
                let enum_name_idx = chunk.add_constant(Object::String(name.name.clone())).unwrap();
                chunk.write_instruction(Instruction::Constant(enum_name_idx), 0);
                let variant_name_idx = chunk.add_constant(Object::String(v_name.name.clone())).unwrap();
                chunk.write_instruction(Instruction::Constant(variant_name_idx), 0);
                chunk.write_instruction(Instruction::BuildEnumTuple(params.len() as u8), 0);
                chunk.write_instruction(Instruction::ReturnValue, 0);

                let fn_obj = Object::Function(Box::new(FunctionData {
                    local_names: new_params.iter().map(|p| p.name.clone()).collect(),
                    params: new_params,
                    chunk: std::sync::Arc::new(chunk),
                    env: std::sync::Arc::new(std::sync::Mutex::new(
                        crate::vm::runtime::env::Environment::new(),
                    )),
                }));
                method_map.insert(v_name.name.clone(), fn_obj);
            }
            EnumVariant::Struct(v_name, fields) => {
                let mut new_params = vec![Ident::new("this".to_string())];
                new_params.extend(fields.clone());
                for (i, p) in new_params.iter_mut().enumerate() {
                    p.slot = crate::ast::ast::SlotIndex(i as u16);
                }

                let mut chunk = crate::vm::chunk::Chunk::new();
                for (i, f) in fields.iter().enumerate() {
                    let field_name_idx = chunk.add_constant(Object::String(f.name.clone())).unwrap();
                    chunk.write_instruction(Instruction::Constant(field_name_idx), 0);
                    chunk.write_instruction(Instruction::GetLocal((i + 1) as u8), 0);
                }
                let enum_name_idx = chunk.add_constant(Object::String(name.name.clone())).unwrap();
                chunk.write_instruction(Instruction::Constant(enum_name_idx), 0);
                let variant_name_idx = chunk.add_constant(Object::String(v_name.name.clone())).unwrap();
                chunk.write_instruction(Instruction::Constant(variant_name_idx), 0);
                chunk.write_instruction(Instruction::BuildEnumStruct(fields.len() as u8), 0);
                chunk.write_instruction(Instruction::ReturnValue, 0);

                let fn_obj = Object::Function(Box::new(FunctionData {
                    local_names: new_params.iter().map(|p| p.name.clone()).collect(),
                    params: new_params,
                    chunk: std::sync::Arc::new(chunk),
                    env: std::sync::Arc::new(std::sync::Mutex::new(
                        crate::vm::runtime::env::Environment::new(),
                    )),
                }));
                method_map.insert(v_name.name.clone(), fn_obj);
            }
        }
    }

    let struct_obj = Object::Struct(Box::new(StructObject {
        name: name.name.clone(),
        fields: field_map,
        methods: method_map,
    }));

    let struct_idx = compiler.chunk.add_constant(struct_obj.clone());
    if let Some(struct_idx) = struct_idx {
        compiler.emit(Instruction::Constant(struct_idx), line);
    }

    compiler.struct_templates.insert(name.name.clone(), struct_obj);

    let name_idx = compiler
        .chunk
        .add_constant(Object::String(name.name.clone()));
    if let Some(name_idx) = name_idx {
        compiler.emit(Instruction::SetGlobal(name_idx), line);
    }
}
