use nom::{InputIter, InputLength, InputTake, Needed, Slice};
use num_bigint::BigInt;
use std::iter::Enumerate;
use std::ops::{Range, RangeFrom, RangeFull, RangeTo};

/// Lexical tokens produced by the [`Lexer`](super::lexer::Lexer).
///
/// Keywords, operators, literals, and punctuation are all represented here.
/// Each token may carry source location information via the [`Spanned`] wrapper.
///
/// # Token Types
///
/// - **Literals**: `IntLiteral`, `FloatLiteral`, `StringLiteral`, `BoolLiteral`, `NullLiteral`
/// - **Identifiers**: `Ident` for variable and function names
/// - **Keywords**: `Let`, `Fn`, `If`, `Else`, `Return`, `While`, `For`, `Struct`, etc.
/// - **Operators**: `Plus`, `Minus`, `Multiply`, `Equal`, `And`, `Or`, etc.
/// - **Punctuation**: `LParen`, `RBrace`, `Comma`, `SemiColon`, etc.
/// - **Special**: `EOF` marks the end of the token stream, `Illegal` for unrecognized input
#[derive(PartialEq, Debug, Clone)]
pub enum Token {
    Illegal,
    EOF,
    // identifier and literals
    Ident(String),
    StringLiteral(String),
    IntLiteral(i64),
    BigIntLiteral(BigInt),
    FloatLiteral(f64),
    BoolLiteral(bool),
    NullLiteral,
    // statements
    Assign,
    If,
    Else,
    // Assignments
    PlusAssign,
    MinusAssign,
    MultiplyAssign,
    DivideAssign,
    ModuloAssign,
    // operators
    Plus,
    Minus,
    Divide,
    Multiply,
    Modulo,
    Equal,
    NotEqual,
    GreaterThanEqual,
    LessThanEqual,
    GreaterThan,
    LessThan,
    // reserved words
    Function,
    Let,
    Return,
    Struct,
    Enum,
    This,
    Import,
    // punctuations
    Comma,
    Colon,
    SemiColon,
    LParen,
    RParen,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    // Logical operators
    And,
    Or,
    Not,
    Dot,
    DoubleColon,
    // Loops
    While,
    For,
    In,
    Break,
    Continue,
    // Error handling
    Try,
    Catch,
    Finally,
    Throw,
    // Match
    Match,
    FatArrow,
    // Async
    Async,
    Await,
}

/// A `nom`-compatible input wrapper over a slice of [`Token`]s.
///
/// Tracks `start` and `end` offsets so that error messages can reference
/// the original position in the token stream.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) struct Tokens<'a> {
    pub token: &'a [Token],
    pub start: usize,
    pub end: usize,
}

impl<'a> Tokens<'a> {
    /// Creates a new `Tokens` wrapper over the full slice.
    pub fn new(vec: &'a [Token]) -> Self {
        Tokens {
            token: vec,
            start: 0,
            end: vec.len(),
        }
    }
}

// nom trait implementations — these allow `Tokens` to be used as the
// input type for nom parser combinators.

impl<'a> InputLength for Tokens<'a> {
    #[inline]
    fn input_len(&self) -> usize {
        self.token.len()
    }
}

impl<'a> InputTake for Tokens<'a> {
    #[inline]
    fn take(&self, count: usize) -> Self {
        Tokens {
            token: &self.token[0..count],
            start: 0,
            end: count,
        }
    }

    #[inline]
    fn take_split(&self, count: usize) -> (Self, Self) {
        let (prefix, suffix) = self.token.split_at(count);
        let first = Tokens {
            token: prefix,
            start: 0,
            end: prefix.len(),
        };
        let second = Tokens {
            token: suffix,
            start: 0,
            end: suffix.len(),
        };
        (second, first)
    }
}

impl InputLength for Token {
    #[inline]
    fn input_len(&self) -> usize {
        1
    }
}

impl<'a> Slice<Range<usize>> for Tokens<'a> {
    #[inline]
    fn slice(&self, range: Range<usize>) -> Self {
        Tokens {
            token: self.token.slice(range.clone()),
            start: self.start + range.start,
            end: self.start + range.end,
        }
    }
}

impl<'a> Slice<RangeTo<usize>> for Tokens<'a> {
    #[inline]
    fn slice(&self, range: RangeTo<usize>) -> Self {
        self.slice(0..range.end)
    }
}

impl<'a> Slice<RangeFrom<usize>> for Tokens<'a> {
    #[inline]
    fn slice(&self, range: RangeFrom<usize>) -> Self {
        self.slice(range.start..self.end - self.start)
    }
}

impl<'a> Slice<RangeFull> for Tokens<'a> {
    #[inline]
    fn slice(&self, _: RangeFull) -> Self {
        Tokens {
            token: self.token,
            start: self.start,
            end: self.end,
        }
    }
}

impl<'a> InputIter for Tokens<'a> {
    type Item = &'a Token;
    type Iter = Enumerate<::std::slice::Iter<'a, Token>>;
    type IterElem = ::std::slice::Iter<'a, Token>;

    #[inline]
    fn iter_indices(&self) -> Enumerate<::std::slice::Iter<'a, Token>> {
        self.token.iter().enumerate()
    }
    #[inline]
    fn iter_elements(&self) -> ::std::slice::Iter<'a, Token> {
        self.token.iter()
    }
    #[inline]
    fn position<P>(&self, predicate: P) -> Option<usize>
    where
        P: Fn(Self::Item) -> bool,
    {
        self.token.iter().position(predicate)
    }
    #[inline]
    fn slice_index(&self, count: usize) -> Result<usize, Needed> {
        if self.token.len() >= count {
            Ok(count)
        } else {
            Err(Needed::Unknown)
        }
    }
}

/// Represents a location in source code (line and column).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Location {
    pub line: usize,
    pub column: usize,
}

impl Location {
    pub fn new(line: usize, column: usize) -> Self {
        Self { line, column }
    }
}

impl std::fmt::Display for Location {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "line {}, column {}", self.line, self.column)
    }
}

/// Represents a span of source code with start and end locations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Span {
    pub start: Location,
    pub end: Location,
}

impl Span {
    pub fn new(start: Location, end: Location) -> Self {
        Self { start, end }
    }

    pub fn merge(self, other: Span) -> Self {
        Self {
            start: self.start,
            end: other.end,
        }
    }
}

/// A value paired with its source location span.
///
/// Used by the lexer to attach position information to each token,
/// enabling accurate error reporting with file/line/column details.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Spanned<T> {
    pub node: T,
    pub span: Span,
}

impl<T> Spanned<T> {
    pub fn new(node: T, span: Span) -> Self {
        Self { node, span }
    }
}

impl<T> std::ops::Deref for Spanned<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &self.node
    }
}

impl<T> std::ops::DerefMut for Spanned<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.node
    }
}

/// A cursor over a slice of [`Spanned<Token>`] with position tracking.
///
/// Provides convenient methods for peeking and advancing through the token
/// stream while preserving access to source location information.
#[derive(Clone, Debug)]
pub(crate) struct SpannedTokens<'a> {
    pub tokens: &'a [Spanned<Token>],
    pub index: usize,
}

impl<'a> SpannedTokens<'a> {
    pub fn new(tokens: &'a [Spanned<Token>]) -> Self {
        Self { tokens, index: 0 }
    }

    pub fn to_tokens(&self) -> Tokens<'static> {
        let owned: Vec<Token> = self.tokens.iter().map(|s| s.node.clone()).collect();
        let leaked = Box::leak(owned.into_boxed_slice());
        Tokens::new(leaked)
    }

    pub fn to_tokens_with_offset(&self) -> (Tokens<'static>, usize) {
        let start_index = self.index;
        let owned: Vec<Token> = self.tokens.iter().map(|s| s.node.clone()).collect();
        let leaked = Box::leak(owned.into_boxed_slice());
        let tokens = Tokens::new(leaked);
        (tokens, start_index)
    }
}
