// Lexer: tokenizes .cry files using logos, with Cryptol layout processing.
//
// Pipeline: source → strip block comments → logos tokenize → merge qualifieds
//         → apply layout (insert VCurlyL/VCurlyR/VSemi) → token stream for lalrpop

use std::fmt;

mod layout;
mod merge;
mod preprocess;
mod raw;
#[cfg(test)]
mod tests;

pub use preprocess::DocComment;

// ── Token type (used by lalrpop) ────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum Tok {
    // Literals
    Num(u64, u8), // value, base (10/16/2/8)
    Frac(String), // kept as string
    StrLit(String),
    ChrLit(char),

    // Identifiers
    Ident(String),
    QualIdent(String, String), // qualifier, name  e.g. ("Foo::Bar::", "baz")
    Selector(String),          // ".field" or ".0"

    // ── Keywords ────────────────────────────────────────────────────────
    KwElse,
    KwIf,
    KwCase,
    KwOf,
    KwPrivate,
    KwInclude,
    KwModule,
    KwSubmodule,
    KwInterface,
    KwNewtype,
    KwEnum,
    KwDeriving,
    KwProperty,
    KwThen,
    KwType,
    KwWhere,
    KwLet,
    KwX,
    KwImport,
    KwAs,
    KwHiding,
    KwInfixl,
    KwInfixr,
    KwInfix,
    KwPrimitive,
    KwParameter,
    KwConstraint,
    KwForeign,
    KwProp,
    KwBy,
    KwDown,

    // ── Symbols ─────────────────────────────────────────────────────────
    Bar,        // |
    ArrL,       // <-
    ArrR,       // ->
    FatArrR,    // =>
    Lambda,     // backslash
    EqDef,      // =
    Comma,      // ,
    Semi,       // ;
    Colon,      // :
    BackTick,   // `
    DotDot,     // ..
    DotDotDot,  // ...
    DotDotLt,   // ..<
    DotDotGt,   // ..>
    ParenL,     // (
    ParenR,     // )
    BracketL,   // [
    BracketR,   // ]
    CurlyL,     // {
    CurlyR,     // }
    TriL,       // <|
    TriR,       // |>
    Lt,         // <
    Gt,         // >
    Underscore, // _

    // ── Named operators (grammar-level) ─────────────────────────────────
    Plus,       // +
    Minus,      // -
    Star,       // *
    Exp,        // ^^
    Hash,       // #
    At,         // @
    Complement, // ~

    // ── Generic operators ───────────────────────────────────────────────
    Op(String),
    QualOp(String, String),

    // ── Layout (virtual) ────────────────────────────────────────────────
    VCurlyL,
    VCurlyR,
    VSemi,

    // ── Doc comment ─────────────────────────────────────────────────────
    Doc(String),
}

impl fmt::Display for Tok {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Tok::Num(v, _) => write!(f, "{v}"),
            Tok::Ident(s) | Tok::Op(s) => write!(f, "{s}"),
            _ => write!(f, "{:?}", self),
        }
    }
}

// ── Error type ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub struct LexError {
    pub pos: usize,
    pub msg: String,
}

impl fmt::Display for LexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "lex error at byte {}: {}", self.pos, self.msg)
    }
}

// ── Token with position info (shared across submodules) ─────────────────────

pub(super) struct PosToken {
    pub(super) tok: Tok,
    pub(super) start: usize,
    pub(super) end: usize,
    pub(super) line: usize,
    pub(super) col: usize,
}

// ── Public API ──────────────────────────────────────────────────────────────

/// A LALR-style spanned token: `(start_byte, token, end_byte)`. Aliased so
/// downstream signatures (and the LALRPOP-generated parser) stay below the
/// `clippy::type_complexity` threshold.
pub type SpannedTok = (usize, Tok, usize);

/// Output of [`lex`]: the spanned-token stream plus the extracted block
/// doc comments.
pub type LexOutput = (Vec<SpannedTok>, Vec<DocComment>);

/// Lex a Cryptol source string into a token stream suitable for lalrpop.
/// Returns (start_byte, token, end_byte) triples and extracted block doc comments.
pub fn lex(source: &str) -> Result<LexOutput, LexError> {
    // 1. Strip block comments (preserve positions)
    let (cleaned, block_docs) = preprocess::strip_block_comments(source);

    // 2. Compute line starts for column tracking
    let line_starts = preprocess::compute_line_starts(&cleaned);

    // 3. Tokenize with logos
    let mut tokens = raw::tokenize(&cleaned, &line_starts);

    // 4. Merge qualified identifiers (Foo::bar)
    merge::merge_qualified(&mut tokens);

    // 5. Apply layout processing
    let result = layout::apply_layout(tokens);

    Ok((result, block_docs))
}
