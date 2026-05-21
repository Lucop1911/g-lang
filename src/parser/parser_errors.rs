//! Diagnostic-quality parser error reporting.
//!
//! Translates raw `nom` parse errors into human-readable [`ParserError`] messages.
//! Rather than reporting a generic "unexpected token", this module performs contextual
//! analysis of the remaining token stream to produce actionable diagnostics such as:
//! - "Expected ';', found '}'"
//! - "Unexpected token: identifier 'foo'"
//! - "Expected '(' after 'if'"
//!
//! # Error Resolution Pipeline
//!
//! 1. [`convert_nom_error`] — entry point; classifies the nom error kind
//! 2. [`is_incomplete_statement`] — detects truncated statement keywords
//! 3. [`detect_incomplete_error`] — per-statement heuristic analysis
//! 4. [`infer_context_from_tokens`] — fallback with bracket/operator analysis
//! 5. [`create_contextual_error`] — maps context strings to specific messages

use crate::lexer::token::{Location, Spanned, Token, Tokens};
use crate::vm::runtime::runtime_errors::ParserError;
use nom::Err;
use nom::error::{Error, ErrorKind};

/// Converts a `nom` parse error into a user-friendly [`ParserError`] with location info.
///
/// The `context` parameter is a string tag identifying which parser combinator
/// failed (e.g. `"let_stmt"`, `"if_expr_lparen"`). When empty, the function
/// falls back to token-pattern inference. The `spanned_tokens` parameter provides access
/// to source location information for accurate error reporting.
pub(crate) fn convert_nom_error<'a>(
    err: &Err<Error<Tokens<'a>>>,
    context: &str,
    spanned_tokens: &[Spanned<Token>],
    error_index: usize,
) -> ParserError {
    let location = spanned_tokens.get(error_index.saturating_sub(1)).map(|s| s.span.end);

    match err {
        Err::Error(e) | Err::Failure(e) => {
            let tokens = &e.input;

            if tokens.token.is_empty() {
                return ParserError::UnexpectedEOF {
                    location,
                };
            }

            let current_token = &tokens.token[0];
            let token_description = describe_token(current_token);

            let looks_incomplete = is_incomplete_statement(tokens);

            if looks_incomplete {
                return detect_incomplete_error(tokens, location);
            }

            if current_token == &Token::EOF && e.code == ErrorKind::Verify {
                return ParserError::ExpectedToken {
                    expected: "';' after statement".to_string(),
                    found: "end of file".to_string(),
                    location,
                };
            }

            match e.code {
                ErrorKind::Tag | ErrorKind::Verify => {
                    create_contextual_error(context, current_token, &token_description, tokens, location)
                }
                ErrorKind::Many0 | ErrorKind::Many1 => ParserError::InvalidExpression {
                    message: format!("unexpected token: {}", token_description),
                    location,
                },
                _ => ParserError::UnexpectedToken {
                    token: token_description,
                    location,
                },
            }
        }
        Err::Incomplete(_) => ParserError::UnexpectedEOF {
            location,
        },
    }
}

fn is_incomplete_statement(tokens: &Tokens) -> bool {
    if tokens.token.is_empty() {
        return false;
    }

    matches!(
        tokens.token[0],
        Token::Let
            | Token::For
            | Token::While
            | Token::If
            | Token::Return
            | Token::Function
            | Token::Struct
    )
}

fn detect_incomplete_error(tokens: &Tokens, location: Option<Location>) -> ParserError {
    if tokens.token.is_empty() {
        return ParserError::UnexpectedEOF {
            location,
        };
    }

    let statement_type = &tokens.token[0];

    if let Some(return_pos) = find_token_position(tokens, &Token::Return)
        && (return_pos == 0
            || (return_pos > 0 && tokens.token.get(return_pos - 1) == Some(&Token::LBrace))
            && return_pos + 1 < tokens.token.len())
        {
            let after_return = &tokens.token[return_pos + 1];
            return match after_return {
                Token::EOF => ParserError::ExpectedToken {
                    expected: "expression after 'return'".to_string(),
                    found: "end of input".to_string(),
                    location,
                },
                Token::SemiColon => ParserError::ExpectedToken {
                    expected: "expression before ';'".to_string(),
                    found: "';'".to_string(),
                    location,
                },
                Token::RParen | Token::RBracket | Token::RBrace => ParserError::ExpectedToken {
                    expected: "expression after 'return'".to_string(),
                    found: describe_token(after_return),
                    location,
                },
                _ => ParserError::ExpectedToken {
                    expected: "expression after 'return'".to_string(),
                    found: describe_token(after_return),
                    location,
                },
        }
    }

    let mut has_eof = false;
    for token in tokens.token.iter() {
        if token == &Token::EOF {
            has_eof = true;
            break;
        }
    }

    if has_eof {
        match statement_type {
            Token::Let => {
                if tokens.token.len() > 1 {
                    match &tokens.token[1] {
                        Token::Assign | Token::Colon => {
                            return ParserError::ExpectedToken {
                                expected: "variable name after 'let'".to_string(),
                                found: describe_token(&tokens.token[1]),
                                location,
                            };
                        }
                        Token::Ident(_) => {
                            if let Some(pos) = find_token_position(tokens, &Token::Assign) {
                                if let Some(colon_pos) = find_token_position(tokens, &Token::Colon)
                                    && colon_pos > pos
                                {
                                    return ParserError::ExpectedToken {
                                        expected: "';' after statement".to_string(),
                                        found: "':'".to_string(),
                                        location,
                                    };
                                }
                                if pos + 2 >= tokens.token.len()
                                    || tokens.token[pos + 1] == Token::EOF
                                {
                                    return ParserError::ExpectedToken {
                                        expected: "expression after '='".to_string(),
                                        found: "end of input".to_string(),
                                        location,
                                    };
                                } else {
                                    return ParserError::ExpectedToken {
                                        expected: "';' after statement".to_string(),
                                        found: "end of input".to_string(),
                                        location,
                                    };
                                }
                            } else {
                                return ParserError::ExpectedToken {
                                    expected: "'=' after variable name".to_string(),
                                    found: describe_token(
                                        tokens.token.get(2).unwrap_or(&Token::EOF),
                                    ),
                                    location,
                                };
                            }
                        }
                        _ => {
                            return ParserError::ExpectedToken {
                                expected: "variable name after 'let'".to_string(),
                                found: describe_token(&tokens.token[1]),
                                location,
                            };
                        }
                    }
                } else {
                    return ParserError::ExpectedToken {
                        expected: "variable name after 'let'".to_string(),
                        found: "end of input".to_string(),
                        location,
                    };
                }
            }
            Token::For => {
                if !has_matching_paren(tokens) {
                    return ParserError::ExpectedToken {
                        expected: "')' to close for loop condition".to_string(),
                        found: "end of input".to_string(),
                        location,
                    };
                }
                if !has_matching_brace(tokens) {
                    return ParserError::ExpectedToken {
                        expected: "'{' for loop body or '}' to close loop body".to_string(),
                        found: "end of input".to_string(),
                        location,
                    };
                }
                return ParserError::InvalidExpression {
                    message: "incomplete for loop".to_string(),
                    location,
                };
            }
            Token::While => {
                if !has_matching_paren(tokens) {
                    return ParserError::ExpectedToken {
                        expected: "')' to close while condition".to_string(),
                        found: "end of input".to_string(),
                        location,
                    };
                }
                if !has_matching_brace(tokens) {
                    return ParserError::ExpectedToken {
                        expected: "'{' for loop body or '}' to close loop body".to_string(),
                        found: "end of input".to_string(),
                        location,
                    };
                }
                return ParserError::InvalidExpression {
                    message: "incomplete while loop".to_string(),
                    location,
                };
            }
            Token::If => {
                if !has_matching_paren(tokens) {
                    return ParserError::ExpectedToken {
                        expected: "')' to close if condition".to_string(),
                        found: "end of input".to_string(),
                        location,
                    };
                }
                if !has_matching_brace(tokens) {
                    return ParserError::ExpectedToken {
                        expected: "'}' to close if body".to_string(),
                        found: "end of input".to_string(),
                        location,
                    };
                }
                return ParserError::InvalidExpression {
                    message: "incomplete if statement".to_string(),
                    location,
                };
            }
            Token::Struct => {
                if tokens.token.len() > 1 {
                    match &tokens.token[1] {
                        Token::Ident(_) => {
                            if !has_matching_brace(tokens) {
                                return ParserError::ExpectedToken {
                                    expected: "'}' to close struct definition".to_string(),
                                    found: "end of input".to_string(),
                                    location,
                                };
                            }
                            return ParserError::InvalidExpression {
                                message: "incomplete struct definition".to_string(),
                                location,
                            };
                        }
                        _ => {
                            return ParserError::ExpectedToken {
                                expected: "struct name after 'struct'".to_string(),
                                found: describe_token(&tokens.token[1]),
                                location,
                            };
                        }
                    }
                } else {
                    return ParserError::ExpectedToken {
                        expected: "struct name after 'struct'".to_string(),
                        found: "end of input".to_string(),
                        location,
                    };
                }
            }
            Token::Function => {
                if tokens.token.len() > 1 {
                    match &tokens.token[1] {
                        Token::Ident(_) => {
                            if !has_matching_paren(tokens) {
                                return ParserError::ExpectedToken {
                                    expected: "')' to close function parameters".to_string(),
                                    found: "end of input".to_string(),
                                    location,
                                };
                            }
                            if !has_matching_brace(tokens) {
                                return ParserError::ExpectedToken {
                                    expected: "'{' for function body".to_string(),
                                    found: "end of input".to_string(),
                                    location,
                                };
                            }
                            return ParserError::InvalidExpression {
                                message: "incomplete function definition".to_string(),
                                location,
                            };
                        }
                        _ => {
                            return ParserError::ExpectedToken {
                                expected: "function name after 'fn'".to_string(),
                                found: describe_token(&tokens.token[1]),
                                location,
                            };
                        }
                    }
                } else {
                    return ParserError::ExpectedToken {
                        expected: "function name after 'fn'".to_string(),
                        found: "end of input".to_string(),
                        location,
                    };
                }
            }
            Token::Return => {
                if tokens.token.len() > 1 {
                    match &tokens.token[1] {
                        Token::EOF => {
                            return ParserError::ExpectedToken {
                                expected: "expression after 'return'".to_string(),
                                found: "end of input".to_string(),
                                location,
                            };
                        }
                        Token::SemiColon => {
                            return ParserError::ExpectedToken {
                                expected: "expression before ';'".to_string(),
                                found: "';'".to_string(),
                                location,
                            };
                        }
                        Token::RParen | Token::RBracket | Token::RBrace => {
                            return ParserError::ExpectedToken {
                                expected: "expression after 'return'".to_string(),
                                found: describe_token(&tokens.token[1]),
                                location,
                            };
                        }
                        _ => {
                            return ParserError::ExpectedToken {
                                expected: "expression after 'return'".to_string(),
                                found: describe_token(&tokens.token[1]),
                                location,
                            };
                        }
                    }
                } else {
                    return ParserError::ExpectedToken {
                        expected: "expression after 'return'".to_string(),
                        found: "end of input".to_string(),
                        location,
                    };
                }
            }
            _ => {}
        }
    }

    ParserError::UnexpectedToken {
        token: describe_token(statement_type),
        location,
    }
}

fn find_token_position(tokens: &Tokens, target: &Token) -> Option<usize> {
    tokens.token.iter().position(|t| t == target)
}

fn has_matching_paren(tokens: &Tokens) -> bool {
    let mut depth = 0;
    for token in tokens.token.iter() {
        match token {
            Token::LParen => depth += 1,
            Token::RParen => depth -= 1,
            Token::EOF => break,
            _ => {}
        }
    }
    depth == 0
}

fn has_matching_brace(tokens: &Tokens) -> bool {
    let mut depth = 0;
    for token in tokens.token.iter() {
        match token {
            Token::LBrace => depth += 1,
            Token::RBrace => depth -= 1,
            Token::EOF => break,
            _ => {}
        }
    }
    depth == 0
}

fn count_unmatched(tokens: &Tokens, open: Token, close: Token) -> i32 {
    let mut depth = 0;
    for token in tokens.token.iter() {
        if *token == open {
            depth += 1;
        } else if *token == close {
            depth -= 1;
        }
    }
    depth
}

fn infer_context_from_tokens(tokens: &Tokens, token_desc: &str, location: Option<Location>) -> ParserError {
    if tokens.token.is_empty() {
        return ParserError::UnexpectedToken {
            token: token_desc.to_string(),
            location,
        };
    }

    let last_idx = tokens.token.len() - 1;
    if tokens.token[last_idx] == Token::EOF && last_idx > 0 {
        let before_eof = &tokens.token[last_idx - 1];
        if matches!(
            before_eof,
            Token::Plus
                | Token::Minus
                | Token::Multiply
                | Token::Divide
                | Token::Modulo
                | Token::GreaterThan
                | Token::LessThan
                | Token::GreaterThanEqual
                | Token::LessThanEqual
                | Token::Equal
                | Token::NotEqual
                | Token::And
                | Token::Or
                | Token::Assign
        ) {
            return ParserError::ExpectedToken {
                expected: "expression after operator".to_string(),
                found: "end of file".to_string(),
                location,
            };
        }
    }

    let paren_depth = count_unmatched(tokens, Token::LParen, Token::RParen);
    let bracket_depth = count_unmatched(tokens, Token::LBracket, Token::RBracket);
    let brace_depth = count_unmatched(tokens, Token::LBrace, Token::RBrace);

    if paren_depth > 0 {
        return ParserError::ExpectedToken {
            expected: "')' to close parenthesis".to_string(),
            found: "end of file".to_string(),
            location,
        };
    }
    if bracket_depth > 0 {
        return ParserError::ExpectedToken {
            expected: "']' to close array".to_string(),
            found: "end of file".to_string(),
            location,
        };
    }
    if brace_depth > 0 {
        return ParserError::ExpectedToken {
            expected: "'}' to close block".to_string(),
            found: "end of file".to_string(),
            location,
        };
    }

    if tokens.token.len() < 2 {
        return ParserError::UnexpectedToken {
            token: token_desc.to_string(),
            location,
        };
    }

    let curr = &tokens.token[0];
    let next = &tokens.token[1];

    match (curr, next) {
        (Token::Let, Token::Assign) => ParserError::ExpectedToken {
            expected: "identifier after 'let'".to_string(),
            found: "'='".to_string(),
            location,
        },
        (Token::Let, Token::Colon) => ParserError::ExpectedToken {
            expected: "identifier after 'let'".to_string(),
            found: "':'".to_string(),
            location,
        },
        (Token::Let, Token::LParen) => ParserError::ExpectedToken {
            expected: "identifier after 'let'".to_string(),
            found: "'('".to_string(),
            location,
        },
        (Token::Let, t) if !matches!(t, Token::Ident(_)) => ParserError::ExpectedToken {
            expected: "identifier after 'let'".to_string(),
            found: describe_token(t),
            location,
        },
        (Token::If, Token::Assign | Token::Colon | Token::LBrace | Token::SemiColon) => {
            ParserError::ExpectedToken {
                expected: "'(' after 'if'".to_string(),
                found: describe_token(next),
                location,
            }
        }
        (Token::If, t) if !matches!(t, Token::LParen) => ParserError::ExpectedToken {
            expected: "'(' after 'if'".to_string(),
            found: describe_token(t),
            location,
        },
        (Token::While, t) if !matches!(t, Token::LParen) => ParserError::ExpectedToken {
            expected: "'(' after 'while'".to_string(),
            found: describe_token(t),
            location,
        },
        (Token::For, t) if !matches!(t, Token::LParen) => ParserError::ExpectedToken {
            expected: "'(' after 'for'".to_string(),
            found: describe_token(t),
            location,
        },
        (Token::Function, t) if !matches!(t, Token::Ident(_)) => ParserError::ExpectedToken {
            expected: "function name after 'fn'".to_string(),
            found: describe_token(t),
            location,
        },
        (Token::Struct, t) if !matches!(t, Token::Ident(_)) => ParserError::ExpectedToken {
            expected: "struct name after 'struct'".to_string(),
            found: describe_token(t),
            location,
        },
        (Token::Return, Token::LParen | Token::LBrace | Token::If | Token::For | Token::While) => {
            ParserError::ExpectedToken {
                expected: "expression or ';' after 'return'".to_string(),
                found: describe_token(next),
                location,
            }
        }
        (Token::Assign, Token::Assign | Token::Equal) => ParserError::ExpectedToken {
            expected: "expression after '='".to_string(),
            found: describe_token(next),
            location,
        },
        (Token::LParen, Token::RParen) => ParserError::InvalidExpression {
            message: "empty parentheses".to_string(),
            location,
        },
        (Token::LBracket, Token::RBracket) => ParserError::InvalidExpression {
            message: "empty array literal".to_string(),
            location,
        },
        (Token::LBrace, Token::RBrace) => ParserError::InvalidExpression {
            message: "empty block".to_string(),
            location,
        },
        (Token::Comma, Token::RParen | Token::RBracket | Token::RBrace) => {
            ParserError::ExpectedToken {
                expected: "expression after ','".to_string(),
                found: describe_token(next),
                location,
            }
        }
        (Token::Colon, Token::Colon) => ParserError::ExpectedToken {
            expected: "type or expression after ':'".to_string(),
            found: "'::'".to_string(),
            location,
        },
        (Token::Dot, Token::Dot) => ParserError::ExpectedToken {
            expected: "identifier after '.'".to_string(),
            found: "'.'".to_string(),
            location,
        },
        (Token::Else, Token::Assign | Token::Colon | Token::SemiColon | Token::LParen) => {
            ParserError::ExpectedToken {
                expected: "'{' after 'else'".to_string(),
                found: describe_token(next),
                location,
            }
        }
        _ => ParserError::UnexpectedToken {
            token: token_desc.to_string(),
            location
        },
    }
}

fn create_contextual_error(
    context: &str,
    _: &Token,
    token_desc: &str,
    tokens: &Tokens,
    location: Option<Location>,
) -> ParserError {
    if context.is_empty() {
        return infer_context_from_tokens(tokens, token_desc, location);
    }

    match context {
        "let_stmt" => ParserError::ExpectedToken {
            expected: "identifier after 'let'".to_string(),
            found: token_desc.to_string(),
            location,
        },
        "assign_stmt" => ParserError::ExpectedToken {
            expected: "'=' for assignment".to_string(),
            found: token_desc.to_string(),
            location,
        },
        "if_expr_lparen" => ParserError::ExpectedToken {
            expected: "'(' after 'if'".to_string(),
            found: token_desc.to_string(),
            location,
        },
        "if_expr_rparen" => ParserError::ExpectedToken {
            expected: "')' after condition".to_string(),
            found: token_desc.to_string(),
            location,
        },
        "while_lparen" => ParserError::ExpectedToken {
            expected: "'(' after 'while'".to_string(),
            found: token_desc.to_string(),
            location,
        },
        "while_rparen" => ParserError::ExpectedToken {
            expected: "')' after while condition".to_string(),
            found: token_desc.to_string(),
            location,
        },
        "for_lparen" => ParserError::ExpectedToken {
            expected: "'(' after 'for'".to_string(),
            found: token_desc.to_string(),
            location,
        },
        "for_rparen" => ParserError::ExpectedToken {
            expected: "')' after for declaration".to_string(),
            found: token_desc.to_string(),
            location,
        },
        "block_lbrace" => ParserError::ExpectedToken {
            expected: "'{'".to_string(),
            found: token_desc.to_string(),
            location,
        },
        "block_rbrace" => ParserError::ExpectedToken {
            expected: "'}'".to_string(),
            found: token_desc.to_string(),
            location,
        },
        "semicolon" => ParserError::ExpectedToken {
            expected: "';' after statement".to_string(),
            found: token_desc.to_string(),
            location,
        },
        "fn_params" => ParserError::ExpectedToken {
            expected: "parameter list".to_string(),
            found: token_desc.to_string(),
            location,
        },
        "struct_name" => ParserError::ExpectedToken {
            expected: "struct name".to_string(),
            found: token_desc.to_string(),
            location,
        },
        "array_close" => ParserError::ExpectedToken {
            expected: "']' to close array".to_string(),
            found: token_desc.to_string(),
            location,
        },
        "call_close" => ParserError::ExpectedToken {
            expected: "')' to close function call".to_string(),
            found: token_desc.to_string(),
            location,
        },
        "hash_close" => ParserError::ExpectedToken {
            expected: "'}' to close hash".to_string(),
            found: token_desc.to_string(),
            location,
        },
        "expression" => ParserError::InvalidExpression {
            message: format!("unexpected token: {}", token_desc),
            location,
        },
        _ => ParserError::UnexpectedToken {
            token: token_desc.to_string(),
            location,
        },
    }
}

pub(crate) fn describe_token(token: &Token) -> String {
    match token {
        Token::Illegal => "illegal token".to_string(),
        Token::EOF => "end of file".to_string(),
        Token::Ident(name) => format!("identifier '{}'", name),
        Token::StringLiteral(s) => {
            if s.len() > 20 {
                format!("string \"{}...\"", &s[..20])
            } else {
                format!("string \"{}\"", s)
            }
        }
        Token::IntLiteral(n) => format!("integer {}", n),
        Token::BigIntLiteral(n) => format!("big integer {}", n),
        Token::FloatLiteral(f) => format!("float {}", f),
        Token::BoolLiteral(b) => format!("boolean {}", b),
        Token::NullLiteral => "null".to_string(),
        Token::Assign => "'='".to_string(),
        Token::If => "'if'".to_string(),
        Token::Else => "'else'".to_string(),
        Token::PlusAssign => "'+='".to_string(),
        Token::MinusAssign => "'-='".to_string(),
        Token::MultiplyAssign => "'*='".to_string(),
        Token::DivideAssign => "'/='".to_string(),
        Token::ModuloAssign => "'%='".to_string(),
        Token::Plus => "'+'".to_string(),
        Token::Minus => "'-'".to_string(),
        Token::Divide => "'/'".to_string(),
        Token::Multiply => "'*'".to_string(),
        Token::Modulo => "'%'".to_string(),
        Token::Equal => "'=='".to_string(),
        Token::NotEqual => "'!='".to_string(),
        Token::GreaterThanEqual => "'>='".to_string(),
        Token::LessThanEqual => "'<='".to_string(),
        Token::GreaterThan => "'>'".to_string(),
        Token::LessThan => "'<'".to_string(),
        Token::Function => "'fn'".to_string(),
        Token::Let => "'let'".to_string(),
        Token::Return => "'return'".to_string(),
        Token::Struct => "'struct'".to_string(),
        Token::This => "'this'".to_string(),
        Token::Import => "'import'".to_string(),
        Token::Comma => "','".to_string(),
        Token::Colon => "':'".to_string(),
        Token::SemiColon => "';'".to_string(),
        Token::LParen => "'('".to_string(),
        Token::RParen => "')'".to_string(),
        Token::LBrace => "'{'".to_string(),
        Token::RBrace => "'}'".to_string(),
        Token::LBracket => "'['".to_string(),
        Token::RBracket => "']'".to_string(),
        Token::And => "'&&'".to_string(),
        Token::Or => "'||'".to_string(),
        Token::Not => "'!'".to_string(),
        Token::Dot => "'.'".to_string(),
        Token::DoubleColon => "'::'".to_string(),
        Token::While => "'while'".to_string(),
        Token::For => "'for'".to_string(),
        Token::In => "'in'".to_string(),
        Token::Break => "'break'".to_string(),
        Token::Continue => "'continue'".to_string(),
        Token::Try => "'try'".to_string(),
        Token::Catch => "'catch'".to_string(),
        Token::Finally => "'finally'".to_string(),
        Token::Throw => "'throw'".to_string(),
        Token::Async => "'async'".to_string(),
        Token::Await => "'await'".to_string(),
        Token::Match => "'match'".to_string(),
        Token::FatArrow => "'=>'".to_string(),
    }
}

pub(crate) fn show_error_context(tokens: &Tokens, num_context_tokens: usize) -> String {
    if tokens.token.is_empty() {
        return "Near: end of file".to_string();
    }

    let mut result = String::new();
    result.push_str("Near: ");

    let error_pos: usize = 0;
    let context_before = error_pos.saturating_sub(num_context_tokens);

    for i in context_before..error_pos {
        result.push_str(&describe_token(&tokens.token[i]));
        result.push(' ');
    }

    result.push_str(">>> ");
    result.push_str(&describe_token(&tokens.token[error_pos]));
    result.push_str(" <<<");

    let max_show = std::cmp::min(error_pos + num_context_tokens + 1, tokens.token.len());
    for i in (error_pos + 1)..max_show {
        result.push(' ');
        result.push_str(&describe_token(&tokens.token[i]));
    }

    result
}
