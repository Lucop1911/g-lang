use std::fmt;

use crate::lexer::token::Location;

#[derive(Debug, Clone, PartialEq)]
pub enum ParserError {
    UnexpectedToken {
        token: String,
        location: Option<Location>,
    },
    ExpectedToken {
        expected: String,
        found: String,
        location: Option<Location>,
    },
    InvalidExpression {
        message: String,
        location: Option<Location>,
    },
    UnexpectedEOF {
        location: Option<Location>,
    },
    AwaitOutsideAsync {
        location: Option<Location>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum RuntimeError {
    TypeMismatch { expected: String, got: String },
    UndefinedVariable(String),
    InvalidOperation(String),
    DivisionByZero,
    ModuloByZero,
    IndexOutOfBounds { index: i64, length: usize },
    WrongNumberOfArguments { min: usize, max: usize, got: usize },
    NotCallable(String),
    NotHashable(String),
    NotIndexable(String),
    EmptyArray,
    InvalidArguments(String),
    UncaughtException(String),
}

impl fmt::Display for ParserError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ParserError::UnexpectedToken { token, location } => {
                if let Some(loc) = location {
                    write!(f, "Unexpected token: {} at {}", token, loc)
                } else {
                    write!(f, "Unexpected token: {}", token)
                }
            }
            ParserError::ExpectedToken {
                expected,
                found,
                location,
            } => {
                if let Some(loc) = location {
                    write!(f, "Expected '{}', found '{}' at {}", expected, found, loc)
                } else {
                    write!(f, "Expected '{}', found '{}'", expected, found)
                }
            }
            ParserError::InvalidExpression { message, location } => {
                if let Some(loc) = location {
                    write!(f, "Invalid expression: {} at {}", message, loc)
                } else {
                    write!(f, "Invalid expression: {}", message)
                }
            }
            ParserError::UnexpectedEOF { location } => {
                if let Some(loc) = location {
                    write!(f, "Unexpected end of file at {}", loc)
                } else {
                    write!(f, "Unexpected end of file")
                }
            }
            ParserError::AwaitOutsideAsync { location } => {
                if let Some(loc) = location {
                    write!(
                        f,
                        "Cannot use 'await' outside of an async function at {}",
                        loc
                    )
                } else {
                    write!(f, "Cannot use 'await' outside of an async function")
                }
            }
        }
    }
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            RuntimeError::TypeMismatch { expected, got } => {
                write!(f, "Type mismatch: expected {}, got {}", expected, got)
            }
            RuntimeError::UndefinedVariable(name) => {
                write!(f, "Undefined variable: '{}'", name)
            }
            RuntimeError::InvalidOperation(op) => write!(f, "Invalid operation: {}", op),
            RuntimeError::DivisionByZero => write!(f, "Division by zero"),
            RuntimeError::ModuloByZero => write!(f, "Modulo by zero"),
            RuntimeError::IndexOutOfBounds { index, length } => {
                write!(
                    f,
                    "Index {} out of bounds for array of length {}",
                    index, length
                )
            }
            RuntimeError::WrongNumberOfArguments { min, max, got } => {
                if min != max {
                    write!(
                        f,
                        "Wrong number of arguments: min {}, max: {} got {}",
                        min, max, got
                    )
                } else {
                    write!(f, "Wrong number of arguments: expected {} got {}", min, got)
                }
            }
            RuntimeError::NotCallable(s) => write!(f, "{} is not callable", s),
            RuntimeError::NotHashable(s) => write!(f, "{} is not hashable", s),
            RuntimeError::NotIndexable(s) => write!(f, "{} is not indexable", s),
            RuntimeError::EmptyArray => write!(f, "Cannot perform operation on empty array"),
            RuntimeError::InvalidArguments(s) => write!(f, "Invalid arguments: {}", s),
            RuntimeError::UncaughtException(s) => write!(f, "Uncaught exception: {}", s),
        }
    }
}

impl std::error::Error for ParserError {}
impl std::error::Error for RuntimeError {}
