//! Pratt-parser implementation built on `nom` combinators.
//!
//! The parser consumes a stream of [`Token`]s and produces an
//! [`ast::Program`](crate::ast::ast::Program). Expression parsing uses a
//! precedence-climbing (Pratt) strategy so that operator precedence and
//! associativity are handled correctly without grammar ambiguity.

use nom::bytes::complete::take;
use nom::combinator::{cut, map, opt, verify};
use nom::error::{Error, ErrorKind};
use nom::multi::many0;
use nom::sequence::*;
use nom::Err;
use nom::{branch::*, IResult};
use std::result::Result::*;

use crate::ast::ast::{
    Expr, Ident, ImportItems, Infix, Literal, Pattern, Precedence, Prefix, Program, Stmt,
};
use crate::lexer::token::{Token, Tokens};
use crate::parser::await_ctx_helpers::validate_await_usage;
use crate::parser::parser_helpers::*;

/// Generates a parser that consumes exactly one token matching `$tag`.
///
/// Each invocation produces a function `fn $func_name(tokens: Tokens) -> IResult<Tokens, Tokens>`
/// that peeks the first token, checks equality, and advances the stream on success.
macro_rules! tag_token {
    ($func_name:ident, $tag:expr) => {
        #[inline]
        pub(crate) fn $func_name(tokens: Tokens) -> IResult<Tokens, Tokens> {
            verify(take(1usize), |t: &Tokens| {
                !t.token.is_empty() && t.token[0] == $tag
            })(tokens)
        }
    };
}

// ─── Literal and identifier parsers ─────────────────────────────────

fn parse_literal(input: Tokens) -> IResult<Tokens, Literal> {
    let (i1, t1) = take(1usize)(input)?;

    if t1.token.is_empty() {
        return Err(Err::Error(Error::new(input, ErrorKind::Tag)));
    }

    // Pattern matching to know when i need to clone and when i don't
    match &t1.token[0] {
        Token::IntLiteral(n) => Ok((i1, Literal::IntLiteral(*n))),
        Token::BigIntLiteral(n) => Ok((i1, Literal::BigIntLiteral(n.clone()))),
        Token::FloatLiteral(f) => Ok((i1, Literal::FloatLiteral(*f))),
        Token::StringLiteral(s) => Ok((i1, Literal::StringLiteral(s.clone()))),
        Token::BoolLiteral(b) => Ok((i1, Literal::BoolLiteral(*b))),
        Token::NullLiteral => Ok((i1, Literal::NullLiteral)),
        _ => Err(Err::Error(Error::new(input, ErrorKind::Tag))),
    }
}

fn parse_ident(input: Tokens) -> IResult<Tokens, Ident> {
    let (i1, t1) = take(1usize)(input)?;

    if t1.token.is_empty() {
        return Err(Err::Error(Error::new(input, ErrorKind::Tag)));
    }

    match &t1.token[0] {
        Token::Ident(name) => Ok((i1, Ident::new(name.clone()))),
        _ => Err(Err::Error(Error::new(input, ErrorKind::Tag))),
    }
}

fn parse_tuple_of_idents(input: Tokens) -> IResult<Tokens, Vec<Ident>> {
    parens(comma_separated1(parse_ident))(input)
}

// ─── Token tag parsers ──────────────────────────────────────────────
// Each macro invocation generates a `fn(tag: Tokens) -> IResult<Tokens, Tokens>`
// that consumes one token if it matches the expected variant.

tag_token!(let_tag, Token::Let);
tag_token!(assign_tag, Token::Assign);
tag_token!(semicolon_tag, Token::SemiColon);
tag_token!(return_tag, Token::Return);
tag_token!(lbrace_tag, Token::LBrace);
tag_token!(rbrace_tag, Token::RBrace);
tag_token!(lparen_tag, Token::LParen);
tag_token!(rparen_tag, Token::RParen);
tag_token!(lbracket_tag, Token::LBracket);
tag_token!(rbracket_tag, Token::RBracket);
tag_token!(comma_tag, Token::Comma);
tag_token!(colon_tag, Token::Colon);
tag_token!(plus_assign_tag, Token::PlusAssign);
tag_token!(minus_assign_tag, Token::MinusAssign);
tag_token!(multiply_assign_tag, Token::MultiplyAssign);
tag_token!(divide_assign_tag, Token::DivideAssign);
tag_token!(modulo_assign_tag, Token::ModuloAssign);
tag_token!(if_tag, Token::If);
tag_token!(else_tag, Token::Else);
tag_token!(function_tag, Token::Function);
tag_token!(eof_tag, Token::EOF);
tag_token!(dot_tag, Token::Dot);
tag_token!(double_colon_tag, Token::DoubleColon);
tag_token!(struct_tag, Token::Struct);
tag_token!(this_tag, Token::This);
tag_token!(import_tag, Token::Import);
tag_token!(while_tag, Token::While);
tag_token!(for_tag, Token::For);
tag_token!(in_tag, Token::In);
tag_token!(break_tag, Token::Break);
tag_token!(continue_tag, Token::Continue);
tag_token!(try_tag, Token::Try);
tag_token!(catch_tag, Token::Catch);
tag_token!(finally_tag, Token::Finally);
tag_token!(throw_tag, Token::Throw);
tag_token!(match_tag, Token::Match);
tag_token!(fat_arrow_tag, Token::FatArrow);
tag_token!(async_tag, Token::Async);
tag_token!(await_tag, Token::Await);

// ─── Operator precedence table ──────────────────────────────────────
// Maps a token to its (precedence, infix_operator) pair.
// Used by the Pratt parser to decide whether to continue binding
// or return the current left-hand side.

fn infix_op(t: &Token) -> (Precedence, Option<Infix>) {
    match *t {
        Token::Or => (Precedence::POr, Some(Infix::Or)),
        Token::And => (Precedence::PAnd, Some(Infix::And)),
        Token::Equal => (Precedence::PEquals, Some(Infix::Equal)),
        Token::NotEqual => (Precedence::PEquals, Some(Infix::NotEqual)),
        Token::LessThanEqual => (Precedence::PLessGreater, Some(Infix::LessThanEqual)),
        Token::GreaterThanEqual => (Precedence::PLessGreater, Some(Infix::GreaterThanEqual)),
        Token::LessThan => (Precedence::PLessGreater, Some(Infix::LessThan)),
        Token::GreaterThan => (Precedence::PLessGreater, Some(Infix::GreaterThan)),
        Token::Plus => (Precedence::PSum, Some(Infix::Plus)),
        Token::Minus => (Precedence::PSum, Some(Infix::Minus)),
        Token::Multiply => (Precedence::PProduct, Some(Infix::Multiply)),
        Token::Divide => (Precedence::PProduct, Some(Infix::Divide)),
        Token::Modulo => (Precedence::PProduct, Some(Infix::Modulo)),
        Token::LParen => (Precedence::PCall, None),
        Token::LBracket => (Precedence::PIndex, None),
        Token::Dot => (Precedence::PCall, None),
        Token::DoubleColon => (Precedence::PCall, None),
        _ => (Precedence::PLowest, None),
    }
}

// ─── Program, expression, and statement parsers ─────────────────────

fn parse_program(input: Tokens) -> IResult<Tokens, Program> {
    terminated(many0(parse_stmt), eof_tag)(input)
}

fn parse_expr(input: Tokens) -> IResult<Tokens, Expr> {
    parse_pratt_expr(input, Precedence::PLowest)
}

/// Top-level statement dispatcher.
///
/// Tries each statement parser in order via `alt`. The final fallback
/// `parse_expr_or_assign_stmt` catches anything that looks like an
/// expression or assignment.
fn parse_stmt(input: Tokens) -> IResult<Tokens, Stmt> {
    alt((
        parse_import_stmt,
        parse_let_stmt,
        parse_tuple_assign_stmt,
        parse_fn_declaration,
        parse_return_stmt,
        parse_struct_stmt,
        parse_while_stmt,
        parse_for_stmt,
        parse_break_stmt,
        parse_continue_stmt,
        parse_throw_stmt,
        parse_expr_or_assign_stmt,
    ))(input)
}

/// Parses both sync and async function declarations.
///
/// An `async fn` is desugared into a `LetStmt` binding an `AsyncFnExpr`,
/// while a plain `fn` becomes a `FnStmt`.
fn parse_fn_declaration(input: Tokens) -> IResult<Tokens, Stmt> {
    map(
        tuple((
            opt(async_tag),
            function_tag,
            parse_ident,
            parens(comma_separated0(parse_ident)),
            parse_block_stmt,
            opt(semicolon_tag),
        )),
        |(is_async, _, name, params, body, _)| {
            if is_async.is_some() {
                Stmt::LetStmt(name, Expr::AsyncFnExpr { params, body })
            } else {
                Stmt::FnStmt { name, params, body }
            }
        },
    )(input)
}

// ─── Assignment and expression parsing ──────────────────────────────

/// Parses an expression, then determines whether it forms a statement.
///
/// Three outcomes are possible:
/// 1. A simple `=` assignment (`x = expr`)
/// 2. A compound assignment (`x += expr`)
/// 3. An expression statement (`expr;` or trailing expr in a block)
///
/// Block expressions (if, fn, while, for, try-catch) do not require a
/// trailing semicolon when they appear at the end of a block.
fn parse_expr_or_assign_stmt(input: Tokens) -> IResult<Tokens, Stmt> {
    let (i1, lhs_expr) = parse_expr(input)?;

    if peek_matches(i1, Token::Assign) {
        // This is an assignment statement
        let (i2, _) = assign_tag(i1)?;
        let (i3, rhs_expr) = parse_expr(i2)?;
        let (i4, _) = semicolon_tag(i3)?;

        match lhs_expr {
            Expr::IdentExpr(ident) => Ok((i4, Stmt::AssignStmt(ident, rhs_expr))),
            Expr::FieldAccessExpr { object, field } => Ok((
                i4,
                Stmt::FieldAssignStmt {
                    object,
                    field,
                    value: Box::new(rhs_expr),
                },
            )),
            Expr::IndexExpr { array, index } => Ok((
                i4,
                Stmt::IndexAssignStmt {
                    target: array,
                    index,
                    value: Box::new(rhs_expr),
                },
            )),
            _ => {
                // Invalid l-value for assignment
                Err(Err::Error(Error::new(input, ErrorKind::Verify)))
            }
        }
    } else if let Ok((i2, infix)) = aug_assign_to_infix(i1) {
        let (i3, rhs_expr) = parse_expr(i2)?;
        let (i4, _) = semicolon_tag(i3)?;

        match lhs_expr {
            Expr::IdentExpr(ident) => Ok((
                i4,
                Stmt::AssignStmt(
                    ident.clone(),
                    Expr::InfixExpr(infix, Box::new(Expr::IdentExpr(ident)), Box::new(rhs_expr)),
                ),
            )),
            _ => Err(Err::Error(Error::new(input, ErrorKind::Verify))),
        }
    } else {
        // This is an expression statement
        let after_expr = i1;
        let expr = lhs_expr;

        if peek_matches(after_expr, Token::SemiColon) {
            let (i_final, _) = semicolon_tag(after_expr)?;
            Ok((i_final, Stmt::ExprStmt(expr)))
        } else {
            let is_block_expr = matches!(
                &expr,
                Expr::IfExpr { .. }
                    | Expr::FnExpr { .. }
                    | Expr::AsyncFnExpr { .. }
                    | Expr::WhileExpr { .. }
                    | Expr::ForExpr { .. }
                    | Expr::CStyleForExpr { .. }
                    | Expr::TryCatchExpr { .. }
                    | Expr::MatchExpr { .. }
            );
            let is_trailing_in_block = peek_matches(after_expr, Token::RBrace);

            if is_block_expr || is_trailing_in_block {
                Ok((after_expr, Stmt::ExprValueStmt(expr)))
            } else {
                // Expect a semicolon for other expression statements
                let (i_final, _) = cut(semicolon_tag)(after_expr)?;
                Ok((i_final, Stmt::ExprStmt(expr)))
            }
        }
    }
}

fn parse_tuple_of_exprs(input: Tokens) -> IResult<Tokens, Vec<Expr>> {
    parens(comma_separated1(parse_expr))(input)
}

// STATEMENT PARSERS

fn parse_let_stmt_no_semicolon(input: Tokens) -> IResult<Tokens, Stmt> {
    let (i1, _) = let_tag(input)?;

    let (i2, mut idents) = alt((
        map(parse_tuple_of_idents, |i| i),
        map(comma_separated1(parse_ident), |i| i),
    ))(i1)?;

    let (i3, _) = assign_tag(i2)?;

    let (i4, mut values): (_, Vec<Expr>) = if peek_matches(i3, Token::LParen) {
        let (rem, vals) = parse_tuple_of_exprs(i3)?;
        (rem, vals)
    } else {
        let (rem, expr) = parse_expr(i3)?;
        (rem, vec![expr])
    };

    if idents.len() == 1 && values.len() == 1 {
        Ok((i4, Stmt::LetStmt(idents.remove(0), values.remove(0))))
    } else {
        Ok((i4, Stmt::MultiLetStmt { idents, values }))
    }
}

fn parse_tuple_assign_stmt(input: Tokens) -> IResult<Tokens, Stmt> {
    if peek_matches(input, Token::LParen) {
        let (i2, targets) = parse_tuple_of_idents(input)?;
        let (i3, _) = assign_tag(i2)?;
        let (i4, values) = parse_tuple_of_exprs(i3)?;
        let (i5, _) = opt(semicolon_tag)(i4)?;
        Ok((i5, Stmt::TupleAssignStmt { targets, values }))
    } else {
        Err(Err::Error(Error::new(input, ErrorKind::Tag)))
    }
}

fn parse_assign_stmt_no_semicolon(input: Tokens) -> IResult<Tokens, Stmt> {
    if peek_matches(input, Token::LParen) {
        let (i2, targets) = parse_tuple_of_idents(input)?;
        let (i3, op) = peek_assign_op(i2)?;
        if op == "=" {
            let (i4, values) = parse_tuple_of_exprs(i3)?;
            Ok((i4, Stmt::TupleAssignStmt { targets, values }))
        } else {
            let (i4, values) = parse_tuple_of_exprs(i3)?;
            let mut stmts = targets
                .iter()
                .zip(values.iter())
                .map(|(id, ex)| {
                    Stmt::AssignStmt(
                        id.clone(),
                        Expr::InfixExpr(
                            assign_op_to_infix(&op),
                            Box::new(Expr::IdentExpr(id.clone())),
                            Box::new(ex.clone()),
                        ),
                    )
                })
                .collect::<Vec<_>>();
            if stmts.len() == 1 {
                Ok((i4, stmts.remove(0)))
            } else {
                Ok((
                    i4,
                    Stmt::MultiLetStmt {
                        idents: targets,
                        values,
                    },
                ))
            }
        }
    } else {
        let (i1, ident) = parse_ident(input)?;
        let (i2, op) = alt((
            map(assign_tag, |_| "=".to_string()),
            map(plus_assign_tag, |_| "+=".to_string()),
            map(minus_assign_tag, |_| "-=".to_string()),
            map(multiply_assign_tag, |_| "*=".to_string()),
            map(divide_assign_tag, |_| "/=".to_string()),
            map(modulo_assign_tag, |_| "%=".to_string()),
        ))(i1)?;
        let (i3, expr) = parse_expr(i2)?;

        let stmt = if op == "=" {
            Stmt::AssignStmt(ident, expr)
        } else {
            let binop_expr = Expr::InfixExpr(
                assign_op_to_infix(&op),
                Box::new(Expr::IdentExpr(ident.clone())),
                Box::new(expr),
            );
            Stmt::AssignStmt(ident, binop_expr)
        };

        Ok((i3, stmt))
    }
}

fn parse_break_stmt(input: Tokens) -> IResult<Tokens, Stmt> {
    map(terminated(break_tag, semicolon_tag), |_| Stmt::BreakStmt)(input)
}

fn parse_continue_stmt(input: Tokens) -> IResult<Tokens, Stmt> {
    map(terminated(continue_tag, semicolon_tag), |_| {
        Stmt::ContinueStmt
    })(input)
}

fn parse_throw_stmt(input: Tokens) -> IResult<Tokens, Stmt> {
    map(
        tuple((throw_tag, parse_expr, semicolon_tag)),
        |(_, expr, _)| Stmt::ThrowStmt(expr),
    )(input)
}

fn parse_let_stmt(input: Tokens) -> IResult<Tokens, Stmt> {
    let (i1, _) = let_tag(input)?;

    let (i2, mut idents) = alt((
        map(parse_tuple_of_idents, |i| i),
        map(comma_separated1(parse_ident), |i| i),
    ))(i1)?;

    let (i3, _) = assign_tag(i2)?;

    let (i4, mut values): (_, Vec<Expr>) = if peek_matches(i3, Token::LParen) {
        let (remaining, vals) = parse_tuple_of_exprs(i3)?;
        (remaining, vals)
    } else {
        let (remaining, expr) = parse_expr(i3)?;
        (remaining, vec![expr])
    };

    let (i5, _) = opt(semicolon_tag)(i4)?;

    if idents.len() == 1 && values.len() == 1 {
        Ok((i5, Stmt::LetStmt(idents.remove(0), values.remove(0))))
    } else {
        Ok((i5, Stmt::MultiLetStmt { idents, values }))
    }
}

fn parse_return_stmt(input: Tokens) -> IResult<Tokens, Stmt> {
    map(
        tuple((return_tag, parse_expr, opt(semicolon_tag))),
        |(_, expr, _)| Stmt::ReturnStmt(expr),
    )(input)
}

fn parse_block_stmt(input: Tokens) -> IResult<Tokens, Program> {
    braced(many0(parse_stmt))(input)
}

// EXPRESSION PARSING

fn parse_fn_expr(input: Tokens) -> IResult<Tokens, Expr> {
    map(
        tuple((
            opt(async_tag),
            function_tag,
            parens(comma_separated0(parse_ident)),
            parse_block_stmt,
        )),
        |(is_async, _, params, body)| {
            if is_async.is_some() {
                Expr::AsyncFnExpr { params, body }
            } else {
                Expr::FnExpr { params, body }
            }
        },
    )(input)
}

fn parse_await_expr(input: Tokens) -> IResult<Tokens, Expr> {
    map(
        tuple((await_tag, |i| parse_pratt_expr(i, Precedence::PPrefix))), // Corrected call
        |(_, expr)| Expr::AwaitExpr(Box::new(expr)),
    )(input)
}

fn parse_atom_expr(input: Tokens) -> IResult<Tokens, Expr> {
    alt((
        parse_literal_expr,
        parse_ident_or_struct_literal,
        parse_fn_expr,
        parse_await_expr,
        parse_if_expr,
        parse_match_expr,
        parse_this_expr,
        parse_array_expr,
        parse_hash_expr,
        parse_prefix_expr,
        parse_try_catch_expr,
        parens(parse_expr),
    ))(input)
}

// ─── Pratt parser (precedence climbing) ─────────────────────────────

/// Core expression parser using Pratt's precedence-climbing algorithm.
///
/// The algorithm works in two phases:
/// 1. Parse an atom (literal, identifier, parenthesised expr, etc.)
/// 2. Loop: while the next infix operator has higher precedence than
///    the current `precedence` threshold, consume it and recurse.
///
/// This correctly handles precedence and left-associativity without
/// the grammar ambiguity issues of a naive recursive approach.
fn parse_pratt_expr(input: Tokens, precedence: Precedence) -> IResult<Tokens, Expr> {
    let (mut i, mut left) = parse_atom_expr(input)?;

    loop {
        let Some(curr_token) = peek_token(i) else {
            break;
        };
        let (peek_precedence, _) = infix_op(curr_token);

        if precedence >= peek_precedence || peek_precedence == Precedence::PLowest {
            break;
        }

        match curr_token {
            Token::LParen => {
                let (i2, args) = parens(comma_separated0(parse_expr))(i)?;
                left = Expr::CallExpr {
                    function: Box::new(left),
                    arguments: args,
                };
                i = i2;
            }
            Token::LBracket => {
                let (i2, index) = bracketed(parse_expr)(i)?;
                left = Expr::IndexExpr {
                    array: Box::new(left),
                    index: Box::new(index),
                };
                i = i2;
            }
            Token::Dot => {
                let (i1, _) = dot_tag(i)?;
                let (
                    i2,
                    Ident {
                        name: field_name, ..
                    },
                ) = parse_ident(i1)?;

                if peek_matches(i2, Token::LParen) {
                    let (i3, args) = parens(comma_separated0(parse_expr))(i2)?;
                    left = Expr::MethodCallExpr {
                        object: Box::new(left),
                        method: field_name,
                        arguments: args,
                    };
                    i = i3;
                } else {
                    left = Expr::FieldAccessExpr {
                        object: Box::new(left),
                        field: field_name,
                    };
                    i = i2;
                }
            }
            Token::DoubleColon => {
                let (i1, _) = double_colon_tag(i)?;
                let (
                    i2,
                    Ident {
                        name: field_name, ..
                    },
                ) = parse_ident(i1)?;

                if peek_matches(i2, Token::LParen) {
                    let (i3, args) = parens(comma_separated0(parse_expr))(i2)?;
                    left = Expr::MethodCallExpr {
                        object: Box::new(left),
                        method: field_name,
                        arguments: args,
                    };
                    i = i3;
                } else {
                    left = Expr::FieldAccessExpr {
                        object: Box::new(left),
                        field: field_name,
                    };
                    i = i2;
                }
            }
            _ => {
                let (_, infix_op_opt) = infix_op(curr_token);
                if let Some(infix) = infix_op_opt {
                    let (i1, _) = take(1usize)(i)?;
                    let (i2, right) = parse_pratt_expr(i1, peek_precedence)?;
                    left = Expr::InfixExpr(infix, Box::new(left), Box::new(right));
                    i = i2;
                } else {
                    break;
                }
            }
        }
    }

    Ok((i, left))
}

fn parse_literal_expr(input: Tokens) -> IResult<Tokens, Expr> {
    map(parse_literal, Expr::LitExpr)(input)
}

fn parse_ident_or_struct_literal(input: Tokens) -> IResult<Tokens, Expr> {
    let (after_ident, ident) = parse_ident(input)?;

    if peek_matches(after_ident, Token::LBrace) {
        match braced(comma_separated0(separated_pair(
            parse_ident,
            colon_tag,
            parse_expr,
        )))(after_ident)
        {
            Ok((i1, fields)) => Ok((
                i1,
                Expr::StructLiteral {
                    name: ident,
                    fields,
                },
            )),
            Err(_) => Ok((after_ident, Expr::IdentExpr(ident))),
        }
    } else {
        Ok((after_ident, Expr::IdentExpr(ident)))
    }
}

fn parse_prefix_expr(input: Tokens) -> IResult<Tokens, Expr> {
    let (i1, t1) = take(1usize)(input)?;

    if t1.token.is_empty() {
        return Err(Err::Error(Error::new(input, ErrorKind::Tag)));
    }

    let prefix = match t1.token[0] {
        Token::Not => Prefix::Not,
        Token::Plus => Prefix::PrefixPlus,
        Token::Minus => Prefix::PrefixMinus,
        _ => return Err(Err::Error(Error::new(input, ErrorKind::Tag))),
    };

    let (i2, expr) = parse_pratt_expr(i1, Precedence::PPrefix)?;
    Ok((i2, Expr::PrefixExpr(prefix, Box::new(expr))))
}

fn parse_if_expr(input: Tokens) -> IResult<Tokens, Expr> {
    map(
        tuple((
            if_tag,
            parens(parse_expr),
            parse_block_stmt,
            opt(preceded(
                else_tag,
                alt((
                    map(parse_if_expr, |expr| vec![Stmt::ExprValueStmt(expr)]),
                    parse_block_stmt,
                )),
            )),
        )),
        |(_, cond, consequence, alternative)| Expr::IfExpr {
            cond: Box::new(cond),
            consequence,
            alternative,
        },
    )(input)
}

fn parse_array_expr(input: Tokens) -> IResult<Tokens, Expr> {
    map(bracketed(comma_separated0(parse_expr)), Expr::ArrayExpr)(input)
}

fn parse_hash_expr(input: Tokens) -> IResult<Tokens, Expr> {
    map(
        braced(comma_separated0(separated_pair(
            parse_expr, colon_tag, parse_expr,
        ))),
        Expr::HashExpr,
    )(input)
}

fn parse_this_expr(input: Tokens) -> IResult<Tokens, Expr> {
    map(this_tag, |_| Expr::ThisExpr)(input)
}

fn parse_try_catch_expr(input: Tokens) -> IResult<Tokens, Expr> {
    let (i1, _) = try_tag(input)?;
    let (i2, try_body) = parse_block_stmt(i1)?;

    let (i3, catch_block) = opt(tuple((catch_tag, parens(parse_ident), parse_block_stmt)))(i2)?;

    let (i4, finally_body) = opt(preceded(finally_tag, parse_block_stmt))(i3)?;

    if catch_block.is_none() && finally_body.is_none() {
        return Err(Err::Error(Error::new(input, ErrorKind::Verify)));
    }

    let (catch_ident, catch_body) = if let Some((_, ident, body)) = catch_block {
        (Some(ident), Some(body))
    } else {
        (None, None)
    };

    Ok((
        i4,
        Expr::TryCatchExpr {
            try_body,
            catch_ident,
            catch_body,
            finally_body,
        },
    ))
}

fn parse_pattern(input: Tokens) -> IResult<Tokens, Pattern> {
    let (i1, t1) = take(1usize)(input)?;
    if t1.token.is_empty() {
        return Err(Err::Error(Error::new(input, ErrorKind::Tag)));
    }
    match &t1.token[0] {
        Token::Ident(s) if s == "_" => Ok((i1, Pattern::Wildcard)),
        _ => {
            let (i2, lit) = parse_literal(input)?;
            Ok((i2, Pattern::Literal(lit)))
        }
    }
}

fn parse_match_expr(input: Tokens) -> IResult<Tokens, Expr> {
    let (i1, _) = match_tag(input)?;
    let (i2, value) = parse_expr(i1)?;
    let (i3, arms) = braced(many0(|i| {
        let (ia, pattern) = parse_pattern(i)?;
        let (ib, _) = fat_arrow_tag(ia)?;
        let (ic, body) = alt((
            parse_block_stmt,
            map(parse_expr, |e| vec![Stmt::ExprValueStmt(e)]),
        ))(ib)?;
        let (id, _) = opt(comma_tag)(ic)?;
        Ok((id, (pattern, body)))
    }))(i2)?;
    Ok((i3, Expr::MatchExpr {
        value: Box::new(value),
        arms,
    }))
}

// STRUCT PARSING

fn parse_struct_stmt(input: Tokens) -> IResult<Tokens, Stmt> {
    map(
        tuple((
            struct_tag,
            parse_ident,
            braced(comma_separated0(separated_pair(
                parse_ident,
                colon_tag,
                parse_expr,
            ))),
            opt(semicolon_tag),
        )),
        |(_, name, pairs, _)| {
            let mut fields = Vec::new();
            let mut methods = Vec::new();

            for (ident, expr) in pairs {
                match expr {
                    Expr::FnExpr { .. } => methods.push((ident, expr)),
                    _ => fields.push((ident, expr)),
                }
            }

            Stmt::StructStmt {
                name,
                fields,
                methods,
            }
        },
    )(input)
}

// LOOP STATEMENTS

fn parse_while_stmt(input: Tokens) -> IResult<Tokens, Stmt> {
    map(
        tuple((while_tag, parens(parse_expr), parse_block_stmt)),
        |(_, cond, body)| {
            Stmt::ExprStmt(Expr::WhileExpr {
                cond: Box::new(cond),
                body,
            })
        },
    )(input)
}

fn parse_for_stmt(input: Tokens) -> IResult<Tokens, Stmt> {
    let (i1, _) = for_tag(input)?;
    let (i2, _) = lparen_tag(i1)?;

    match peek_token(i2) {
        Some(Token::Let) => {
            // Check if it's `for (let x in ...)` or `for (let x = ...; ...; ...)`
            let (after_let, _) = let_tag(i2)?;
            if let Ok((after_ident, ident)) = parse_ident(after_let)
                && peek_matches(after_ident, Token::In) {
                    // for (let x in ...) — parse as for-in
                    let idents = vec![ident];
                    let (i3, _) = in_tag(after_ident)?;
                    let (i4, iterable) = parse_expr(i3)?;
                    let (i5, _) = rparen_tag(i4)?;
                    let (i6, body) = parse_block_stmt(i5)?;
                    return Ok((
                        i6,
                        Stmt::ExprStmt(Expr::ForExpr {
                            ident: idents,
                            iterable: Box::new(iterable),
                            body,
                        }),
                    ));
            }
            // Otherwise fall through to c-style for
            parse_c_style_for(i2)
        }
        Some(Token::Ident(_)) => {
            if let Ok((after_ident, _)) = parse_ident(i2) {
                if peek_matches(after_ident, Token::In) {
                    parse_for_in_loop(i2)
                } else {
                    parse_c_style_for(i2)
                }
            } else {
                parse_c_style_for(i2)
            }
        }
        Some(Token::LParen) => parse_for_in_loop(i2),
        _ => parse_c_style_for(i2),
    }
}

fn parse_for_in_loop(input: Tokens) -> IResult<Tokens, Stmt> {
    let (i1, idents) = alt((
        map(parse_tuple_of_idents, |idents| idents),
        map(parse_ident, |i| vec![i]),
    ))(input)?;

    let (i2, _) = in_tag(i1)?;
    let (i3, iterable) = parse_expr(i2)?;
    let (i4, _) = rparen_tag(i3)?;
    let (i5, body) = parse_block_stmt(i4)?;

    Ok((
        i5,
        Stmt::ExprStmt(Expr::ForExpr {
            ident: idents,
            iterable: Box::new(iterable),
            body,
        }),
    ))
}

fn parse_c_style_for(input: Tokens) -> IResult<Tokens, Stmt> {
    let (i1, init) = opt(alt((
        map(parse_let_stmt_no_semicolon, Box::new),
        map(parse_assign_stmt_no_semicolon, Box::new),
    )))(input)?;

    let (i2, _) = semicolon_tag(i1)?;
    let (i3, cond) = opt(map(parse_expr, Box::new))(i2)?;
    let (i4, _) = semicolon_tag(i3)?;
    let (i5, update) = opt(map(parse_assign_stmt_no_semicolon, Box::new))(i4)?;
    let (i6, _) = rparen_tag(i5)?;
    let (i7, body) = parse_block_stmt(i6)?;

    Ok((
        i7,
        Stmt::ExprStmt(Expr::CStyleForExpr {
            init,
            cond,
            update,
            body,
        }),
    ))
}

// IMPORT STATEMENT

fn parse_import_stmt(input: Tokens) -> IResult<Tokens, Stmt> {
    let (i1, _) = import_tag(input)?;
    let (i2, Ident { name: first, .. }) = parse_ident(i1)?;
    let mut path = vec![first];

    let (i3, rest) = many0(preceded(double_colon_tag, parse_ident))(i2)?;
    for Ident { name, .. } in rest {
        path.push(name);
    }

    let (i4, items) = if peek_matches(i3, Token::LBrace) {
        let (i_items, idents) = braced(comma_separated1(parse_ident))(i3)?;
        let names = idents.into_iter().map(|Ident { name, .. }| name).collect();
        (i_items, ImportItems::Specific(names))
    } else {
        (i3, ImportItems::All)
    };

    let (i5, _) = semicolon_tag(i4)?;
    Ok((i5, Stmt::ImportStmt { path, items }))
}

pub(crate) struct Parser;

impl Parser {
    pub fn parse_tokens(tokens: Tokens) -> IResult<Tokens, Program> {
        let (rest, program) = parse_program(tokens)?;
        if validate_await_usage(&program).is_err() {
            return Err(Err::Error(Error::new(tokens, ErrorKind::Verify)));
        }

        // This is for checking wether there are more statements after a top level return and if they are valid
        if program.len() > 1 {
            for (i, stmt) in program.iter().enumerate() {
                if i == 0 {
                    continue;
                }
                // Check if current is ExprValueStmt (implicit return, no semicolon)
                // and previous is ReturnStmt
                if matches!(stmt, Stmt::ExprValueStmt(_)) {
                    let prev_stmt = &program[i - 1];
                    if matches!(prev_stmt, Stmt::ReturnStmt(_)) {
                        return Err(Err::Error(Error::new(tokens, ErrorKind::Verify)));
                    }
                }
            }
        }

        Ok((rest, program))
    }
}
