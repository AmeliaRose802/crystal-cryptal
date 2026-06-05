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
    let (cleaned, _) = super::preprocess::strip_block_comments("a /* comment */ b");
    assert!(cleaned.contains('a'));
    assert!(cleaned.contains('b'));
    assert!(!cleaned.contains("comment"));
}

#[test]
fn test_nested_block_comments() {
    let (cleaned, _) = super::preprocess::strip_block_comments("a /* outer /* inner */ still */ b");
    assert!(cleaned.contains('a'));
    assert!(cleaned.contains('b'));
    assert!(!cleaned.contains("outer"));
    assert!(!cleaned.contains("inner"));
}
