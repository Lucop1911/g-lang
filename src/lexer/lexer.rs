//! Lexer for G-lang source code.
//!
//! Transforms raw UTF-8 bytes into a stream of [`Spanned<Token>`] values,
//! each carrying its source location for accurate error reporting. Skips
//! whitespace and `//` line comments.
//!
//! # Lexer Architecture
//!
//! The lexer is built around a state machine (`LexerState`) that tracks
//! position (byte offset, line, column) as it consumes input. Individual
//! token parsers (`parse_string`, `parse_operator`, etc.) attempt to match
//! their respective token type and return `None` if the input doesn't match.
//!
//! # Error Handling
//!
//! Unterminated strings and invalid characters are reported via the
//! [`LexerError`] enum, which includes source locations for each error.

use num_bigint::BigInt;
use std::str::FromStr;

use crate::lexer::token::{Location, Span, Spanned, Token};

struct LexerState<'a> {
    input: &'a [u8],
    pos: usize,
    line: usize,
    column: usize,
}

impl<'a> LexerState<'a> {
    fn new(input: &'a [u8]) -> Self {
        Self {
            input,
            pos: 0,
            line: 1,
            column: 1,
        }
    }

    fn current(&self) -> &'a [u8] {
        &self.input[self.pos..]
    }

    fn peek_char(&self) -> Option<char> {
        std::str::from_utf8(&self.input[self.pos..])
            .ok()
            .and_then(|s| s.chars().next())
    }

    fn peek_bytes(&self, n: usize) -> Option<&'a [u8]> {
        if self.pos + n <= self.input.len() {
            Some(&self.input[self.pos..self.pos + n])
        } else {
            None
        }
    }

    fn advance(&mut self, n: usize) {
        for _ in 0..n {
            if let Some(c) = std::str::from_utf8(&self.input[self.pos..])
                .ok()
                .and_then(|s| s.chars().next())
            {
                self.pos += c.len_utf8();
                if c == '\n' {
                    self.line += 1;
                    self.column = 1;
                } else {
                    self.column += 1;
                }
            }
        }
    }

    fn advance_char(&mut self) {
        if let Some(c) = std::str::from_utf8(&self.input[self.pos..])
            .ok()
            .and_then(|s| s.chars().next())
        {
            self.pos += c.len_utf8();
            if c == '\n' {
                self.line += 1;
                self.column = 1;
            } else {
                self.column += 1;
            }
        }
    }

    fn skip_whitespace_and_comments(&mut self) {
        loop {
            let remaining = self.current();
            if remaining.is_empty() {
                return;
            }

            if remaining.starts_with(b"//") {
                let after_comment = &remaining[2..];
                let mut found_newline = false;
                for (i, &byte) in after_comment.iter().enumerate() {
                    if byte == b'\n' {
                        self.advance(i + 3);
                        found_newline = true;
                        break;
                    }
                }
                if !found_newline {
                    self.pos = self.input.len();
                    return;
                }
            } else if remaining[0].is_ascii_whitespace() {
                self.advance_char();
            } else {
                return;
            }
        }
    }

    fn location(&self) -> Location {
        Location::new(self.line, self.column)
    }

    fn span(&self, len: usize) -> Span {
        let start = self.location();
        let mut end_column = start.column + len;
        for i in 0..len {
            if self.pos + i < self.input.len()
                && let Ok(c) = std::str::from_utf8(&[self.input[self.pos + i]])
                    && c == "\n" {
                        end_column = 1;
                    }
        }
        Span::new(start, Location::new(start.line, end_column))
    }
}

fn parse_string(state: &mut LexerState) -> Option<Result<Spanned<Token>, LexerError>> {
    let quote = state.peek_bytes(1)?;
    if quote != b"\"" && quote != b"'" {
        return None;
    }

    let quote_byte = state.input[state.pos];
    state.advance(1);

    let start = state.location();
    let mut contents = String::new();

    while let Some(c) = state.peek_char() {
        if c as u8 == quote_byte {
            let end = state.location();
            state.advance(1);
            return Some(Ok(Spanned::new(
                Token::StringLiteral(contents),
                Span::new(start, end),
            )));
        }

        if c == '\\' {
            state.advance(1);
            if let Some(escaped) = state.peek_char() {
                let ch = match escaped {
                    'n' => '\n',
                    'r' => '\r',
                    't' => '\t',
                    '"' => '"',
                    '\'' => '\'',
                    '\\' => '\\',
                    _ => {
                        contents.push('\\');
                        escaped
                    }
                };
                contents.push(ch);
                state.advance(1);
            }
        } else if c == '\n' || c == '\r' {
            return Some(Err(LexerError::UnterminatedString(start)));
        } else {
            contents.push(c);
            state.advance_char();
        }
    }

    Some(Err(LexerError::UnterminatedString(start)))
}

fn parse_operator(state: &mut LexerState) -> Option<Spanned<Token>> {
    let remaining = state.current();
    if remaining.is_empty() {
        return None;
    }

    let (token, len) = if remaining.starts_with(b"+=") {
        (Token::PlusAssign, 2)
    } else if remaining.starts_with(b"-=") {
        (Token::MinusAssign, 2)
    } else if remaining.starts_with(b"*=") {
        (Token::MultiplyAssign, 2)
    } else if remaining.starts_with(b"/=") {
        (Token::DivideAssign, 2)
    } else if remaining.starts_with(b"%=") {
        (Token::ModuloAssign, 2)
    } else if remaining.starts_with(b"==") {
        (Token::Equal, 2)
    } else if remaining.starts_with(b"!=") {
        (Token::NotEqual, 2)
    } else if remaining.starts_with(b">=") {
        (Token::GreaterThanEqual, 2)
    } else if remaining.starts_with(b"<=") {
        (Token::LessThanEqual, 2)
    } else if remaining.starts_with(b"&&") {
        (Token::And, 2)
    } else if remaining.starts_with(b"||") {
        (Token::Or, 2)
    } else if remaining.starts_with(b"::") {
        (Token::DoubleColon, 2)
    } else if remaining.starts_with(b"=>") {
        (Token::FatArrow, 2)
    } else if remaining.starts_with(b"..") {
        (Token::Dot, 2)
    } else {
        match remaining[0] {
            b'+' => (Token::Plus, 1),
            b'-' => (Token::Minus, 1),
            b'*' => (Token::Multiply, 1),
            b'/' => (Token::Divide, 1),
            b'%' => (Token::Modulo, 1),
            b'!' => (Token::Not, 1),
            b'>' => (Token::GreaterThan, 1),
            b'<' => (Token::LessThan, 1),
            b'=' => (Token::Assign, 1),
            _ => return None,
        }
    };

    let span = state.span(len);
    state.advance(len);
    Some(Spanned::new(token, span))
}

fn parse_punctuation(state: &mut LexerState) -> Option<Spanned<Token>> {
    let remaining = state.current();
    if remaining.is_empty() {
        return None;
    }

    let (token, len) = match remaining[0] {
        b',' => (Token::Comma, 1),
        b';' => (Token::SemiColon, 1),
        b':' => (Token::Colon, 1),
        b'(' => (Token::LParen, 1),
        b')' => (Token::RParen, 1),
        b'{' => (Token::LBrace, 1),
        b'}' => (Token::RBrace, 1),
        b'[' => (Token::LBracket, 1),
        b']' => (Token::RBracket, 1),
        b'.' => (Token::Dot, 1),
        _ => return None,
    };

    let span = state.span(len);
    state.advance(len);
    Some(Spanned::new(token, span))
}

fn parse_ident_or_keyword(state: &mut LexerState) -> Option<Spanned<Token>> {
    let remaining = state.current();
    if remaining.is_empty() {
        return None;
    }

    let first_char = remaining[0];
    if !first_char.is_ascii_alphabetic() && first_char != b'_' {
        return None;
    }

    let start = state.location();
    let start_pos = state.pos;

    state.advance_char();
    while let Some(c) = state.peek_char() {
        if c.is_ascii_alphanumeric() || c == '_' {
            state.advance_char();
        } else {
            break;
        }
    }

    let ident = std::str::from_utf8(&state.input[start_pos..state.pos]).ok()?;
    let token = match ident {
        "let" => Token::Let,
        "fn" => Token::Function,
        "if" => Token::If,
        "else" => Token::Else,
        "return" => Token::Return,
        "struct" => Token::Struct,
        "enum" => Token::Enum,
        "this" => Token::This,
        "import" => Token::Import,
        "true" => Token::BoolLiteral(true),
        "false" => Token::BoolLiteral(false),
        "null" => Token::NullLiteral,
        "while" => Token::While,
        "for" => Token::For,
        "in" => Token::In,
        "break" => Token::Break,
        "continue" => Token::Continue,
        "try" => Token::Try,
        "catch" => Token::Catch,
        "finally" => Token::Finally,
        "throw" => Token::Throw,
        "match" => Token::Match,
        "async" => Token::Async,
        "await" => Token::Await,
        _ => Token::Ident(ident.to_string()),
    };

    let end = state.location();
    Some(Spanned::new(token, Span::new(start, end)))
}

fn parse_number(state: &mut LexerState) -> Option<Spanned<Token>> {
    let remaining = state.current();
    if remaining.is_empty() || !remaining[0].is_ascii_digit() {
        return None;
    }

    let start = state.location();
    let start_pos = state.pos;
    let mut pos = state.pos;
    let mut has_dot = false;

    while pos < state.input.len() {
        let c = state.input[pos];
        if c.is_ascii_digit() {
            pos += 1;
        } else if c == b'.' && !has_dot {
            if pos + 1 < state.input.len() && state.input[pos + 1].is_ascii_digit() {
                has_dot = true;
                pos += 2;
                break;
            } else {
                break;
            }
        } else {
            break;
        }
    }

    if has_dot {
        while pos < state.input.len() && state.input[pos].is_ascii_digit() {
            pos += 1;
        }
    }

    let num_str = std::str::from_utf8(&state.input[start_pos..pos]).ok()?;
    state.pos = pos;
    let end = state.location();

    if has_dot {
        match f64::from_str(num_str) {
            Ok(f) => Some(Spanned::new(Token::FloatLiteral(f), Span::new(start, end))),
            Err(_) => Some(Spanned::new(Token::Illegal, Span::new(start, end))),
        }
    } else {
        match i64::from_str(num_str) {
            Ok(n) => Some(Spanned::new(Token::IntLiteral(n), Span::new(start, end))),
            Err(_) => match BigInt::parse_bytes(num_str.as_bytes(), 10) {
                Some(big) => Some(Spanned::new(
                    Token::BigIntLiteral(big),
                    Span::new(start, end),
                )),
                None => Some(Spanned::new(Token::Illegal, Span::new(start, end))),
            },
        }
    }
}

fn lex_token(state: &mut LexerState) -> Option<Result<Spanned<Token>, LexerError>> {
    state.skip_whitespace_and_comments();

    if state.current().is_empty() {
        return None;
    }

    parse_string(state)
        .or_else(|| Some(Ok(parse_operator(state)?)))
        .or_else(|| Some(Ok(parse_punctuation(state)?)))
        .or_else(|| Some(Ok(parse_ident_or_keyword(state)?)))
        .or_else(|| Some(Ok(parse_number(state)?)))
}

pub struct Lexer;

impl Lexer {
    pub fn lex_tokens(bytes: &[u8]) -> Result<Vec<Spanned<Token>>, LexerError> {
        let mut state = LexerState::new(bytes);
        let mut tokens = Vec::new();

        while let Some(result) = lex_token(&mut state) {
            tokens.push(result?);
        }

        if !state.current().is_empty() {
            let c = std::str::from_utf8(state.current())
                .ok()
                .and_then(|s| s.chars().next())
                .unwrap_or('?');
            return Err(LexerError::UnexpectedCharacter(c, state.location()));
        }

        let eof_location = Location::new(state.line, state.column);
        tokens.push(Spanned::new(
            Token::EOF,
            Span::new(eof_location, eof_location),
        ));

        Ok(tokens)
    }

}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LexerError {
    UnexpectedCharacter(char, Location),
    UnterminatedString(Location),
}

impl std::fmt::Display for LexerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LexerError::UnexpectedCharacter(c, loc) => {
                write!(f, "Unexpected character: '{}' at {}", c, loc)
            }
            LexerError::UnterminatedString(loc) => {
                write!(f, "Unterminated string literal at {}", loc)
            }
        }
    }
}

impl std::error::Error for LexerError {}
