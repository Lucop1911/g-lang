//! Shared parser combinators and utilities used throughout the Pratt parser.
//!
//! This module provides:
//! - Peek helpers (`peek_token`, `peek_matches`) for lookahead without consuming
//! - Delimited combinators (`parens`, `braced`, `bracketed`)
//! - Comma-separated list parsers (`comma_separated0`, `comma_separated1`)
//! - Assignment operator conversion helpers

use crate::ast::ast::Infix;
use crate::lexer::token::{Token, Tokens};
use crate::parser::parser::*;
use nom::bytes::complete::take;
use nom::combinator::{map, opt};
use nom::error::Error;
use nom::sequence::*;
use nom::{branch::alt, multi::many0, IResult};
use std::result::Result::*;

// Peek at the next token without consuming it
#[inline]
pub(crate) fn peek_token(input: Tokens<'_>) -> Option<&'_ Token> {
    input.token.first()
}

// Check if next token matches expected
#[inline]
pub(crate) fn peek_matches(input: Tokens, expected: Token) -> bool {
    peek_token(input).is_some_and(|t| *t == expected)
}

pub(crate) fn peek_assign_op(input: Tokens) -> IResult<Tokens, String> {
    alt((
        map(assign_tag, |_| "=".to_string()),
        map(plus_assign_tag, |_| "+=".to_string()),
        map(minus_assign_tag, |_| "-=".to_string()),
        map(multiply_assign_tag, |_| "*=".to_string()),
        map(divide_assign_tag, |_| "/=".to_string()),
        map(modulo_assign_tag, |_| "%=".to_string()),
    ))(input)
}

// Parse comma-separated items (at least one)
pub(crate) fn comma_separated1<'a, F, O>(
    mut item_parser: F,
) -> impl FnMut(Tokens<'a>) -> IResult<Tokens<'a>, Vec<O>>
where
    F: FnMut(Tokens<'a>) -> IResult<Tokens<'a>, O>,
{
    move |input| {
        let (i1, first) = item_parser(input)?;
        let (i2, rest) = many0(preceded(comma_tag, &mut item_parser))(i1)?;
        let (i3, _) = opt(comma_tag)(i2)?;

        let mut result = Vec::with_capacity(1 + rest.len());
        result.push(first);
        result.extend(rest);
        Ok((i3, result))
    }
}

// Parse comma-separated items (empty allowed)
pub(crate) fn comma_separated0<'a, F, O>(
    item_parser: F,
) -> impl FnMut(Tokens<'a>) -> IResult<Tokens<'a>, Vec<O>>
where
    F: FnMut(Tokens<'a>) -> IResult<Tokens<'a>, O>,
{
    alt((comma_separated1(item_parser), |input| Ok((input, vec![]))))
}

// Wrap parser in braces { }
pub(crate) fn braced<'a, F, O>(parser: F) -> impl FnMut(Tokens<'a>) -> IResult<Tokens<'a>, O>
where
    F: FnMut(Tokens<'a>) -> IResult<Tokens<'a>, O>,
{
    delimited(lbrace_tag, parser, rbrace_tag)
}

// Wrap parser in parentheses ( )
pub(crate) fn parens<'a, F, O>(parser: F) -> impl FnMut(Tokens<'a>) -> IResult<Tokens<'a>, O>
where
    F: FnMut(Tokens<'a>) -> IResult<Tokens<'a>, O>,
{
    delimited(lparen_tag, parser, rparen_tag)
}

// Wrap parser in brackets [ ]
pub(crate) fn bracketed<'a, F, O>(parser: F) -> impl FnMut(Tokens<'a>) -> IResult<Tokens<'a>, O>
where
    F: FnMut(Tokens<'a>) -> IResult<Tokens<'a>, O>,
{
    delimited(lbracket_tag, parser, rbracket_tag)
}

pub(crate) fn aug_assign_to_infix<'a>(input: Tokens<'a>) -> IResult<Tokens<'a>, Infix> {
    let (i1, t1) = take(1usize)(input)?;

    if t1.token.is_empty() {
        return Err(nom::Err::Error(Error::new(
            input,
            nom::error::ErrorKind::Eof,
        )));
    }

    match &t1.token[0] {
        Token::PlusAssign => Ok((i1, Infix::Plus)),
        Token::MinusAssign => Ok((i1, Infix::Minus)),
        Token::MultiplyAssign => Ok((i1, Infix::Multiply)),
        Token::DivideAssign => Ok((i1, Infix::Divide)),
        Token::ModuloAssign => Ok((i1, Infix::Modulo)),
        _ => Err(nom::Err::Error(Error::new(
            input,
            nom::error::ErrorKind::Tag,
        ))),
    }
}

pub(crate) fn assign_op_to_infix(op: &str) -> Infix {
    match op {
        "+=" => Infix::Plus,
        "-=" => Infix::Minus,
        "*=" => Infix::Multiply,
        "/=" => Infix::Divide,
        "%=" => Infix::Modulo,
        _ => unreachable!(),
    }
}
