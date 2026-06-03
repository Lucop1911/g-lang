use num_bigint::BigInt;
use std::hash::{Hash, Hasher};

/// A program is a sequence of statements.
pub type Program = Vec<Stmt>;

/// Statements — constructs that do not produce a value on their own.
///
/// Includes declarations, control flow, assignments, and expression statements.
#[derive(PartialEq, Debug, Clone, Hash)]
pub enum Stmt {
    /// `let x = expr;`
    LetStmt(Ident, Expr),
    /// `let (a, b) = (1, 2);`
    MultiLetStmt {
        idents: Vec<Ident>,
        values: Vec<Expr>,
    },
    /// `x = expr;`
    AssignStmt(Ident, Expr),
    /// `(a, b) = (1, 2);`
    TupleAssignStmt {
        targets: Vec<Ident>,
        values: Vec<Expr>,
    },
    /// `obj.field = expr;`
    FieldAssignStmt {
        object: Box<Expr>,
        field: String,
        value: Box<Expr>,
    },
    /// `arr[i] = expr;`
    IndexAssignStmt {
        target: Box<Expr>,
        index: Box<Expr>,
        value: Box<Expr>,
    },
    /// `return expr`
    ReturnStmt(Expr),
    /// An expression whose result is discarded (statement position, ends with `;`).
    ExprStmt(Expr),
    /// An expression whose result is propagated (e.g. trailing expr in a block).
    ExprValueStmt(Expr),
    /// `fn name(params) { body }`
    FnStmt {
        name: Ident,
        params: Vec<Ident>,
        body: Program,
    },
    /// `struct Name { fields..., methods... }`
    StructStmt {
        name: Ident,
        fields: Vec<(Ident, Expr)>,
        methods: Vec<(Ident, Expr)>,
    },
    /// `enum Name { variants... }`
    EnumStmt {
        name: Ident,
        variants: Vec<EnumVariant>,
    },
    /// `import path::to::{items};`
    ImportStmt {
        path: Vec<String>,
        items: ImportItems,
    },
    BreakStmt,
    ContinueStmt,
    /// `throw expr`
    ThrowStmt(Expr),
}

/// Expressions — constructs that evaluate to an [`Object`].
#[derive(PartialEq, Debug, Clone, Hash)]
pub enum Expr {
    IdentExpr(Ident),
    LitExpr(Literal),
    PrefixExpr(Prefix, Box<Expr>),
    InfixExpr(Infix, Box<Expr>, Box<Expr>),
    IfExpr {
        cond: Box<Expr>,
        consequence: Program,
        alternative: Option<Program>,
    },
    FnExpr {
        params: Vec<Ident>,
        body: Program,
    },
    CallExpr {
        function: Box<Expr>,
        arguments: Vec<Expr>,
    },
    ArrayExpr(Vec<Expr>),
    HashExpr(Vec<(Expr, Expr)>),
    IndexExpr {
        array: Box<Expr>,
        index: Box<Expr>,
    },
    MethodCallExpr {
        object: Box<Expr>,
        method: String,
        arguments: Vec<Expr>,
    },
    StructLiteral {
        name: Ident,
        fields: Vec<(Ident, Expr)>,
    },
    EnumStructLiteral {
        enum_name: Ident,
        variant_name: Ident,
        fields: Vec<(Ident, Expr)>,
    },
    ThisExpr,
    FieldAccessExpr {
        object: Box<Expr>,
        field: String,
    },
    WhileExpr {
        cond: Box<Expr>,
        body: Program,
    },
    ForExpr {
        ident: Vec<Ident>,
        iterable: Box<Expr>,
        body: Program,
    },
    CStyleForExpr {
        init: Option<Box<Stmt>>,
        cond: Option<Box<Expr>>,
        update: Option<Box<Stmt>>,
        body: Program,
    },
    TryCatchExpr {
        try_body: Program,
        catch_ident: Option<Ident>,
        catch_body: Option<Program>,
        finally_body: Option<Program>,
    },
    AsyncFnExpr {
        params: Vec<Ident>,
        body: Program,
    },
    AwaitExpr(Box<Expr>),
    MatchExpr {
        value: Box<Expr>,
        arms: Vec<(Pattern, Program)>,
    },
}

#[derive(PartialEq, Debug, Clone, Hash)]
/// A pattern used in a match arm.
pub enum Pattern {
    /// Wildcard pattern `_` that matches anything.
    Wildcard,
    /// Literal pattern
    Literal(Literal),
    /// Enum variant pattern
    Enum {
        path: Vec<String>,
        payload: EnumPatternPayload,
    },
}

/// The data associated with an enum variant in a match pattern.
#[derive(PartialEq, Debug, Clone, Hash)]
pub enum EnumPatternPayload {
    /// Simple variant without data.
    Unit,
    /// Tuple-like variant with bindings
    Tuple(Vec<Ident>),
    /// Struct-like variant with bindings
    Struct(Vec<(Ident, Ident)>),
}

/// A variant definition in an `enum` declaration.
#[derive(PartialEq, Debug, Clone, Hash)]
pub enum EnumVariant {
    /// A simple variant
    Unit(Ident),
    /// A tuple-like variant
    Tuple(Ident, Vec<Ident>),
    /// A struct-like variant
    Struct(Ident, Vec<Ident>),
}

/// Runtime literal values as they appear in source.
#[derive(PartialEq, Debug, Clone)]
pub enum Literal {
    IntLiteral(i64),
    BigIntLiteral(BigInt),
    FloatLiteral(f64),
    BoolLiteral(bool),
    StringLiteral(String),
    NullLiteral,
}

impl Hash for Literal {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match self {
            Literal::IntLiteral(i) => i.hash(state),
            Literal::BigIntLiteral(b) => b.hash(state),
            // f64 doesn't implement Hash; use bit representation instead.
            Literal::FloatLiteral(f) => f.to_bits().hash(state),
            Literal::BoolLiteral(b) => b.hash(state),
            Literal::StringLiteral(s) => s.hash(state),
            Literal::NullLiteral => "null".hash(state),
        }
    }
}

/// Slot index for O(1) variable access inside function frames.
///
/// During the compiler pass (`compute_slots`), each identifier is assigned
/// a slot index that corresponds to its position in the environment's
/// `slots` vector. `UNSET` indicates that name-based lookup should be used
/// instead (e.g. for variables captured from enclosing scopes).
#[derive(PartialEq, Debug, Eq, Clone, Copy, Hash)]
pub struct SlotIndex(pub u16);

impl SlotIndex {
    /// Sentinel value indicating no slot has been assigned.
    pub const UNSET: SlotIndex = SlotIndex(u16::MAX);

    pub fn is_unset(&self) -> bool {
        *self == Self::UNSET
    }
}

/// An identifier paired with an optional slot index.
///
/// The `name` field is always populated and serves as the fallback for
/// name-based lookups. The `slot` field is filled in by the compiler pass
/// for O(1) access within the correct scope.
#[derive(PartialEq, Debug, Eq, Clone, Hash)]
pub struct Ident {
    pub name: String,
    pub slot: SlotIndex,
}

impl Ident {
    pub fn new(name: String) -> Self {
        Ident {
            name,
            slot: SlotIndex::UNSET,
        }
    }
}

/// Unary operators.
#[derive(PartialEq, Debug, Clone, Hash)]
pub enum Prefix {
    PrefixPlus,
    PrefixMinus,
    Not,
}

/// Binary operators.
#[derive(PartialEq, Debug, Clone, Hash)]
pub enum Infix {
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
    And,
    Or,
}

/// Operator precedence levels used by the Pratt parser.
///
/// Variants are ordered from lowest to highest binding strength so that
/// `PartialOrd` comparisons work correctly.
#[derive(PartialEq, PartialOrd, Debug, Clone)]
pub(crate) enum Precedence {
    PLowest,
    POr,          // Lowest logical operator
    PAnd,         // Higher than OR
    PEquals,      // ==, !=
    PLessGreater, // <, >, <=, >=
    PSum,         // +, -
    PProduct,     // *, /, %
    PPrefix,      // !, -, +
    PCall,        // function calls
    PIndex,       // array[index]
}

/// Import specifier: `import foo::*`, `import foo::{a, b}`, or `import foo::bar`.
#[derive(PartialEq, Debug, Clone, Hash)]
pub enum ImportItems {
    All,
    Specific(Vec<String>),
    Single(String),
}
