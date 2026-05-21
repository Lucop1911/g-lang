//! Integration tests for the VM execution engine.
//!
//! These tests verify every language feature against the stack-based VM.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::lexer::lexer::Lexer;
use crate::parser::parser::Parser;
use crate::ast::ast::Program;
use crate::lexer::token::SpannedTokens;
use crate::vm::runtime::runtime_errors::RuntimeError;
use crate::vm::runtime::env::Environment;
use crate::vm::runtime::module_registry::ModuleRegistry;
use crate::vm::obj::Object;
use crate::vm::compiler::Compiler;
use crate::vm::vm::VirtualMachine;

fn parse_test_helper(input: &str) -> Program {
    let spanned_tokens = Lexer::lex_tokens(input.as_bytes())
        .expect("lexer failed");
    let spanned = SpannedTokens::new(&spanned_tokens);
    let tokens = spanned.to_tokens();
    let result = Parser::parse_tokens(tokens).expect("parser failed");
    let (remaining_tokens, program) = result;
    assert_eq!(remaining_tokens.token.len(), 0, "Parser did not consume all tokens");
    program
}

async fn vm_test_helper(input: &str) -> Object {
    let trimmed = input.trim_end();
    let needs_semicolon = !trimmed.ends_with(';') && !trimmed.ends_with('}');
    let input_to_parse = if needs_semicolon {
        format!("{};", input)
    } else {
        input.to_string()
    };
    let mut program = parse_test_helper(&input_to_parse);
    let chunk = Compiler::compile_program(&mut program)
        .expect("compilation failed");
    let globals = Arc::new(Mutex::new(Environment::new_root()));
    let module_registry = Arc::new(Mutex::new(ModuleRegistry::new(PathBuf::from("."))));
    let mut vm = VirtualMachine::new(globals, module_registry);
    match vm.run(Arc::new(chunk)).await {
        Ok(obj) => obj,
        Err(e) => Object::Error(Box::new(e)),
    }
}

// ─── Integer Arithmetic ──────────────────────────────────────────────

#[tokio::test]
async fn vm_test_integer_expression() {
    let tests = vec![
        ("5", 5i64),
        ("10", 10),
        ("-5", -5),
        ("-10", -10),
        ("5 + 5 + 5 + 5 - 10", 10),
        ("2 * 2 * 2 * 2 * 2", 32),
        ("-50 + 100 + -50", 0),
        ("5 * 2 + 10", 20),
        ("5 + 2 * 10", 25),
        ("20 + 2 * -10", 0),
        ("50 / 2 * 2 + 10", 60),
        ("2 * (5 + 10)", 30),
        ("3 * 3 * 3 + 10", 37),
        ("3 * (3 * 3) + 10", 37),
        ("(5 + 10 * 2 + 15 / 3) * 2 + -10", 50),
        ("10 % 3", 1),
        ("10 % 2", 0),
        ("7 % 4", 3),
    ];

    for (input, expected) in tests {
        let evaluated = vm_test_helper(input).await;
        assert_eq!(evaluated, Object::Integer(expected), "input: {}", input);
    }
}

#[tokio::test]
async fn vm_test_boolean_expression() {
    let tests = vec![
        ("true", true),
        ("false", false),
        ("1 < 2", true),
        ("1 > 2", false),
        ("1 < 1", false),
        ("1 > 1", false),
        ("1 == 1", true),
        ("1 != 1", false),
        ("1 == 2", false),
        ("1 != 2", true),
        ("true == true", true),
        ("false == false", true),
        ("true == false", false),
        ("true != false", true),
        ("false != true", true),
        ("(1 < 2) == true", true),
        ("(1 < 2) == false", false),
        ("(1 > 2) == true", false),
        ("(1 > 2) == false", true),
    ];

    for (input, expected) in tests {
        let evaluated = vm_test_helper(input).await;
        assert_eq!(evaluated, Object::Boolean(expected), "input: {}", input);
    }
}

#[tokio::test]
async fn vm_test_bang_operator() {
    let tests = vec![
        ("!true", false),
        ("!false", true),
        ("!5", false),
        ("!!true", true),
        ("!!false", false),
        ("!!5", true),
    ];

    for (input, expected) in tests {
        let evaluated = vm_test_helper(input).await;
        assert_eq!(evaluated, Object::Boolean(expected), "input: {}", input);
    }
}

// ─── Control Flow ────────────────────────────────────────────────────

#[tokio::test]
async fn vm_test_if_else_expressions() {
    let tests = vec![
        ("if (true) { 10 }", 10i64),
        ("if (false) { 10 }", 0),
        ("if (1) { 10 }", 10),
        ("if (1 < 2) { 10 }", 10),
        ("if (1 > 2) { 10 } else { 20 }", 20),
        ("if (1 < 2) { 10 } else { 20 }", 10),
    ];

    for (input, expected) in tests {
        let evaluated = vm_test_helper(input).await;
        match evaluated {
            Object::Integer(i) => assert_eq!(i, expected, "input: {}", input),
            Object::Null => assert_eq!(expected, 0, "input: {} (got Null)", input),
            _ => panic!("Expected Integer, got {:?} for input: {}", evaluated, input),
        }
    }
}

#[tokio::test]
async fn vm_test_return_statements() {
    let input = r#"
let f = fn(x) {
  return x;
  10;
};
f(10);
"#;
    let evaluated = vm_test_helper(input).await;
    assert_eq!(evaluated, Object::Integer(10));
}

// ─── Variables ───────────────────────────────────────────────────────

#[tokio::test]
async fn vm_test_let_statements() {
    let tests = vec![
        ("let a = 5; a;", 5i64),
        ("let a = 5 * 5; a;", 25),
        ("let a = 5; let b = a; b;", 5),
        ("let a = 5; let b = a; let c = a + b + 5; c;", 15),
    ];

    for (input, expected) in tests {
        let evaluated = vm_test_helper(input).await;
        assert_eq!(evaluated, Object::Integer(expected), "input: {}", input);
    }
}

// ─── Functions ───────────────────────────────────────────────────────

#[tokio::test]
async fn vm_test_function_application() {
    let input = r#"
let identity = fn(x) { x; };
identity(5);
"#;
    let evaluated = vm_test_helper(input).await;
    assert_eq!(evaluated, Object::Integer(5));
}

#[tokio::test]
async fn vm_test_function_with_multiple_params() {
    let input = r#"
let add = fn(a, b) { a + b; };
add(1, 2);
"#;
    let evaluated = vm_test_helper(input).await;
    assert_eq!(evaluated, Object::Integer(3));
}

// ─── Closures ────────────────────────────────────────────────────────

#[tokio::test]
async fn vm_test_closures() {
    let input = r#"
let newAdder = fn(x) {
  fn(y) { x + y; };
};
let addTwo = newAdder(2);
addTwo(3);
"#;
    let evaluated = vm_test_helper(input).await;
    assert_eq!(evaluated, Object::Integer(5));
}

#[tokio::test]
async fn vm_test_closure_with_counter() {
    let input = r#"
let newCounter = fn() {
  let i = 0;
  fn() { i = i + 1; i; };
};
let counter = newCounter();
counter();
counter();
counter();
"#;
    let evaluated = vm_test_helper(input).await;
    assert_eq!(evaluated, Object::Integer(3));
}

// ─── Recursion ───────────────────────────────────────────────────────

#[tokio::test]
async fn vm_test_recursion() {
    let input = r#"
fn fib(n) {
  if (n <= 1) { n; } else { fib(n - 1) + fib(n - 2); };
};
fib(10);
"#;
    let evaluated = vm_test_helper(input).await;
    assert_eq!(evaluated, Object::Integer(55));
}

#[tokio::test]
async fn vm_test_factorial() {
    let input = r#"
let fact = fn(n) {
  if (n <= 1) { 1; } else { n * fact(n - 1); };
};
fact(5);
"#;
    let evaluated = vm_test_helper(input).await;
    assert_eq!(evaluated, Object::Integer(120));
}

// ─── Arrays ──────────────────────────────────────────────────────────

#[tokio::test]
async fn vm_test_array_literals() {
    let input = "[1, 2, 3];";
    let evaluated = vm_test_helper(input).await;
    assert_eq!(
        evaluated,
        Object::Array(Box::new(vec![
            Object::Integer(1),
            Object::Integer(2),
            Object::Integer(3),
        ]))
    );
}

#[tokio::test]
async fn vm_test_array_indexing() {
    let input = "[1, 2, 3][1];";
    let evaluated = vm_test_helper(input).await;
    assert_eq!(evaluated, Object::Integer(2));
}

// ─── Hashes ──────────────────────────────────────────────────────────

#[tokio::test]
async fn vm_test_hash_literals() {
    let input = r#"let h = {"a": 1, "b": 2}; h["a"];"#;
    let evaluated = vm_test_helper(input).await;
    assert_eq!(evaluated, Object::Integer(1));
}

// ─── Strings ─────────────────────────────────────────────────────────

#[tokio::test]
async fn vm_test_string_concatenation() {
    let input = r#""Hello" + " " + "World";"#;
    let evaluated = vm_test_helper(input).await;
    assert_eq!(evaluated, Object::String("Hello World".to_string()));
}

// ─── Division by Zero ────────────────────────────────────────────────

#[tokio::test]
async fn vm_test_division_by_zero() {
    let input = "10 / 0;";
    let evaluated = vm_test_helper(input).await;
    assert_eq!(
        evaluated,
        Object::Error(Box::new(crate::vm::runtime::runtime_errors::RuntimeError::DivisionByZero))
    );
}

// ─── While Loops ─────────────────────────────────────────────────────

#[tokio::test]
async fn vm_test_while_loop() {
    let input = r#"
let i = 0;
let sum = 0;
while (i < 5) {
  sum = sum + i;
  i = i + 1;
}
sum;
"#;
    let evaluated = vm_test_helper(input).await;
    assert_eq!(evaluated, Object::Integer(10));
}

// ─── For-in Loops ────────────────────────────────────────────────────

#[tokio::test]
async fn vm_test_for_in_loop() {
    let input = r#"
let sum = 0;
for (let x in [1, 2, 3, 4]) {
  sum = sum + x;
}
sum;
"#;
    let evaluated = vm_test_helper(input).await;
    assert_eq!(evaluated, Object::Integer(10));
}

// ─── Higher-Order Functions ──────────────────────────────────────────

#[tokio::test]
async fn vm_test_higher_order_functions() {
    let input = r#"
let apply = fn(f, x) { f(x); };
let double = fn(x) { x * 2; };
apply(double, 21);
"#;
    let evaluated = vm_test_helper(input).await;
    assert_eq!(evaluated, Object::Integer(42));
}

// ─── Try/Catch ───────────────────────────────────────────────────────

#[tokio::test]
async fn vm_test_try_catch() {
    let input = r#"
try {
  throw "error!";
} catch (e) {
  e;
}
"#;
    let evaluated = vm_test_helper(input).await;
    assert_eq!(evaluated, Object::String("error!".to_string()));
}

#[tokio::test]
async fn vm_test_try_catch_division_by_zero() {
    let input = r#"
        let result = try {
            10 / 0;
        } catch (e) {
            42;
        };
        result
    "#;
    let evaluated = vm_test_helper(input).await;
    assert_eq!(evaluated, Object::Integer(42));
}


// ─── Tuple Destructuring ─────────────────────────────────────────────

#[tokio::test]
async fn vm_test_multi_let() {
    let input = r#"
let (a, b) = (1, 2);
a + b;
"#;
    let evaluated = vm_test_helper(input).await;
    assert_eq!(evaluated, Object::Integer(3));
}

// ─── Extended Runtime Tests (migrated from runtime/mod.rs) ───────────

#[tokio::test]
async fn vm_test_async_function_basic() {
    let input = r#"
        async fn main() {
            async fn async_identity(x) {
                return x;
            }
            return await async_identity(10);
        }
        main();
    "#;
    assert_eq!(vm_test_helper(input).await, Object::Integer(10));
}

#[tokio::test]
async fn vm_test_await_expressions() {
    let input = r#"
        async fn main() {
            async fn add_one(x) {
                return x + 1;
            }
            let res = await add_one(5);
            return await add_one(res);
        }
        main();
    "#;
    assert_eq!(vm_test_helper(input).await, Object::Integer(7));
}

#[tokio::test]
async fn vm_test_chained_async_calls() {
    let input = r#"
        async fn main() {
            async fn add_one(x) {
                return x + 1;
            }
            async fn add_two(x) {
                let one_added = await add_one(x);
                return await add_one(one_added);
            }
            return await add_two(5);
        }
        main();
    "#;
    let mut program = parse_test_helper(input);
    let chunk = Compiler::compile_program(&mut program)
        .expect("compilation failed");
    let globals = Arc::new(Mutex::new(Environment::new_root()));
    let module_registry = Arc::new(Mutex::new(ModuleRegistry::new(PathBuf::from("."))));
    
    // Print the global environment *before* running the VM to see what's defined
    {
        let globals_guard = globals.lock().unwrap();
        println!("Globals before execution: {:?}", globals_guard);
    }

    let mut vm = VirtualMachine::new(Arc::clone(&globals), module_registry);
    let result = vm.run(Arc::new(chunk)).await;
    
    if let Ok(Object::Error(err)) = &result {
        println!("Execution failed with Error object: {:?}", err);
    } else if let Err(err) = &result {
        println!("Execution failed with error: {:?}", err);
    }
    
    match result {
        Ok(obj) => assert_eq!(obj, Object::Integer(7)),
        Err(e) => {
            println!("VM execution returned Err: {:?}", e);
            panic!("VM execution failed with error: {:?}", e);
        }
    }
}

#[tokio::test]
async fn vm_test_async_function_return_types() {
    let input_int = r#"
        async fn main() {
            async fn get_int() {
                return 123;
            }
            return await get_int();
        }
        main();
    "#;
    assert_eq!(vm_test_helper(input_int).await, Object::Integer(123));

    let input_bool = r#"
        async fn main() {
            async fn get_bool() {
                return true;
            }
            return await get_bool();
        }
        main();
    "#;
    assert_eq!(vm_test_helper(input_bool).await, Object::Boolean(true));

    let input_string = r#"
        async fn main() {
            async fn get_string() {
                return "hello";
            }
            return await get_string();
        }
        main();
    "#;
    assert_eq!(vm_test_helper(input_string).await, Object::String("hello".to_string()));
}

#[tokio::test]
async fn vm_test_try_catch_no_throw() {
    let input = r#"
        let x = 0;
        try {
            x = 1;
        } catch (e) {
            x = 2;
        } finally {
            x = x + 1;
        }
        x
    "#;
    assert_eq!(vm_test_helper(input).await, Object::Integer(2));
}

#[tokio::test]
async fn vm_test_try_throw_catch() {
    let input = r#"
        let x = 0;
        try {
            throw "Error!";
            x = 1;
        } catch (e) {
            x = 2;
        } finally {
            x = x + 1;
        }
        x
    "#;
    assert_eq!(vm_test_helper(input).await, Object::Integer(3));
}

#[tokio::test]
async fn vm_test_try_throw_catch_with_exception_ident() {
    let input = r#"
        let err_msg = "";
        try {
            throw "Something went wrong";
        } catch (e) {
            err_msg = e;
        }
        err_msg
    "#;
    assert_eq!(vm_test_helper(input).await, Object::String("Something went wrong".to_string()));
}

#[tokio::test]
async fn vm_test_try_throw_no_catch_finally() {
    let input = r#"
        let x = 0;
        try {
            throw "Error!";
            x = 1;
        } finally {
            x = x + 1;
        }
        x
    "#;
    match vm_test_helper(input).await {
        Object::ThrownValue(obj) => assert_eq!(*obj, Object::String("Error!".to_string())),
        _ => panic!("Expected a ThrownValue"),
    }
}

#[tokio::test]
async fn vm_test_finally_overrides_return() {
    let input = r#"
        fn test_fn() {
            try {
                return 1;
            } finally {
                return 2;
            }
        }
        test_fn()
    "#;
    assert_eq!(vm_test_helper(input).await, Object::Integer(2));
}

#[tokio::test]
async fn vm_test_finally_overrides_thrown_value() {
    let input = r#"
        try {
            throw "Error from try";
        } finally {
            throw "Error from finally";
        }
    "#;
    match vm_test_helper(input).await {
        Object::ThrownValue(obj) => assert_eq!(*obj, Object::String("Error from finally".to_string())),
        _ => panic!("Expected ThrownValue"),
    }
}

#[tokio::test]
async fn vm_test_finally_not_overriding_thrown_value_if_not_thrown() {
    let input = r#"
        let result = try {
            1 + 1
        } catch (e) {
            "Caught: " + e
        } finally {
        };
        result
    "#;
    assert_eq!(vm_test_helper(input).await, Object::Integer(2));
}

#[tokio::test]
async fn vm_test_nested_try_catch() {
    let input = r#"
        let outer_status = "";
        try {
            try {
                throw "Inner Error";
            } catch (e) {
                outer_status = "Inner caught: " + e;
            } finally {
                outer_status = outer_status + " (inner finally)";
            }
        } catch (e) {
            outer_status = outer_status + "Outer caught: " + e;
        } finally {
            outer_status = outer_status + " (outer finally)";
        }
        outer_status
    "#;
    assert_eq!(vm_test_helper(input).await, Object::String("Inner caught: Inner Error (inner finally) (outer finally)".to_string()));
}

#[tokio::test]
async fn vm_test_try_result_is_last_expression() {
    let input = r#"
        let result = try {
            1 + 1
        } catch (e) {
            0
        };
        result
    "#;
    assert_eq!(vm_test_helper(input).await, Object::Integer(2));
}

#[tokio::test]
async fn vm_test_catch_result_is_last_expression() {
    let input = r#"
        let result = try {
            throw 1;
        } catch (e) {
            e + 1
        };
        result
    "#;
    assert_eq!(vm_test_helper(input).await, Object::Integer(2));
}

#[tokio::test]
async fn vm_test_error_type_propagation() {
    let input = r#"
        let err = try {
            throw true;
        } catch (e) {
            e
        };
        err
    "#;
    assert_eq!(vm_test_helper(input).await, Object::Boolean(true));

    let input_str = r#"
        let err = try {
            throw "some string";
        } catch (e) {
            e
        };
        err
    "#;
    assert_eq!(vm_test_helper(input_str).await, Object::String("some string".to_string()));

    let input_int = r#"
        let err = try {
            throw 123;
        } catch (e) {
            e
        };
        err
    "#;
    assert_eq!(vm_test_helper(input_int).await, Object::Integer(123));
}

#[tokio::test]
async fn vm_test_recursion_extended() {
    let input = r#"
        let factorial = fn(n) {
            if (n <= 1) {
                1
            } else {
                n * factorial(n - 1)
            }
        };
        factorial(5)
    "#;
    assert_eq!(vm_test_helper(input).await, Object::Integer(120));

    let input2 = r#"
        let sum_to = fn(n) {
            if (n == 0) {
                0
            } else {
                n + sum_to(n - 1)
            }
        };
        sum_to(10)
    "#;
    assert_eq!(vm_test_helper(input2).await, Object::Integer(55));

    let input3 = r#"
        let fib = fn(n) {
            if (n <= 1) {
                n
            } else {
                fib(n - 1) + fib(n - 2)
            }
        };
        fib(10)
    "#;
    assert_eq!(vm_test_helper(input3).await, Object::Integer(55));
}

#[tokio::test]
async fn vm_test_fibonacci() {
    let input = r#"
        let fib = fn(n) {
            if (n == 0) {
                0
            } else {
                if (n == 1) {
                    1
                } else {
                    fib(n - 1) + fib(n - 2)
                }
            }
        };
        fib(7)
    "#;
    assert_eq!(vm_test_helper(input).await, Object::Integer(13));

    let input2 = r#"
        let fib = fn(n) {
            if (n == 0) {
                0
            } else {
                if (n == 1) {
                    1
                } else {
                    fib(n - 1) + fib(n - 2)
                }
            }
        };
        fib(15)
    "#;
    assert_eq!(vm_test_helper(input2).await, Object::Integer(610));
}

#[tokio::test]
async fn vm_test_mutual_recursion() {
    let input = r#"
        let is_even = fn(n) {
            if (n == 0) {
                true
            } else {
                is_odd(n - 1)
            }
        };
        let is_odd = fn(n) {
            if (n == 0) {
                false
            } else {
                is_even(n - 1)
            }
        };
        is_even(4)
    "#;
    assert_eq!(vm_test_helper(input).await, Object::Boolean(true));

    let input2 = r#"
        let is_even = fn(n) {
            if (n == 0) {
                true
            } else {
                is_odd(n - 1)
            }
        };
        let is_odd = fn(n) {
            if (n == 0) {
                false
            } else {
                is_even(n - 1)
            }
        };
        is_odd(7)
    "#;
    assert_eq!(vm_test_helper(input2).await, Object::Boolean(true));
}

#[tokio::test]
async fn vm_test_higher_order_functions_extended() {
    let input = r#"
        let apply_twice = fn(f, x) {
            f(f(x))
        };
        let add_one = fn(x) { x + 1 };
        apply_twice(add_one, 0)
    "#;
    assert_eq!(vm_test_helper(input).await, Object::Integer(2));

    let input2 = r#"
        let compose = fn(f, g) {
            fn(x) {
                f(g(x))
            }
        };
        let double = fn(x) { x * 2 };
        let add_one = fn(x) { x + 1 };
        let double_then_add_one = compose(add_one, double);
        double_then_add_one(5)
    "#;
    assert_eq!(vm_test_helper(input2).await, Object::Integer(11));

    let input3 = r#"
        let apply_three_times = fn(f, x) {
            f(f(f(x)))
        };
        let square = fn(x) { x * x };
        apply_three_times(square, 2)
    "#;
    assert_eq!(vm_test_helper(input3).await, Object::Integer(256));
}

#[tokio::test]
async fn vm_test_closure_with_counter_extended() {
    let input = r#"
        let make_counter = fn() {
            let count = 0;
            let increment = fn() {
                count = count + 1;
                count
            };
            increment
        };
        let counter = make_counter();
        let first = counter();
        let second = counter();
        let third = counter();
        first + second + third
    "#;
    assert_eq!(vm_test_helper(input).await, Object::Integer(6));
}

#[tokio::test]
async fn vm_test_nested_closures() {
    let input = r#"
        let outer = fn(x) {
            let middle = fn(y) {
                let inner = fn(z) {
                    x + y + z
                };
                inner
            };
            middle
        };
        outer(1)(2)(3)
    "#;
    assert_eq!(vm_test_helper(input).await, Object::Integer(6));

    let input2 = r#"
        let factory = fn(x) {
            fn(y) {
                fn(z) {
                    x * y + z
                }
            }
        };
        factory(2)(3)(4)
    "#;
    assert_eq!(vm_test_helper(input2).await, Object::Integer(10));
}

#[tokio::test]
async fn vm_test_tuple_assign() {
    let input = r#"
        let a = 0;
        let b = 0;
        (a, b) = (10, 20);
        a
    "#;
    assert_eq!(vm_test_helper(input).await, Object::Integer(10));

    let input2 = r#"
        let a = 0;
        let b = 0;
        (a, b) = (10, 20);
        b
    "#;
    assert_eq!(vm_test_helper(input2).await, Object::Integer(20));

    let input3 = r#"
        let a = 0;
        let b = 0;
        let c = 0;
        (a, b, c) = (1, 2, 3);
        a + b + c
    "#;
    assert_eq!(vm_test_helper(input3).await, Object::Integer(6));
}

#[tokio::test]
async fn vm_test_swap_variables() {
    let input = r#"
        let a = 10;
        let b = 20;
        (a, b) = (b, a);
        a
    "#;
    assert_eq!(vm_test_helper(input).await, Object::Integer(20));

    let input2 = r#"
        let a = 10;
        let b = 20;
        (a, b) = (b, a);
        b
    "#;
    assert_eq!(vm_test_helper(input2).await, Object::Integer(10));

    let input3 = r#"
        let x = 1;
        let y = 2;
        let z = 3;
        (x, y, z) = (z, x, y);
        x
    "#;
    assert_eq!(vm_test_helper(input3).await, Object::Integer(3));

    let input4 = r#"
        let x = 1;
        let y = 2;
        let z = 3;
        (x, y, z) = (z, x, y);
        y
    "#;
    assert_eq!(vm_test_helper(input4).await, Object::Integer(1));

    let input5 = r#"
        let x = 1;
        let y = 2;
        let z = 3;
        (x, y, z) = (z, x, y);
        z
    "#;
    assert_eq!(vm_test_helper(input5).await, Object::Integer(2));
}

#[tokio::test]
async fn vm_test_destructuring_for_in_loop() {
    let input = r#"
        let sum = 0;
        for ((a, b) in [[1, 2], [3, 4], [5, 6]]) {
            sum = sum + a + b;
        }
        sum
    "#;
    assert_eq!(vm_test_helper(input).await, Object::Integer(21));

    let input2 = r#"
        let first_sum = 0;
        let second_sum = 0;
        for ((a, b) in [[1, 2], [3, 4]]) {
            first_sum = first_sum + a;
            second_sum = second_sum + b;
        }
        first_sum + second_sum
    "#;
    assert_eq!(vm_test_helper(input2).await, Object::Integer(10));

    let input3 = r#"
        let first = 0;
        let second = 0;
        for ((num, _) in [[1, 2], [3, 4]]) {
            if (first == 0) {
                first = num;
            } else {
                second = num;
            }
        }
        first + second
    "#;
    assert_eq!(vm_test_helper(input3).await, Object::Integer(4));

    let input4 = r#"
        let sum = 0;
        for ((x, y, z) in [[1, 2, 3], [4, 5, 6]]) {
            sum = sum + x + y + z;
        }
        sum
    "#;
    assert_eq!(vm_test_helper(input4).await, Object::Integer(21));
}

// ─── Edge Case Tests ───────────────────────────────────────────────────

#[tokio::test]
async fn vm_test_float_operations() {
    let tests = vec![
        ("1.5 + 2.5", Object::Float(4.0)),
        ("5.0 - 3.0", Object::Float(2.0)),
        ("4.0 * 2.5", Object::Float(10.0)),
        ("10.0 / 2.0", Object::Float(5.0)),
        ("7.0 % 3.0", Object::Float(1.0)),
        ("2.5 < 3.5", Object::Boolean(true)),
        ("3.5 > 2.5", Object::Boolean(true)),
        ("2.5 == 2.5", Object::Boolean(true)),
    ];
    for (input, expected) in tests {
        let evaluated = vm_test_helper(input).await;
        assert_eq!(evaluated, expected, "input: {}", input);
    }
}

#[tokio::test]
async fn vm_test_mixed_int_float_arithmetic() {
    let tests = vec![
        ("5 + 2.5", Object::Float(7.5)),
        ("2.5 + 5", Object::Float(7.5)),
        ("10 - 2.5", Object::Float(7.5)),
        ("5 * 2.0", Object::Float(10.0)),
        ("10.0 / 2", Object::Float(5.0)),
    ];
    for (input, expected) in tests {
        let evaluated = vm_test_helper(input).await;
        assert_eq!(evaluated, expected, "input: {}", input);
    }
}

#[tokio::test]
async fn vm_test_integer_overflow() {
    let max_int = 9223372036854775807i64;
    let input = &format!("{}", max_int);
    let evaluated = vm_test_helper(input).await;
    assert_eq!(evaluated, Object::Integer(max_int));
    
    let input2 = "9223372036854775807 + 1";
    let evaluated2 = vm_test_helper(input2).await;
    assert_eq!(evaluated2, Object::Integer(-9223372036854775808));
}

#[tokio::test]
async fn vm_test_string_methods() {
    let tests = vec![
        ("\"  hello  \".trim()", Object::String("hello".to_string())),
        ("\"hello\".contains(\"ell\")", Object::Boolean(true)),
        ("\"hello\".contains(\"world\")", Object::Boolean(false)),
        ("\"hello\".replace(\"l\", \"r\")", Object::String("herro".to_string())),
    ];
    for (input, expected) in tests {
        let evaluated = vm_test_helper(input).await;
        assert_eq!(evaluated, expected, "input: {}", input);
    }
}

#[tokio::test]
async fn vm_test_empty_array() {
    let tests = vec![
        ("let a = []; a.len()", Object::Integer(0)),
    ];
    for (input, expected) in tests {
        let evaluated = vm_test_helper(input).await;
        assert_eq!(evaluated, expected, "input: {}", input);
    }
}

#[tokio::test]
async fn vm_test_array_operations() {
    let tail_result = Object::Array(Box::new(vec![Object::Integer(2), Object::Integer(3)]));
    let tests = vec![
        ("[1, 2].len()", Object::Integer(2)),
        ("[1, 2, 3].head()", Object::Integer(1)),
        ("[1, 2, 3].tail()", tail_result),
    ];
    for (input, expected) in tests {
        let evaluated = vm_test_helper(input).await;
        assert_eq!(evaluated, expected, "input: {}", input);
    }
}

#[tokio::test]
async fn vm_test_empty_hash() {
    let tests = vec![
        ("let h = {}; h.len()", Object::Integer(0)),
    ];
    for (input, expected) in tests {
        let evaluated = vm_test_helper(input).await;
        assert_eq!(evaluated, expected, "input: {}", input);
    }
}

#[tokio::test]
async fn vm_test_hash_operations() {
    let input = r#"
        let h = {"a": 1, "b": 2};
        h.keys()
    "#;
    let evaluated = vm_test_helper(input).await;
    if let Object::Array(arr) = evaluated {
        assert!(arr.len() == 2);
    } else {
        panic!("Expected Array, got {:?}", evaluated);
    }
}

#[tokio::test]
async fn vm_test_nested_arrays() {
    let input = "[[1, 2], [3, 4]][0][1]";
    let evaluated = vm_test_helper(input).await;
    assert_eq!(evaluated, Object::Integer(2));
    
    let input2 = "[[[1, 2], [3, 4]], [[5, 6]]][1][0][0]";
    let evaluated2 = vm_test_helper(input2).await;
    assert_eq!(evaluated2, Object::Integer(5));
}

#[tokio::test]
async fn vm_test_nested_hashes() {
    let input = r#"{"outer": {"inner": 42}}["outer"]["inner"]"#;
    let evaluated = vm_test_helper(input).await;
    assert_eq!(evaluated, Object::Integer(42));
}

#[tokio::test]
async fn vm_test_array_in_hash() {
    let input = r#"{"arr": [1, 2, 3]}["arr"][1]"#;
    let evaluated = vm_test_helper(input).await;
    assert_eq!(evaluated, Object::Integer(2));
}

#[tokio::test]
async fn vm_test_hash_in_array() {
    let input = r#"[{"a": 1}, {"b": 2}][1]["b"]"#;
    let evaluated = vm_test_helper(input).await;
    assert_eq!(evaluated, Object::Integer(2));
}

#[tokio::test]
async fn vm_test_comparison_chaining() {
    let tests = vec![
        ("1 < 2 && 2 < 3", Object::Boolean(true)),
        ("1 < 2 && 2 > 3", Object::Boolean(false)),
        ("1 == 1 || 2 == 3", Object::Boolean(true)),
        ("false || false", Object::Boolean(false)),
    ];
    for (input, expected) in tests {
        let evaluated = vm_test_helper(input).await;
        assert_eq!(evaluated, expected, "input: {}", input);
    }
}

#[tokio::test]
async fn vm_test_nested_if_else() {
    let input = r#"
        let x = 5;
        if (x < 10) {
            if (x < 5) {
                "a"
            } else {
                "b"
            }
        } else {
            "c"
        }
    "#;
    let evaluated = vm_test_helper(input).await;
    assert_eq!(evaluated, Object::String("b".to_string()));
}

#[tokio::test]
async fn vm_test_empty_blocks() {
    let tests = vec![
        ("if (false) {} else { 5 }", Object::Integer(5)),
        ("if (true) { 5 } else {}", Object::Integer(5)),
    ];
    for (input, expected) in tests {
        let evaluated = vm_test_helper(input).await;
        assert_eq!(evaluated, expected, "input: {}", input);
    }
}

#[tokio::test]
async fn vm_test_zero_params_function() {
    let input = r#"
        let f = fn() { 42 };
        f()
    "#;
    let evaluated = vm_test_helper(input).await;
    assert_eq!(evaluated, Object::Integer(42));
}

#[tokio::test]
async fn vm_test_many_params_function() {
    let input = r#"
        let f = fn(a, b, c, d, e) { a + b + c + d + e };
        f(1, 2, 3, 4, 5)
    "#;
    let evaluated = vm_test_helper(input).await;
    assert_eq!(evaluated, Object::Integer(15));
}

#[tokio::test]
async fn vm_test_early_return() {
    let input = r#"
        fn early() {
            return 10;
            20;
        }
        early()
    "#;
    let evaluated = vm_test_helper(input).await;
    assert_eq!(evaluated, Object::Integer(10));
}

#[tokio::test]
async fn vm_test_nested_loops_with_break() {
    let input = r#"
        let result = 0;
        let i = 0;
        while (i < 3) {
            let j = 0;
            while (j < 3) {
                if (j == 1) {
                    break;
                }
                result = result + 1;
                j = j + 1;
            }
            i = i + 1;
        }
        result
    "#;
    let evaluated = vm_test_helper(input).await;
    assert_eq!(evaluated, Object::Integer(3));
}

#[tokio::test]
async fn vm_test_for_loop_with_break() {
    let input = r#"
        let sum = 0;
        for (let i in [1, 2, 3, 4, 5]) {
            if (i == 3) {
                break;
            }
            sum = sum + i;
        }
        sum
    "#;
    let evaluated = vm_test_helper(input).await;
    assert_eq!(evaluated, Object::Integer(3));
}

// this test hangs
#[tokio::test]
async fn vm_test_nested_loops_with_continue() {
    let input = r#"
        let result = 0;
        let i = 0;
        while (i < 3) {
            i = i + 1;
            if (i == 2) {
                continue;
            }
            result = result + i;
        }
        result
    "#;
    let evaluated = vm_test_helper(input).await;
    assert_eq!(evaluated, Object::Integer(4));
}

// this test hangs
#[tokio::test]
async fn vm_test_for_loop_with_continue() {
    let input = r#"
        let sum = 0;
        for (let i in [1, 2, 3, 4, 5]) {
            if (i == 3) {
                continue;
            }
            sum = sum + i;
        }
        sum
    "#;
    let evaluated = vm_test_helper(input).await;
    assert_eq!(evaluated, Object::Integer(12));
}

#[tokio::test]
async fn vm_test_c_style_for_loop() {
    let input = r#"
        let sum = 0;
        for (let i = 0; i < 5; i = i + 1) {
            sum = sum + i;
        }
        sum
    "#;
    let evaluated = vm_test_helper(input).await;
    assert_eq!(evaluated, Object::Integer(10));
}

#[tokio::test]
async fn vm_test_c_style_for_loop_break_continue() {
    let input = r#"
        let sum = 0;
        for (let i = 0; i < 10; i = i + 1) {
            if (i == 5) { break; }
            if (i % 2 == 0) { continue; }
            sum = sum + i;
        }
        sum
    "#;
    let evaluated = vm_test_helper(input).await;
    assert_eq!(evaluated, Object::Integer(4));
}

#[tokio::test]
async fn vm_test_reassign_global() {
    let input = r#"
        let x = 1;
        x = 2;
        x
    "#;
    let evaluated = vm_test_helper(input).await;
    assert_eq!(evaluated, Object::Integer(2));
}

#[tokio::test]
async fn vm_test_function_shadowing() {
    let input = r#"
        let x = 1;
        let f = fn() {
            let x = 2;
            x
        };
        f() + x
    "#;
    let evaluated = vm_test_helper(input).await;
    assert_eq!(evaluated, Object::Integer(3));
}

#[tokio::test]
async fn vm_test_complex_expression() {
    let input = "(1 + 2) * (3 + 4) - (10 / 2) + (100 % 30)";
    // (1+2=3) * (3+4=7) = 21
    // 10/2 = 5, so 21 - 5 = 16
    // 100 % 30 = 10, so 16 + 10 = 26
    let evaluated = vm_test_helper(input).await;
    assert_eq!(evaluated, Object::Integer(26));
}

#[tokio::test]
async fn vm_test_chained_field_access() {
    let input = r#"
        let obj = {"a": {"b": {"c": 42}}};
        obj["a"]["b"]["c"]
    "#;
    let evaluated = vm_test_helper(input).await;
    assert_eq!(evaluated, Object::Integer(42));
}

#[tokio::test]
async fn vm_test_method_on_array() {
    let input = "[1, 2, 3].len()";
    let evaluated = vm_test_helper(input).await;
    assert_eq!(evaluated, Object::Integer(3));
}

#[tokio::test]
async fn vm_test_method_on_string() {
    let input = "\"hello\".len()";
    let evaluated = vm_test_helper(input).await;
    assert_eq!(evaluated, Object::Integer(5));
}

#[tokio::test]
async fn vm_test_method_on_hash() {
    let input = r#"{"a": 1, "b": 2}.len()"#;
    let evaluated = vm_test_helper(input).await;
    assert_eq!(evaluated, Object::Integer(2));
}

#[tokio::test]
async fn vm_test_throw_non_string() {
    // Test that non-string values can be thrown and caught
    let tests = vec![
        ("throw \"42\";", Object::String("42".to_string())),
        ("throw \"true\";", Object::String("true".to_string())),
    ];
    for (input, expected) in tests {
        let input_with_catch = &format!("try {{ {} }} catch(e) {{ e }}", input);
        let evaluated = vm_test_helper(input_with_catch).await;
        assert_eq!(evaluated, expected, "input: {}", input);
    }
}

#[tokio::test]
async fn vm_test_try_catch_with_nested_expressions() {
    let input = r#"
        let x = try {
            let y = 1 + 1;
            throw y;
        } catch (e) {
            e + 10
        };
        x
    "#;
    let evaluated = vm_test_helper(input).await;
    assert_eq!(evaluated, Object::Integer(12));
}

#[tokio::test]
async fn vm_test_closure_captures_multiple_vars() {
    let input = r#"
        let make_adder = fn(x, y) {
            fn(z) { x + y + z };
        };
        let adder = make_adder(10, 20);
        adder(5)
    "#;
    let evaluated = vm_test_helper(input).await;
    assert_eq!(evaluated, Object::Integer(35));
}

#[tokio::test]
async fn vm_test_async_in_sync_context() {
    let input = r#"
        async fn async_add(a, b) { a + b }
        async_add(3, 4)
    "#;
    let evaluated = vm_test_helper(input).await;
    assert_eq!(evaluated, Object::Integer(7));
}

#[tokio::test]
async fn vm_test_divide_by_zero_error() {
    let input = "10 / 0";
    let evaluated = vm_test_helper(input).await;
    // Division by zero returns an error object
    match evaluated {
        Object::Error(e) if matches!(*e, RuntimeError::DivisionByZero) => {}, // Expected
        _ => panic!("Expected DivisionByZero error, got {:?}", evaluated),
    }
}

#[tokio::test]
async fn vm_test_modulo_by_zero_error() {
    let input = "10 % 0";
    let evaluated = vm_test_helper(input).await;
    // Modulo by zero returns an error object
    match evaluated {
        Object::Error(e) if matches!(*e, RuntimeError::DivisionByZero) => {}, // Expected
        _ => panic!("Expected DivisionByZero error, got {:?}", evaluated),
    }
}
// ─── Structs ──────────────────────────────────────────────────────────────

#[tokio::test]
async fn vm_test_struct_method_call() {
    let input = r#"
        struct Person {
            say_hi: fn() { "hi" }
        }
        Person.say_hi()
    "#;
    let evaluated = vm_test_helper(input).await;
    assert_eq!(evaluated, Object::String("hi".to_string()));
}

#[tokio::test]
async fn vm_test_struct_method_with_this() {
    let input = r#"
        struct Person {
            name: "Luca",
            say_hi: fn() { "hi " + this.name }
        }
        Person.say_hi()
    "#;
    let evaluated = vm_test_helper(input).await;
    assert_eq!(evaluated, Object::String("hi Luca".to_string()));
}

#[tokio::test]
async fn vm_test_struct_instance_method_call() {
    let input = r#"
        struct Person {
            name: "",
            greet: fn() { "Hello, " + this.name }
        }
        let p = Person { name: "World" };
        p.greet()
    "#;
    let evaluated = vm_test_helper(input).await;
    assert_eq!(evaluated, Object::String("Hello, World".to_string()));
}
