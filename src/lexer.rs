// Lexer: tokenizes .cry files using logos, with Cryptol layout processing.
//
// Pipeline: source → strip block comments → logos tokenize → merge qualifieds
//         → apply layout (insert VCurlyL/VCurlyR/VSemi) → token stream for lalrpop

use logos::Logos;
use std::fmt;

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

// ── Raw logos tokenizer ─────────────────────────────────────────────────────

/// Internal token type for logos. We post-process into `Tok`.
#[derive(Logos, Debug, Clone, PartialEq)]
#[logos(skip r"[ \t\r]+")]
enum RawTok {
    // ── Keywords (explicit strings, higher priority than ident regex) ────
    #[token("else")]
    KwElse,
    #[token("if")]
    KwIf,
    #[token("case")]
    KwCase,
    #[token("of")]
    KwOf,
    #[token("private")]
    KwPrivate,
    #[token("include")]
    KwInclude,
    #[token("module")]
    KwModule,
    #[token("submodule")]
    KwSubmodule,
    #[token("interface")]
    KwInterface,
    #[token("newtype")]
    KwNewtype,
    #[token("enum")]
    KwEnum,
    #[token("deriving")]
    KwDeriving,
    #[token("property")]
    KwProperty,
    #[token("then")]
    KwThen,
    #[token("type")]
    KwType,
    #[token("where")]
    KwWhere,
    #[token("let")]
    KwLet,
    #[token("import")]
    KwImport,
    #[token("as")]
    KwAs,
    #[token("hiding")]
    KwHiding,
    #[token("infixl")]
    KwInfixl,
    #[token("infixr")]
    KwInfixr,
    #[token("infix")]
    KwInfix,
    #[token("primitive")]
    KwPrimitive,
    #[token("parameter")]
    KwParameter,
    #[token("constraint")]
    KwConstraint,
    #[token("foreign")]
    KwForeign,
    #[token("Prop")]
    KwProp,
    #[token("by")]
    KwBy,
    #[token("down")]
    KwDown,

    // ── Multi-char symbols (longer patterns first for correct matching) ──
    #[token("...")]
    DotDotDot,
    #[token("..<")]
    DotDotLt,
    #[token("..>")]
    DotDotGt,
    #[token("..")]
    DotDot,
    #[token("<-")]
    ArrL,
    #[token("->")]
    ArrR,
    #[token("=>")]
    FatArrR,
    #[token("<|")]
    TriL,
    #[token("|>")]
    TriR,
    #[token("^^")]
    Exp,

    // ── Single-char symbols ─────────────────────────────────────────────
    #[token("|")]
    Bar,
    #[token("\\")]
    Lambda,
    #[token("=")]
    EqDef,
    #[token(",")]
    Comma,
    #[token(";")]
    Semi,
    #[token(":")]
    Colon,
    #[token("`")]
    BackTick,
    #[token("(")]
    ParenL,
    #[token(")")]
    ParenR,
    #[token("[")]
    BracketL,
    #[token("]")]
    BracketR,
    #[token("{")]
    CurlyL,
    #[token("}")]
    CurlyR,
    #[token("<")]
    Lt,
    #[token(">")]
    Gt,
    #[token("_", priority = 10)]
    Underscore,

    // ── Named operators ─────────────────────────────────────────────────
    #[token("+")]
    Plus,
    #[token("-")]
    Minus,
    #[token("*")]
    Star,
    #[token("#")]
    Hash,
    #[token("@")]
    At,
    #[token("~")]
    Complement,

    // ── Generic operators (catch-all for op-char sequences) ─────────────
    // Matches sequences of operator characters not caught above.
    // Low priority so specific multi-char tokens (->  =>  .. etc.) win.
    #[regex(r"[!%&\*/\+\-\.:<=>\\?\^|~]{2,}", priority = 1)]
    OpOther,

    // ── Newline (tracked for layout) ────────────────────────────────────
    #[token("\n")]
    Newline,

    // ── Line comment ────────────────────────────────────────────────────
    #[regex(r"//[^\n]*", priority = 10)]
    LineComment,

    // ── Literals ────────────────────────────────────────────────────────
    #[regex(r"0x[0-9a-fA-F][0-9a-fA-F_]*")]
    HexLit,
    #[regex(r"0b[01][01_]*")]
    BinLit,
    #[regex(r"0o[0-7][0-7_]*")]
    OctLit,
    #[regex(r"[0-9][0-9_]*\.[0-9][0-9_]*([eEpP][+\-]?[0-9_]+)?")]
    FracLit,
    #[regex(r"[0-9][0-9_]*")]
    DecLit,

    // ── Selector (.field or .0) ─────────────────────────────────────────
    #[regex(r"\.[a-zA-Z_][a-zA-Z0-9_']*")]
    SelectorIdent,
    #[regex(r"\.[0-9]+")]
    SelectorNum,

    // ── String / char literals ──────────────────────────────────────────
    #[regex(r#""([^"\\]|\\.)*""#)]
    StrLit,
    #[regex(r"'([^'\\]|\\.)'")]
    ChrLit,

    // ── Identifier ──────────────────────────────────────────────────────
    #[regex(r"[a-zA-Z_][a-zA-Z0-9_']*")]
    Ident,
}

// ── Pre-processing: strip block comments ────────────────────────────────────

pub struct DocComment {
    pub content: String,
    pub byte_pos: usize,
}

/// Replace block comments with whitespace (preserving newlines for line tracking).
/// Extract `/** ... */` doc comments for later insertion.
fn strip_block_comments(source: &str) -> (String, Vec<DocComment>) {
    let bytes = source.as_bytes();
    let mut out = vec![b' '; bytes.len()];
    let mut docs = Vec::new();
    let mut i = 0;

    // Preserve non-comment bytes
    while i < bytes.len() {
        if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'*' {
            let is_doc = i + 2 < bytes.len()
                && bytes[i + 2] == b'*'
                && !(i + 3 < bytes.len() && bytes[i + 3] == b'*');
            let start = i;
            i += 2;
            let mut depth = 1u32;
            let content_start = i;
            while i < bytes.len() && depth > 0 {
                if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'*' {
                    depth += 1;
                    i += 2;
                } else if i + 1 < bytes.len() && bytes[i] == b'*' && bytes[i + 1] == b'/' {
                    depth -= 1;
                    i += 2;
                } else {
                    if bytes[i] == b'\n' {
                        out[i] = b'\n'; // preserve newlines
                    }
                    i += 1;
                }
            }
            if is_doc {
                let end = if i >= 2 { i - 2 } else { i };
                let content = String::from_utf8_lossy(&bytes[content_start..end])
                    .trim_start_matches('*')
                    .trim()
                    .to_string();
                if !content.is_empty() {
                    docs.push(DocComment {
                        content,
                        byte_pos: start,
                    });
                }
            }
            // bytes from start..i are already spaces in `out`
            // but preserve newlines
            for j in start..i.min(bytes.len()) {
                if bytes[j] == b'\n' {
                    out[j] = b'\n';
                }
            }
        } else {
            out[i] = bytes[i];
            i += 1;
        }
    }

    let cleaned = String::from_utf8_lossy(&out).into_owned();
    (cleaned, docs)
}

// ── Token with position info ────────────────────────────────────────────────

struct PosToken {
    tok: Tok,
    start: usize,
    end: usize,
    line: usize,
    col: usize,
}

// ── Column computation ──────────────────────────────────────────────────────

fn compute_line_starts(source: &str) -> Vec<usize> {
    let mut starts = vec![0usize];
    for (i, b) in source.bytes().enumerate() {
        if b == b'\n' {
            starts.push(i + 1);
        }
    }
    starts
}

fn byte_to_line_col(offset: usize, line_starts: &[usize]) -> (usize, usize) {
    let line = match line_starts.binary_search(&offset) {
        Ok(l) => l,
        Err(l) => l.saturating_sub(1),
    };
    let col = offset - line_starts[line] + 1;
    (line + 1, col) // 1-based
}

// ── Raw → Tok conversion ───────────────────────────────────────────────────

fn raw_to_tok(raw: &RawTok, text: &str) -> Option<Tok> {
    Some(match raw {
        RawTok::KwElse => Tok::KwElse,
        RawTok::KwIf => Tok::KwIf,
        RawTok::KwCase => Tok::KwCase,
        RawTok::KwOf => Tok::KwOf,
        RawTok::KwPrivate => Tok::KwPrivate,
        RawTok::KwInclude => Tok::KwInclude,
        RawTok::KwModule => Tok::KwModule,
        RawTok::KwSubmodule => Tok::KwSubmodule,
        RawTok::KwInterface => Tok::KwInterface,
        RawTok::KwNewtype => Tok::KwNewtype,
        RawTok::KwEnum => Tok::KwEnum,
        RawTok::KwDeriving => Tok::KwDeriving,
        RawTok::KwProperty => Tok::KwProperty,
        RawTok::KwThen => Tok::KwThen,
        RawTok::KwType => Tok::KwType,
        RawTok::KwWhere => Tok::KwWhere,
        RawTok::KwLet => Tok::KwLet,
        RawTok::KwImport => Tok::KwImport,
        RawTok::KwAs => Tok::KwAs,
        RawTok::KwHiding => Tok::KwHiding,
        RawTok::KwInfixl => Tok::KwInfixl,
        RawTok::KwInfixr => Tok::KwInfixr,
        RawTok::KwInfix => Tok::KwInfix,
        RawTok::KwPrimitive => Tok::KwPrimitive,
        RawTok::KwParameter => Tok::KwParameter,
        RawTok::KwConstraint => Tok::KwConstraint,
        RawTok::KwForeign => Tok::KwForeign,
        RawTok::KwProp => Tok::KwProp,
        RawTok::KwBy => Tok::KwBy,
        RawTok::KwDown => Tok::KwDown,

        RawTok::DotDotDot => Tok::DotDotDot,
        RawTok::DotDotLt => Tok::DotDotLt,
        RawTok::DotDotGt => Tok::DotDotGt,
        RawTok::DotDot => Tok::DotDot,
        RawTok::ArrL => Tok::ArrL,
        RawTok::ArrR => Tok::ArrR,
        RawTok::FatArrR => Tok::FatArrR,
        RawTok::TriL => Tok::TriL,
        RawTok::TriR => Tok::TriR,
        RawTok::Exp => Tok::Exp,

        RawTok::Bar => Tok::Bar,
        RawTok::Lambda => Tok::Lambda,
        RawTok::EqDef => Tok::EqDef,
        RawTok::Comma => Tok::Comma,
        RawTok::Semi => Tok::Semi,
        RawTok::Colon => Tok::Colon,
        RawTok::BackTick => Tok::BackTick,
        RawTok::ParenL => Tok::ParenL,
        RawTok::ParenR => Tok::ParenR,
        RawTok::BracketL => Tok::BracketL,
        RawTok::BracketR => Tok::BracketR,
        RawTok::CurlyL => Tok::CurlyL,
        RawTok::CurlyR => Tok::CurlyR,
        RawTok::Lt => Tok::Lt,
        RawTok::Gt => Tok::Gt,
        RawTok::Underscore => Tok::Underscore,

        RawTok::Plus => Tok::Plus,
        RawTok::Minus => Tok::Minus,
        RawTok::Star => Tok::Star,
        RawTok::Hash => Tok::Hash,
        RawTok::At => Tok::At,
        RawTok::Complement => Tok::Complement,

        RawTok::OpOther => Tok::Op(text.to_string()),

        RawTok::HexLit => {
            let clean: String = text[2..].chars().filter(|c| *c != '_').collect();
            Tok::Num(u64::from_str_radix(&clean, 16).unwrap_or(0), 16)
        }
        RawTok::BinLit => {
            let clean: String = text[2..].chars().filter(|c| *c != '_').collect();
            Tok::Num(u64::from_str_radix(&clean, 2).unwrap_or(0), 2)
        }
        RawTok::OctLit => {
            let clean: String = text[2..].chars().filter(|c| *c != '_').collect();
            Tok::Num(u64::from_str_radix(&clean, 8).unwrap_or(0), 8)
        }
        RawTok::DecLit => {
            let clean: String = text.chars().filter(|c| *c != '_').collect();
            Tok::Num(clean.parse::<u64>().unwrap_or(0), 10)
        }
        RawTok::FracLit => Tok::Frac(text.to_string()),

        RawTok::SelectorIdent => Tok::Selector(text[1..].to_string()),
        RawTok::SelectorNum => Tok::Selector(text[1..].to_string()),

        RawTok::StrLit => Tok::StrLit(text[1..text.len() - 1].to_string()),
        RawTok::ChrLit => {
            let inner = &text[1..text.len() - 1];
            let ch = if inner.starts_with('\\') {
                match inner.chars().nth(1) {
                    Some('n') => '\n',
                    Some('t') => '\t',
                    Some('r') => '\r',
                    Some('\\') => '\\',
                    Some('\'') => '\'',
                    Some('0') => '\0',
                    Some(c) => c,
                    None => '?',
                }
            } else {
                inner.chars().next().unwrap_or('?')
            };
            Tok::ChrLit(ch)
        }

        RawTok::Ident => {
            // `x` is a keyword in Cryptol (for polynomial expressions)
            if text == "x" {
                Tok::KwX
            } else {
                Tok::Ident(text.to_string())
            }
        }

        RawTok::Newline | RawTok::LineComment => return None,
    })
}

// ── Tokenize: logos + post-processing ───────────────────────────────────────

fn tokenize(cleaned: &str, line_starts: &[usize]) -> Vec<PosToken> {
    let mut tokens = Vec::new();
    let mut lexer = RawTok::lexer(cleaned);

    while let Some(result) = lexer.next() {
        let span = lexer.span();
        let text = &cleaned[span.clone()];

        match result {
            Ok(ref raw) => {
                // Track line comments for doc extraction (stored separately)
                if *raw == RawTok::LineComment {
                    let (line, col) = byte_to_line_col(span.start, line_starts);
                    let doc_text = text.strip_prefix("//").unwrap_or(text);
                    let doc_text = doc_text.strip_prefix(' ').unwrap_or(doc_text);
                    let doc_text = doc_text.trim_end_matches('\r');
                    if !doc_text.is_empty() {
                        tokens.push(PosToken {
                            tok: Tok::Doc(doc_text.to_string()),
                            start: span.start,
                            end: span.end,
                            line,
                            col,
                        });
                    }
                    continue;
                }
                if *raw == RawTok::Newline {
                    continue; // newlines handled by line_starts
                }
                if let Some(tok) = raw_to_tok(raw, text) {
                    let (line, col) = byte_to_line_col(span.start, line_starts);
                    tokens.push(PosToken {
                        tok,
                        start: span.start,
                        end: span.end,
                        line,
                        col,
                    });
                }
            }
            Err(()) => {
                // Skip unrecognized characters
            }
        }
    }

    tokens
}

// ── Merge qualified identifiers ─────────────────────────────────────────────

fn merge_qualified(tokens: &mut Vec<PosToken>) {
    // Pattern: Ident(A) :: Ident(B) :: ... :: Ident(Z) → QualIdent("A::B::...::", "Z")
    // We look for Ident followed by Op("::") followed by Ident
    let mut i = 0;
    while i + 2 < tokens.len() {
        let is_qual_start = matches!(&tokens[i].tok, Tok::Ident(_))
            && matches!(&tokens[i + 1].tok, Tok::Op(s) if s == "::")
            && matches!(&tokens[i + 2].tok, Tok::Ident(_) | Tok::Op(_));

        if is_qual_start {
            let mut qual = String::new();
            if let Tok::Ident(ref s) = tokens[i].tok {
                qual.push_str(s);
                qual.push_str("::");
            }
            let start = tokens[i].start;
            let line = tokens[i].line;
            let col = tokens[i].col;

            let mut j = i + 2;
            // Keep merging if we see more ::Ident
            while j + 1 < tokens.len() {
                if matches!(&tokens[j + 1].tok, Tok::Op(s) if s == "::")
                    && j + 2 < tokens.len()
                    && matches!(&tokens[j + 2].tok, Tok::Ident(_) | Tok::Op(_))
                {
                    if let Tok::Ident(ref s) = tokens[j].tok {
                        qual.push_str(s);
                        qual.push_str("::");
                    }
                    j += 2;
                } else {
                    break;
                }
            }

            let end = tokens[j].end;
            let final_tok = match &tokens[j].tok {
                Tok::Ident(s) => Tok::QualIdent(qual, s.clone()),
                Tok::Op(s) => Tok::QualOp(qual, s.clone()),
                _ => unreachable!(),
            };

            // Replace tokens[i..=j] with a single token
            let merged = PosToken {
                tok: final_tok,
                start,
                end,
                line,
                col,
            };
            tokens.splice(i..=j, [merged]);
            // Don't increment i — check the merged token against the next
        } else {
            i += 1;
        }
    }
}

// ── Layout processing ───────────────────────────────────────────────────────
//
// Cryptol uses Haskell-style layout: after `where`, `let`, `of`, `parameter`,
// if the next token is not `{`, insert VCurlyL and track indentation.
// At each new line:
//   col == context  → insert VSemi
//   col <  context  → insert VCurlyR, pop, recheck
//   col >  context  → continuation

fn is_layout_keyword(tok: &Tok) -> bool {
    matches!(
        tok,
        Tok::KwWhere | Tok::KwLet | Tok::KwOf | Tok::KwParameter
    )
}

fn apply_layout(tokens: Vec<PosToken>) -> Vec<(usize, Tok, usize)> {
    if tokens.is_empty() {
        return Vec::new();
    }

    let mut result: Vec<(usize, Tok, usize)> = Vec::new();
    let mut layout_stack: Vec<usize> = Vec::new(); // stack of context columns
    let mut expect_layout = false; // true after a layout keyword
    let mut prev_line = 0usize;

    for pt in &tokens {
        let col = pt.col;
        let line = pt.line;

        if expect_layout {
            expect_layout = false;
            if pt.tok == Tok::CurlyL {
                // Explicit brace — no layout
                result.push((pt.start, pt.tok.clone(), pt.end));
                prev_line = line;
                continue;
            }
            // Insert VCurlyL and push context.
            // The opening token defines the context — skip layout checks for it.
            result.push((pt.start, Tok::VCurlyL, pt.start));
            layout_stack.push(col);
        } else if line > prev_line && !layout_stack.is_empty() {
            // At a new line, check layout
            // Use the end of the previous real token for virtual token positions
            // so that spans don't extend into whitespace/block-comment gaps.
            let prev_end = result.last().map(|(_, _, e)| *e).unwrap_or(pt.start);
            // Close contexts that are deeper than current column
            while let Some(&ctx) = layout_stack.last() {
                if col < ctx {
                    // Only insert VSemi if the block isn't empty
                    if !matches!(result.last(), Some((_, Tok::VCurlyL, _))) {
                        result.push((prev_end, Tok::VSemi, prev_end));
                    }
                    result.push((prev_end, Tok::VCurlyR, prev_end));
                    layout_stack.pop();
                } else {
                    break;
                }
            }
            // Insert VSemi if at same level
            if let Some(&ctx) = layout_stack.last()
                && col == ctx
            {
                result.push((prev_end, Tok::VSemi, prev_end));
            }
        }

        // Check for layout keyword
        if is_layout_keyword(&pt.tok) {
            expect_layout = true;
        }

        result.push((pt.start, pt.tok.clone(), pt.end));
        prev_line = line;
    }

    // If expect_layout is still pending (layout keyword at end of input),
    // insert an empty layout block
    if expect_layout {
        let last_pos = tokens.last().map(|t| t.end).unwrap_or(0);
        result.push((last_pos, Tok::VCurlyL, last_pos));
        layout_stack.push(0);
    }

    // Close all remaining layout contexts
    let last_pos = tokens.last().map(|t| t.end).unwrap_or(0);
    while layout_stack.pop().is_some() {
        // Only insert VSemi if the block isn't empty
        if !matches!(result.last(), Some((_, Tok::VCurlyL, _))) {
            result.push((last_pos, Tok::VSemi, last_pos));
        }
        result.push((last_pos, Tok::VCurlyR, last_pos));
    }

    result
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
    let (cleaned, block_docs) = strip_block_comments(source);

    // 2. Compute line starts for column tracking
    let line_starts = compute_line_starts(&cleaned);

    // 3. Tokenize with logos
    let mut tokens = tokenize(&cleaned, &line_starts);

    // 4. Merge qualified identifiers (Foo::bar)
    merge_qualified(&mut tokens);

    // 5. Apply layout processing
    let result = apply_layout(tokens);

    Ok((result, block_docs))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lex_module_header() {
        let (tokens, _) = lex("module Foo where\n").unwrap();
        let toks: Vec<&Tok> = tokens.iter().map(|(_, t, _)| t).collect();
        assert!(matches!(toks[0], Tok::KwModule));
        assert!(matches!(toks[1], Tok::Ident(s) if s == "Foo"));
        assert!(matches!(toks[2], Tok::KwWhere));
        // After `where`, layout inserts VCurlyL; empty block closes with VCurlyR (no VSemi)
        assert!(matches!(toks[3], Tok::VCurlyL));
        assert!(matches!(toks.last(), Some(Tok::VCurlyR)));
    }

    #[test]
    fn test_lex_type_alias() {
        let (tokens, _) = lex("type FleetMode = [1]\n").unwrap();
        let toks: Vec<&Tok> = tokens.iter().map(|(_, t, _)| t).collect();
        assert!(matches!(toks[0], Tok::KwType));
    }

    #[test]
    fn test_lex_operators() {
        let (tokens, _) = lex("a == b ==> c\n").unwrap();
        let toks: Vec<&Tok> = tokens.iter().map(|(_, t, _)| t).collect();
        // == and ==> should be Op tokens
        assert!(toks.iter().any(|t| matches!(t, Tok::Op(s) if s == "==")));
        assert!(toks.iter().any(|t| matches!(t, Tok::Op(s) if s == "==>")));
    }

    #[test]
    fn test_block_comment_stripping() {
        let (cleaned, _) = strip_block_comments("a /* comment */ b");
        assert!(cleaned.contains('a'));
        assert!(cleaned.contains('b'));
        assert!(!cleaned.contains("comment"));
    }

    #[test]
    fn test_nested_block_comments() {
        let (cleaned, _) = strip_block_comments("a /* outer /* inner */ still */ b");
        assert!(cleaned.contains('a'));
        assert!(cleaned.contains('b'));
        assert!(!cleaned.contains("outer"));
        assert!(!cleaned.contains("inner"));
    }
}
