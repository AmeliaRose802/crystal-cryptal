// Lexer: tokenizes .cry files using logos.

use logos::Logos;

#[derive(Logos, Debug, Clone, PartialEq)]
#[logos(skip r"[ \t\r]+")]
pub enum Token {
    // Keywords (token patterns take priority over regex)
    #[token("module")]
    Module,
    #[token("where")]
    Where,
    #[token("type")]
    Type,
    #[token("property")]
    Property,
    #[token("if")]
    If,
    #[token("then")]
    Then,
    #[token("else")]
    Else,
    #[token("import")]
    Import,

    // Multi-char operators (before single-char to avoid prefix conflicts)
    #[token("==>")]
    Implies,
    #[token("==")]
    EqEq,
    #[token("!=")]
    NotEq,
    #[token("<=")]
    LtEq,
    #[token(">=")]
    GtEq,
    #[token("->")]
    Arrow,
    #[token("::")]
    ColonColon,
    #[token("&&")]
    And,
    #[token("||")]
    Or,
    #[token("<<<")]
    RotLeft,
    #[token(">>>")]
    RotRight,
    #[token("/\\")]
    LogAnd,
    #[token("\\/")]
    LogOr,

    // Single-char operators
    #[token("=")]
    Eq,
    #[token("<")]
    Lt,
    #[token(">")]
    Gt,
    #[token("~")]
    Tilde,
    #[token("#")]
    Hash,
    #[token("@")]
    At,
    #[token("+")]
    Plus,
    #[token("-")]
    Minus,
    #[token("*")]
    Star,
    #[token("|")]
    Pipe,
    #[token(",")]
    Comma,
    #[token(":")]
    Colon,
    #[token(".")]
    Dot,

    // Brackets
    #[token("(")]
    LParen,
    #[token(")")]
    RParen,
    #[token("[")]
    LBracket,
    #[token("]")]
    RBracket,
    #[token("{")]
    LBrace,
    #[token("}")]
    RBrace,

    // Newline
    #[token("\n")]
    Newline,

    // Comment (// to end of line, preserved for doc extraction)
    #[regex(r"//[^\n]*")]
    Comment,

    // Literals
    #[regex(r"0x[0-9a-fA-F]+")]
    HexLiteral,
    #[regex(r"0b[01]+")]
    BinLiteral,
    #[regex(r"[0-9]+")]
    IntLiteral,

    // Backtick type-level expression
    #[regex(r"`[a-zA-Z_][a-zA-Z0-9_]*")]
    Backtick,

    // Identifier
    #[regex(r"[a-zA-Z_][a-zA-Z0-9_]*")]
    Ident,

    // String literal
    #[regex(r#""([^"\\]|\\.)*""#)]
    StringLiteral,
}

/// Lex the input into a list of (Token, text, line_number) triples.
/// Line numbers are 1-based. Comments and newlines are preserved.
pub fn lex(input: &str) -> Vec<(Token, String, usize)> {
    let mut result = Vec::new();
    let mut line: usize = 1;
    let mut lexer = Token::lexer(input);

    while let Some(tok_result) = lexer.next() {
        match tok_result {
            Ok(tok) => {
                let slice = lexer.slice().to_string();
                let tok_line = line;
                if tok == Token::Newline {
                    line += 1;
                }
                result.push((tok, slice, tok_line));
            }
            Err(()) => {
                // Skip unrecognized characters
                let slice = lexer.slice();
                // Still track newlines in error spans
                line += slice.chars().filter(|&c| c == '\n').count();
            }
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tok_types(input: &str) -> Vec<Token> {
        lex(input).into_iter().map(|(t, _, _)| t).collect()
    }

    fn tok_pairs(input: &str) -> Vec<(Token, String)> {
        lex(input).into_iter().map(|(t, s, _)| (t, s)).collect()
    }

    #[test]
    fn test_module_header() {
        let tokens = tok_pairs("module Foo where\n");
        assert_eq!(
            tokens,
            vec![
                (Token::Module, "module".into()),
                (Token::Ident, "Foo".into()),
                (Token::Where, "where".into()),
                (Token::Newline, "\n".into()),
            ]
        );
    }

    #[test]
    fn test_type_alias() {
        let tokens = tok_types("type FleetMode = [1]\n");
        assert_eq!(
            tokens,
            vec![
                Token::Type,
                Token::Ident,
                Token::Eq,
                Token::LBracket,
                Token::IntLiteral,
                Token::RBracket,
                Token::Newline,
            ]
        );
    }

    #[test]
    fn test_comment_preserved() {
        let tokens = tok_pairs("// This is a comment\n");
        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[0].0, Token::Comment);
        assert_eq!(tokens[0].1, "// This is a comment");
        assert_eq!(tokens[1].0, Token::Newline);
    }

    #[test]
    fn test_integers() {
        let tokens = tok_pairs("0 42 255");
        assert_eq!(
            tokens,
            vec![
                (Token::IntLiteral, "0".into()),
                (Token::IntLiteral, "42".into()),
                (Token::IntLiteral, "255".into()),
            ]
        );
    }

    #[test]
    fn test_hex_literal() {
        let tokens = tok_pairs("0xFF 0x0A");
        assert_eq!(
            tokens,
            vec![
                (Token::HexLiteral, "0xFF".into()),
                (Token::HexLiteral, "0x0A".into()),
            ]
        );
    }

    #[test]
    fn test_bin_literal() {
        let tokens = tok_pairs("0b1010");
        assert_eq!(tokens, vec![(Token::BinLiteral, "0b1010".into())]);
    }

    #[test]
    fn test_operators() {
        assert_eq!(tok_types("=="), vec![Token::EqEq]);
        assert_eq!(tok_types("!="), vec![Token::NotEq]);
        assert_eq!(tok_types("==>"), vec![Token::Implies]);
        assert_eq!(tok_types("->"), vec![Token::Arrow]);
        assert_eq!(tok_types("/\\"), vec![Token::LogAnd]);
        assert_eq!(tok_types("\\/"), vec![Token::LogOr]);
        assert_eq!(tok_types("&&"), vec![Token::And]);
        assert_eq!(tok_types("||"), vec![Token::Or]);
        assert_eq!(tok_types("<="), vec![Token::LtEq]);
        assert_eq!(tok_types(">="), vec![Token::GtEq]);
        assert_eq!(tok_types("::"), vec![Token::ColonColon]);
        assert_eq!(tok_types("<<<"), vec![Token::RotLeft]);
        assert_eq!(tok_types(">>>"), vec![Token::RotRight]);
    }

    #[test]
    fn test_single_char_operators() {
        assert_eq!(tok_types("= < > ~ # @ + - *"), vec![
            Token::Eq, Token::Lt, Token::Gt, Token::Tilde,
            Token::Hash, Token::At, Token::Plus, Token::Minus, Token::Star,
        ]);
    }

    #[test]
    fn test_brackets() {
        assert_eq!(
            tok_types("( ) [ ] { }"),
            vec![
                Token::LParen, Token::RParen,
                Token::LBracket, Token::RBracket,
                Token::LBrace, Token::RBrace,
            ]
        );
    }

    #[test]
    fn test_backtick() {
        let tokens = tok_pairs("`FL `FLs");
        assert_eq!(
            tokens,
            vec![
                (Token::Backtick, "`FL".into()),
                (Token::Backtick, "`FLs".into()),
            ]
        );
    }

    #[test]
    fn test_line_numbers() {
        let tokens = lex("module Foo where\ntype X = [1]\n");
        // "module" is on line 1
        assert_eq!(tokens[0].2, 1);
        // "\n" is on line 1
        assert_eq!(tokens[3].2, 1);
        // "type" is on line 2
        assert_eq!(tokens[4].2, 2);
    }

    #[test]
    fn test_keywords_vs_identifiers() {
        let tokens = tok_pairs("module modular where wherever type typeAlias");
        assert_eq!(tokens[0], (Token::Module, "module".into()));
        assert_eq!(tokens[1], (Token::Ident, "modular".into()));
        assert_eq!(tokens[2], (Token::Where, "where".into()));
        assert_eq!(tokens[3], (Token::Ident, "wherever".into()));
        assert_eq!(tokens[4], (Token::Type, "type".into()));
        assert_eq!(tokens[5], (Token::Ident, "typeAlias".into()));
    }

    #[test]
    fn test_string_literal() {
        let tokens = tok_pairs(r#""hello world""#);
        assert_eq!(tokens, vec![(Token::StringLiteral, r#""hello world""#.into())]);
    }

    #[test]
    fn test_comma_colon_dot_pipe() {
        assert_eq!(tok_types(", : . |"), vec![
            Token::Comma, Token::Colon, Token::Dot, Token::Pipe,
        ]);
    }

    #[test]
    fn test_property_decl() {
        let tokens = tok_types("property P1_KeyMonotonicity fleetEnabled =\n");
        assert_eq!(
            tokens,
            vec![
                Token::Property,
                Token::Ident,
                Token::Ident,
                Token::Eq,
                Token::Newline,
            ]
        );
    }

    #[test]
    fn test_sdep_fixture() {
        let src = std::fs::read_to_string("tests/fixtures/SDEP.cry")
            .expect("failed to read SDEP.cry fixture");
        let tokens = lex(&src);

        // Should produce a substantial number of tokens
        assert!(tokens.len() > 100, "expected many tokens, got {}", tokens.len());

        // Should contain key token types
        let types: Vec<_> = tokens.iter().map(|(t, _, _)| t.clone()).collect();
        assert!(types.contains(&Token::Module));
        assert!(types.contains(&Token::Where));
        assert!(types.contains(&Token::Type));
        assert!(types.contains(&Token::Property));
        assert!(types.contains(&Token::Comment));
        assert!(types.contains(&Token::Arrow));
        assert!(types.contains(&Token::Implies));
        assert!(types.contains(&Token::EqEq));
        assert!(types.contains(&Token::And));
        assert!(types.contains(&Token::Or));
        assert!(types.contains(&Token::LogAnd));
        assert!(types.contains(&Token::Backtick));
    }
}
