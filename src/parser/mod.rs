// Parser: lalrpop grammar for structure, Rust code for semantics.
//
// The grammar (cryptol.lalrpop) classifies declarations by their leading
// keyword (type, property, primitive, …) and returns tagged byte-offset
// spans.  This module extracts names, signatures, and bodies from the
// classified spans and converts them to Vec<Item> for the renderers.

use crate::ir::Item;
use crate::lexer;

use lalrpop_util::lalrpop_mod;
lalrpop_mod!(pub cryptol);

mod branches;
mod decls;
mod post;
mod sections;
#[cfg(test)]
mod tests;
mod text;

use decls::classify_decl;
use post::{attach_docs, group_enums, merge_signatures};
use sections::extract_sections_with_offsets;
use text::{clean_doc_lines, split_doc_and_decl};

// ── Declaration kind (set by the grammar) ───────────────────────────────────

#[derive(Debug, Clone, Copy)]
pub enum DeclKind {
    Type,
    Property,
    Primitive,
    Foreign,
    Newtype,
    Enum,
    Import,
    SigOrBind,
    Parameter,
}

/// A classified declaration span: `(kind, start_byte, end_byte)` over the
/// original source. Defined as a type alias so the LALRPOP grammar (which
/// stringifies its return types into the generated parser) avoids the
/// `clippy::type_complexity` lint on the auto-generated action functions.
pub type DeclSpan = (DeclKind, usize, usize);

/// The grammar's top-level return: optional module name plus the list of
/// classified declaration spans. Aliased for the same reason as
/// [`DeclSpan`].
pub type ProgramReturn = (Option<String>, Vec<DeclSpan>);

// ── Public API ──────────────────────────────────────────────────────────────

pub fn parse(source: &str) -> Vec<Item> {
    let source = source.strip_prefix('\u{FEFF}').unwrap_or(source);
    let source = &source.replace('\r', "");

    let (tokens, block_docs) = lexer::lex(source).expect("lexer error");
    let token_iter = tokens.into_iter().map(|(s, t, e)| Ok((s, t, e)));

    let (mod_name, decls) = cryptol::ProgramParser::new()
        .parse(source, token_iter)
        .unwrap_or_else(|e| panic!("parse error: {e}"));

    let mut items = Vec::new();

    // Determine the start of the first declaration for module-doc detection
    let first_decl_start = decls.iter().map(|(_, s, _)| *s).min();

    if let Some(name) = mod_name {
        // Find the module-level block doc comment: the first block doc comment
        // that appears before any declaration.
        let mod_doc_lines: Vec<String> = block_docs
            .iter()
            .filter(|d| first_decl_start.is_none_or(|fds| d.byte_pos < fds))
            .flat_map(|d| clean_doc_lines(&d.content))
            .collect();

        items.push(Item::Module {
            name,
            doc: mod_doc_lines,
        });
    }

    let sections = extract_sections_with_offsets(source);
    let mut positioned: Vec<(usize, Item)> = sections;

    // Insert block doc comments as CommentBlock items at their byte positions,
    // but skip any that fall within a declaration span (e.g., inside a parameter block)
    // and any that appear before the first declaration (already used as module doc).
    for doc in &block_docs {
        // Skip module-level docs (before first declaration)
        if first_decl_start.is_none_or(|fds| doc.byte_pos < fds) {
            continue;
        }
        let inside_decl = decls
            .iter()
            .any(|(_, ds, de)| doc.byte_pos >= *ds && doc.byte_pos < *de);
        if inside_decl {
            continue;
        }
        let lines = clean_doc_lines(&doc.content);
        if !lines.is_empty() {
            positioned.push((doc.byte_pos, Item::CommentBlock { lines }));
        }
    }

    for (kind, start, end) in &decls {
        let text = &source[(*start).min(source.len())..(*end).min(source.len())];
        let text = text.trim();
        if text.is_empty() {
            continue;
        }

        let (doc_text, decl_text) = split_doc_and_decl(text);
        if let Some(doc) = doc_text {
            let lines = clean_doc_lines(&doc);
            if !lines.is_empty() {
                positioned.push((*start, Item::CommentBlock { lines }));
            }
        }
        let decl_text = decl_text.trim();
        if decl_text.is_empty() {
            continue;
        }

        let mut decl_items = Vec::new();
        classify_decl(*kind, decl_text, &mut decl_items);
        for item in decl_items {
            positioned.push((*start, item));
        }
    }

    positioned.sort_by_key(|(pos, _)| *pos);
    items.extend(positioned.into_iter().map(|(_, item)| item));

    merge_signatures(&mut items);
    group_enums(&mut items);
    attach_docs(&mut items);

    items
}
