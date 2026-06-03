use crate::ast::ast::{Expr, Ident, Infix, Literal, Prefix, Program, Stmt};
use crate::lexer::lexer::Lexer;
use crate::lexer::token::SpannedTokens;
use crate::parser::parser::Parser;

fn mk_ident(name: &str) -> Ident {
    Ident::new(name.to_string())
}

fn parse_test_helper(input: &str) -> Program {
    let spanned_tokens = Lexer::lex_tokens(input.as_bytes()).unwrap();
    let spanned = SpannedTokens::new(&spanned_tokens);
    let tokens = spanned.to_tokens();
    let result = Parser::parse_tokens(tokens);
    assert!(
        result.is_ok(),
        "Parser returned an error: {:?}",
        result.err()
    );
    let (remaining_tokens, program) = result.unwrap();
    assert_eq!(
        remaining_tokens.token.len(),
        0,
        "Parser did not consume all tokens including EOF"
    );
    program
}

#[test]
fn test_let_statements() {
    let input = "
        let x = 5;
        let y = 10;
        let foobar = 838383;
    ";

    let program = parse_test_helper(input);

    let expected = vec![
        Stmt::LetStmt(mk_ident("x"), Expr::LitExpr(Literal::IntLiteral(5))),
        Stmt::LetStmt(mk_ident("y"), Expr::LitExpr(Literal::IntLiteral(10))),
        Stmt::LetStmt(
            mk_ident("foobar"),
            Expr::LitExpr(Literal::IntLiteral(838383)),
        ),
    ];

    assert_eq!(program.len(), 3);
    for (i, stmt) in program.iter().enumerate() {
        assert_eq!(*stmt, expected[i]);
    }
}

#[test]
fn test_return_statements() {
    let input = "
        return 5;
        return 10;
        return 993322;
    ";

    let program = parse_test_helper(input);

    let expected = vec![
        Stmt::ReturnStmt(Expr::LitExpr(Literal::IntLiteral(5))),
        Stmt::ReturnStmt(Expr::LitExpr(Literal::IntLiteral(10))),
        Stmt::ReturnStmt(Expr::LitExpr(Literal::IntLiteral(993322))),
    ];

    assert_eq!(program.len(), 3);
    for (i, stmt) in program.iter().enumerate() {
        assert_eq!(*stmt, expected[i]);
    }
}

#[test]
fn test_identifier_expression() {
    let input = "foobar;";
    let program = parse_test_helper(input);

    assert_eq!(program.len(), 1);
    let stmt = &program[0];
    assert_eq!(*stmt, Stmt::ExprStmt(Expr::IdentExpr(mk_ident("foobar"))));
}

#[test]
fn test_integer_literal_expression() {
    let input = "5;";
    let program = parse_test_helper(input);

    assert_eq!(program.len(), 1);
    let stmt = &program[0];
    assert_eq!(*stmt, Stmt::ExprStmt(Expr::LitExpr(Literal::IntLiteral(5))));
}

#[test]
fn test_prefix_expressions() {
    let tests = vec![
        ("!5;", Prefix::Not, Literal::IntLiteral(5)),
        ("-15;", Prefix::PrefixMinus, Literal::IntLiteral(15)),
    ];

    for (input, prefix, literal) in tests {
        let program = parse_test_helper(input);
        assert_eq!(program.len(), 1);
        let stmt = &program[0];
        assert_eq!(
            *stmt,
            Stmt::ExprStmt(Expr::PrefixExpr(prefix, Box::new(Expr::LitExpr(literal))))
        );
    }
}

#[test]
fn test_infix_expressions() {
    let tests = vec![
        ("5 + 5;", 5, Infix::Plus, 5),
        ("5 - 5;", 5, Infix::Minus, 5),
        ("5 * 5;", 5, Infix::Multiply, 5),
        ("5 / 5;", 5, Infix::Divide, 5),
        ("5 > 5;", 5, Infix::GreaterThan, 5),
        ("5 < 5;", 5, Infix::LessThan, 5),
        ("5 == 5;", 5, Infix::Equal, 5),
        ("5 != 5;", 5, Infix::NotEqual, 5),
    ];

    for (input, left, infix, right) in tests {
        let program = parse_test_helper(input);
        assert_eq!(program.len(), 1);
        let stmt = &program[0];
        assert_eq!(
            *stmt,
            Stmt::ExprStmt(Expr::InfixExpr(
                infix,
                Box::new(Expr::LitExpr(Literal::IntLiteral(left))),
                Box::new(Expr::LitExpr(Literal::IntLiteral(right)))
            ))
        );
    }
}

#[test]
fn test_operator_precedence() {
    let input = "-a * b;";
    let program = parse_test_helper(input);
    assert_eq!(program.len(), 1);
    let expected_expr = Expr::InfixExpr(
        Infix::Multiply,
        Box::new(Expr::PrefixExpr(
            Prefix::PrefixMinus,
            Box::new(Expr::IdentExpr(mk_ident("a"))),
        )),
        Box::new(Expr::IdentExpr(mk_ident("b"))),
    );
    match &program[0] {
        Stmt::ExprStmt(expr) => assert_eq!(*expr, expected_expr),
        _ => panic!("Expected ExprStmt"),
    }

    let input = "a + b * c;";
    let program = parse_test_helper(input);
    assert_eq!(program.len(), 1);
    let expected_expr = Expr::InfixExpr(
        Infix::Plus,
        Box::new(Expr::IdentExpr(mk_ident("a"))),
        Box::new(Expr::InfixExpr(
            Infix::Multiply,
            Box::new(Expr::IdentExpr(mk_ident("b"))),
            Box::new(Expr::IdentExpr(mk_ident("c"))),
        )),
    );
    match &program[0] {
        Stmt::ExprStmt(expr) => assert_eq!(*expr, expected_expr),
        _ => panic!("Expected ExprStmt"),
    }
}

#[test]
fn test_if_expression() {
    let input = "if (x < y) { x; }";
    let program = parse_test_helper(input);

    assert_eq!(program.len(), 1);
    let stmt = &program[0];
    let expr = match stmt {
        Stmt::ExprValueStmt(e) => e,
        _ => panic!("Expected ExprValueStmt, got {:?}", stmt),
    };

    if let Expr::IfExpr {
        cond,
        consequence,
        alternative,
    } = expr
    {
        assert_eq!(
            **cond,
            Expr::InfixExpr(
                Infix::LessThan,
                Box::new(Expr::IdentExpr(mk_ident("x"))),
                Box::new(Expr::IdentExpr(mk_ident("y")))
            )
        );
        assert_eq!(consequence.len(), 1);
        assert_eq!(
            consequence[0],
            Stmt::ExprStmt(Expr::IdentExpr(mk_ident("x")))
        );
        assert!(alternative.is_none());
    } else {
        panic!("Expected IfExpr");
    }
}

#[test]
fn test_if_else_if_expression() {
    let input = "if (x < y) { x; } else if (x > y) { y; } else { z; }";
    let program = parse_test_helper(input);

    assert_eq!(program.len(), 1);
    let stmt = &program[0];
    let expr = match stmt {
        Stmt::ExprValueStmt(e) => e,
        _ => panic!("Expected ExprValueStmt, got {:?}", stmt),
    };

    if let Expr::IfExpr {
        cond,
        consequence,
        alternative,
    } = expr
    {
        // if (x < y) { x; }
        assert_eq!(
            **cond,
            Expr::InfixExpr(
                Infix::LessThan,
                Box::new(Expr::IdentExpr(mk_ident("x"))),
                Box::new(Expr::IdentExpr(mk_ident("y")))
            )
        );
        assert_eq!(consequence.len(), 1);
        assert_eq!(
            consequence[0],
            Stmt::ExprStmt(Expr::IdentExpr(mk_ident("x")))
        );

        // else if (x > y) { y; } else { z; }
        assert!(alternative.is_some());
        let alt_program = alternative.as_ref().unwrap();
        assert_eq!(alt_program.len(), 1);

        let else_if_stmt = &alt_program[0];
        if let Stmt::ExprValueStmt(Expr::IfExpr {
            cond,
            consequence,
            alternative,
        }) = else_if_stmt
        {
            // if (x > y) { y; }
            assert_eq!(
                **cond,
                Expr::InfixExpr(
                    Infix::GreaterThan,
                    Box::new(Expr::IdentExpr(mk_ident("x"))),
                    Box::new(Expr::IdentExpr(mk_ident("y")))
                )
            );
            assert_eq!(consequence.len(), 1);
            assert_eq!(
                consequence[0],
                Stmt::ExprStmt(Expr::IdentExpr(mk_ident("y")))
            );

            // else { z; }
            assert!(alternative.is_some());
            let final_else_program = alternative.as_ref().unwrap();
            assert_eq!(final_else_program.len(), 1);
            assert_eq!(
                final_else_program[0],
                Stmt::ExprStmt(Expr::IdentExpr(mk_ident("z")))
            );
        } else {
            panic!("Expected else if to be an IfExpr");
        }
    } else {
        panic!("Expected IfExpr");
    }
}

#[test]
fn test_function_literal_parsing() {
    let input = "fn(x, y) { x + y }";
    let program = parse_test_helper(input);

    assert_eq!(program.len(), 1);
    let stmt = &program[0];
    let expr = match stmt {
        Stmt::ExprValueStmt(e) => e,
        _ => panic!("Expected ExprValueStmt"),
    };

    if let Expr::FnExpr { params, body } = expr {
        assert_eq!(params.len(), 2);
        assert_eq!(params[0], mk_ident("x"));
        assert_eq!(params[1], mk_ident("y"));
        assert_eq!(body.len(), 1);
        assert_eq!(
            body[0],
            Stmt::ExprValueStmt(Expr::InfixExpr(
                Infix::Plus,
                Box::new(Expr::IdentExpr(mk_ident("x"))),
                Box::new(Expr::IdentExpr(mk_ident("y")))
            ))
        );
    } else {
        panic!("Expected FnExpr");
    }
}

#[test]
fn test_call_expression_parsing() {
    let input = "add(1, 2 * 3, 4 + 5);";
    let program = parse_test_helper(input);

    assert_eq!(program.len(), 1);
    let stmt = &program[0];
    let expr = match stmt {
        Stmt::ExprStmt(e) => e,
        _ => panic!("Expected ExprStmt"),
    };

    if let Expr::CallExpr {
        function,
        arguments,
    } = expr
    {
        assert_eq!(**function, Expr::IdentExpr(mk_ident("add")));
        assert_eq!(arguments.len(), 3);
        assert_eq!(arguments[0], Expr::LitExpr(Literal::IntLiteral(1)));
        assert_eq!(
            arguments[1],
            Expr::InfixExpr(
                Infix::Multiply,
                Box::new(Expr::LitExpr(Literal::IntLiteral(2))),
                Box::new(Expr::LitExpr(Literal::IntLiteral(3)))
            )
        );
        assert_eq!(
            arguments[2],
            Expr::InfixExpr(
                Infix::Plus,
                Box::new(Expr::LitExpr(Literal::IntLiteral(4))),
                Box::new(Expr::LitExpr(Literal::IntLiteral(5)))
            )
        );
    } else {
        panic!("Expected CallExpr");
    }
}

#[test]
fn test_assignment_statement() {
    let input = "x = 5;";
    let program = parse_test_helper(input);
    assert_eq!(program.len(), 1);
    let expected = Stmt::AssignStmt(mk_ident("x"), Expr::LitExpr(Literal::IntLiteral(5)));
    assert_eq!(program[0], expected);
}

#[test]
fn test_if_in_function_allows_implicit_return() {
    let input = "fn a() { x }";
    let program = parse_test_helper(input);
    assert_eq!(program.len(), 1);
    let stmt = &program[0];
    let body_stmt = match stmt {
        Stmt::FnStmt { name, params, body } => {
            assert_eq!(name, &mk_ident("a"));
            assert_eq!(params.len(), 0);
            assert_eq!(body.len(), 1);
            &body[0]
        }
        _ => panic!("Expected FnStmt, got {:?}", stmt),
    };
    if let Stmt::ExprValueStmt(expr) = body_stmt {
        assert_eq!(*expr, Expr::IdentExpr(mk_ident("x")));
    } else {
        panic!("Expected ExprValueStmt, got {:?}", body_stmt);
    }
}

#[test]
fn test_while_loop() {
    let input = "while (x < 5) { x = x + 1; }";
    let program = parse_test_helper(input);
    assert_eq!(program.len(), 1);

    let stmt = &program[0];
    if let Stmt::ExprStmt(Expr::WhileExpr { cond, body }) = stmt {
        assert_eq!(
            **cond,
            Expr::InfixExpr(
                Infix::LessThan,
                Box::new(Expr::IdentExpr(mk_ident("x"))),
                Box::new(Expr::LitExpr(Literal::IntLiteral(5)))
            )
        );
        assert_eq!(body.len(), 1);
        assert_eq!(
            body[0],
            Stmt::AssignStmt(
                mk_ident("x"),
                Expr::InfixExpr(
                    Infix::Plus,
                    Box::new(Expr::IdentExpr(mk_ident("x"))),
                    Box::new(Expr::LitExpr(Literal::IntLiteral(1)))
                )
            )
        );
    } else {
        panic!("Expected Stmt::ExprStmt(Expr::WhileExpr), got {:?}", stmt);
    }
}

#[test]
fn test_c_style_for_loop() {
    let input = "for (let i = 0; i < 10; i = i + 1) { i; }";
    let program = parse_test_helper(input);
    assert_eq!(program.len(), 1);

    let stmt = &program[0];
    if let Stmt::ExprStmt(Expr::CStyleForExpr {
        init,
        cond,
        update,
        body,
    }) = stmt
    {
        // init
        assert!(init.is_some());
        let init_stmt = init.as_ref().unwrap();
        assert_eq!(
            **init_stmt,
            Stmt::LetStmt(mk_ident("i"), Expr::LitExpr(Literal::IntLiteral(0)))
        );

        // cond
        assert!(cond.is_some());
        let cond_expr = cond.as_ref().unwrap();
        assert_eq!(
            **cond_expr,
            Expr::InfixExpr(
                Infix::LessThan,
                Box::new(Expr::IdentExpr(mk_ident("i"))),
                Box::new(Expr::LitExpr(Literal::IntLiteral(10)))
            )
        );

        // update
        assert!(update.is_some());
        let update_stmt = update.as_ref().unwrap();
        assert_eq!(
            **update_stmt,
            Stmt::AssignStmt(
                mk_ident("i"),
                Expr::InfixExpr(
                    Infix::Plus,
                    Box::new(Expr::IdentExpr(mk_ident("i"))),
                    Box::new(Expr::LitExpr(Literal::IntLiteral(1)))
                )
            )
        );

        // body
        assert_eq!(body.len(), 1);
        assert_eq!(body[0], Stmt::ExprStmt(Expr::IdentExpr(mk_ident("i"))));
    } else {
        panic!(
            "Expected Stmt::ExprStmt(Expr::CStyleForExpr), got {:?}",
            stmt
        );
    }
}

#[test]
fn test_for_in_loop() {
    let input = "for (item in items) { item; }";
    let program = parse_test_helper(input);
    assert_eq!(program.len(), 1);

    let stmt = &program[0];
    if let Stmt::ExprStmt(Expr::ForExpr {
        ident,
        iterable,
        body,
    }) = stmt
    {
        assert_eq!(ident.len(), 1);
        assert_eq!(ident[0], mk_ident("item"));
        assert_eq!(**iterable, Expr::IdentExpr(mk_ident("items")));
        assert_eq!(body.len(), 1);
        assert_eq!(body[0], Stmt::ExprStmt(Expr::IdentExpr(mk_ident("item"))));
    } else {
        panic!("Expected Stmt::ExprStmt(Expr::ForExpr), got {:?}", stmt);
    }
}

#[test]
fn test_enum_statement_parsing() {
    let input = "enum Color { Red, Green, Blue(u8), Alpha { r, g, b } }";
    let program = parse_test_helper(input);
    assert_eq!(program.len(), 1);
    // Further inspection of AST if needed
}

#[test]
fn test_match_expression_parsing() {
    let input = "
        match x {
            1 => { println(\"one\"); },
            2 => println(\"two\"),
        }
    ";
    let spanned_tokens = Lexer::lex_tokens(input.as_bytes()).unwrap();
    let spanned = SpannedTokens::new(&spanned_tokens);
    let result = Parser::parse_tokens(spanned.to_tokens());
    assert!(result.is_ok());
}