// AST types produced by the lalrpop grammar.
// These are intentionally lean: expressions, types, and patterns track spans
// rather than full tree structure, since the renderer only needs their source text.

/// Byte-offset span in the original source.
pub type Span = (usize, usize);

// ── Top-level parse result ──────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct ParsedModule {
    pub name: Option<String>,
    pub decls: Vec<ParsedTopDecl>,
}

// ── Top-level declarations ──────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum ParsedTopDecl {
    Decl(ParsedDecl),
    Import {
        module_path: String,
        qualifier: Option<String>,
        hiding: Vec<String>,
    },
    Include(String),
    Private(Vec<ParsedTopDecl>),
    SubModule {
        name: String,
        decls: Vec<ParsedTopDecl>,
    },
    DocComment(String),
}

// ── Declarations ────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum ParsedDecl {
    /// `name : schema` — type signature
    Signature {
        names: Vec<String>,
        rhs_span: Span,
    },
    /// `name params = body` — function/value binding
    Bind {
        name: String,
        full_span: Span,
        body_span: Span,
        is_infix: bool,
    },
    /// `type Name = Type` — type synonym
    TypeSyn {
        name: String,
        rhs_span: Span,
    },
    /// `type constraint Name = Type` — prop synonym
    PropSyn {
        name: String,
        rhs_span: Span,
    },
    /// `property Name params = body`
    Property {
        name: String,
        params: Vec<String>,
        body_span: Span,
    },
    /// `newtype Name = { ... }`
    Newtype {
        name: String,
        full_span: Span,
    },
    /// `enum Name = Con1 | Con2 ...`
    Enum {
        name: String,
        full_span: Span,
    },
    /// `infixl/infixr/infix N op1, op2`
    Fixity {
        assoc: Assoc,
        level: u64,
        ops: Vec<String>,
    },
    /// `foreign name : schema`
    Foreign {
        name: String,
        schema_span: Span,
    },
    /// `primitive name : schema`
    Primitive {
        name: String,
        schema_span: Span,
    },
    /// `pattern = expr` (no name extraction possible)
    PatBind {
        full_span: Span,
    },
}

#[derive(Debug, Clone, Copy)]
pub enum Assoc {
    Left,
    Right,
    Non,
}
