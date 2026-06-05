// Raw logos tokenizer + raw→public Tok conversion.

use logos::Logos;

use super::PosToken;
use super::Tok;
use super::preprocess::byte_to_line_col;

/// Internal token type for logos. We post-process into `Tok`.
#[derive(Logos, Debug, Clone, PartialEq)]
#[logos(skip r"[ \t\r]+")]
pub(super) enum RawTok {
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

pub(super) fn tokenize(cleaned: &str, line_starts: &[usize]) -> Vec<PosToken> {
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
