// Parser: lalrpop grammar for structure, Rust code for semantics.
//
// The grammar (cryptol.lalrpop) classifies declarations by their leading
// keyword (type, property, primitive, …) and returns tagged byte-offset
// spans.  This module extracts names, signatures, and bodies from the
// classified spans and converts them to Vec<Item> for the renderers.

use crate::ir::{Branch, EnumVariant, Item, ParamKind};
use crate::lexer;

use lalrpop_util::lalrpop_mod;
lalrpop_mod!(pub cryptol);

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

// ── Classification dispatch (grammar tells us the kind) ─────────────────────

fn classify_decl(kind: DeclKind, text: &str, items: &mut Vec<Item>) {
    match kind {
        DeclKind::Type => parse_type_decl(text, items),
        DeclKind::Property => parse_property_decl(text, items),
        DeclKind::Primitive | DeclKind::Foreign => parse_prim_or_foreign(text, items),
        DeclKind::Newtype => parse_newtype_decl(text, items),
        DeclKind::Enum => parse_enum_decl(text, items),
        DeclKind::Import => parse_import_decl(text, items),
        DeclKind::SigOrBind => parse_sig_or_bind(text, items),
        DeclKind::Parameter => parse_parameter_block(text, items),
    }
}

// ── Individual declaration parsers ──────────────────────────────────────────

fn parse_parameter_block(text: &str, items: &mut Vec<Item>) {
    let rest = text.strip_prefix("parameter").unwrap_or(text);
    // Split the parameter block into individual declarations
    let raw_chunks = split_private_block(rest);

    // Merge doc-comment-only chunks with the following declaration chunk.
    // split_private_block may put /** ... */ on a separate chunk from "type B : #".
    let mut chunks: Vec<String> = Vec::new();
    let mut pending_doc = String::new();
    for chunk in raw_chunks {
        let trimmed = chunk.trim();
        let is_doc_only = trimmed.lines().all(|l| {
            let lt = l.trim();
            lt.is_empty()
                || lt.starts_with("/**")
                || lt.starts_with("///")
                || lt.starts_with("//")
                || lt.starts_with(" *")
                || lt.starts_with("*")
                || lt.starts_with("*/")
        });
        if is_doc_only {
            if !pending_doc.is_empty() {
                pending_doc.push('\n');
            }
            pending_doc.push_str(&chunk);
        } else {
            if !pending_doc.is_empty() {
                let mut merged = std::mem::take(&mut pending_doc);
                merged.push('\n');
                merged.push_str(&chunk);
                chunks.push(merged);
            } else {
                chunks.push(chunk);
            }
        }
    }
    // If only doc remains with no following decl, discard it
    // (shouldn't normally happen)

    for chunk in &chunks {
        let chunk = chunk.trim();
        if chunk.is_empty() {
            continue;
        }
        // Extract doc comment if present
        let (doc_text, decl_text) = split_doc_and_decl(chunk);
        let doc: Vec<String> = doc_text
            .map(|d| clean_doc_lines(&d))
            .unwrap_or_default();

        let decl_text = decl_text.trim();
        if decl_text.is_empty() {
            continue;
        }

        if decl_text.starts_with("type constraint") {
            // type constraint (fin B, L <= B, ...)
            let rest = decl_text.strip_prefix("type").unwrap_or(decl_text);
            let rest = rest.strip_prefix(" constraint").unwrap_or(rest).trim();
            items.push(Item::ModuleParam {
                name: String::new(),
                kind: ParamKind::Constraint,
                signature: rest.to_string(),
                doc,
            });
        } else if decl_text.starts_with("type ") || decl_text.starts_with("type\t") {
            // type B : #
            let rest = skip_keyword(decl_text);
            let name = first_ident(rest);
            let sig = if let Some(colon_pos) = rest.find(':') {
                rest[colon_pos + 1..].trim().to_string()
            } else {
                String::new()
            };
            items.push(Item::ModuleParam {
                name,
                kind: ParamKind::TypeParam,
                signature: sig,
                doc,
            });
        } else {
            // Value parameter: H : {T} (...) => [T][8] -> [L][8]
            let colon_pos = find_top_level(decl_text, ':');
            if let Some(cp) = colon_pos {
                let name = first_ident(&decl_text[..cp]);
                let sig = decl_text[cp + 1..].trim().to_string();
                items.push(Item::ModuleParam {
                    name,
                    kind: ParamKind::ValueParam,
                    signature: sig,
                    doc,
                });
            }
        }
    }
}

fn parse_type_decl(text: &str, items: &mut Vec<Item>) {
    let rest = skip_keyword(text); // strip "type"
    // Handle "type constraint …"
    let rest = rest
        .strip_prefix("constraint")
        .map(|s| s.trim_start())
        .unwrap_or(rest);

    let eq_pos = match find_top_level(rest, '=') {
        Some(p) => p,
        None => return,
    };
    let name = first_ident(&rest[..eq_pos]);
    if name.is_empty() {
        return;
    }
    let rhs = rest[eq_pos + 1..].trim();

    if rhs.contains('{') {
        items.push(Item::RecordType {
            name,
            fields: extract_record_fields(rhs),
            doc: Vec::new(),
        });
    } else {
        items.push(Item::TypeAlias {
            name,
            width: extract_width(rhs),
            doc: Vec::new(),
        });
    }
}

fn parse_property_decl(text: &str, items: &mut Vec<Item>) {
    let rest = skip_keyword(text).trim(); // strip "property"
    let eq_pos = match find_top_level(rest, '=') {
        Some(p) => p,
        None => {
            let name = first_ident(rest);
            if !name.is_empty() {
                let (label, prop_name) = split_property_name(&name);
                items.push(Item::Property {
                    label,
                    name: prop_name,
                    params: Vec::new(),
                    body: rest.to_string(),
                    doc: Vec::new(),
                    proof_status: None,
                    is_private: false,
                });
            }
            return;
        }
    };

    let lhs = rest[..eq_pos].trim();
    let rhs = rest[eq_pos + 1..].trim();

    let name = first_ident(lhs);
    let params = extract_params(&lhs[name.len()..]);
    let (label, prop_name) = split_property_name(&name);

    items.push(Item::Property {
        label,
        name: prop_name,
        params,
        body: dedent(rhs),
        doc: Vec::new(),
        proof_status: None,
        is_private: false,
    });
}

fn parse_import_decl(text: &str, items: &mut Vec<Item>) {
    let rest = skip_keyword(text).trim(); // strip "import"
    if rest.is_empty() {
        return;
    }

    // Parse trailing "hiding (...)" if present.
    let (head, hiding) = if let Some((prefix, suffix)) = rest.rsplit_once("hiding") {
        let list = suffix
            .trim()
            .trim_start_matches('(')
            .trim_end_matches(')')
            .split(',')
            .map(|s| s.trim().trim_matches('`').to_string())
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>();
        (prefix.trim(), list)
    } else {
        (rest, Vec::new())
    };

    // Parse optional "as Alias".
    let (module_path, qualifier) = if let Some((module, alias)) = head.rsplit_once(" as ") {
        (module.trim().to_string(), Some(alias.trim().to_string()))
    } else {
        (head.trim().to_string(), None)
    };

    if module_path.is_empty() {
        return;
    }

    items.push(Item::Import {
        module_path,
        qualifier,
        hiding,
    });
}

fn parse_prim_or_foreign(text: &str, items: &mut Vec<Item>) {
    let rest = skip_keyword(text).trim(); // strip "primitive" or "foreign"
    // Also strip "type" if present (e.g. "primitive type (/) : …")
    let rest = rest
        .strip_prefix("type")
        .map(|s| s.trim_start())
        .unwrap_or(rest);

    if let Some(cp) = find_top_level(rest, ':') {
        let lhs = rest[..cp].trim().trim_start_matches('(').trim_end_matches(')').trim();
        let name = lhs.split_whitespace().last().unwrap_or("").to_string();
        let sig = rest[cp + 1..].trim().to_string();
        items.push(Item::Function {
            name,
            signature: sig,
            branches: Vec::new(),
            body: String::new(),
            doc: Vec::new(),
            proof_status: None,
            is_private: false,
        });
    }
}

fn parse_newtype_decl(text: &str, items: &mut Vec<Item>) {
    let rest = skip_keyword(text).trim(); // strip "newtype"
    let name = first_ident(rest);
    items.push(Item::RecordType {
        name,
        fields: extract_record_fields(text),
        doc: Vec::new(),
    });
}

fn parse_enum_decl(text: &str, items: &mut Vec<Item>) {
    let rest = skip_keyword(text).trim(); // strip "enum"
    let name = first_ident(rest);
    items.push(Item::TypeAlias {
        name,
        width: String::new(),
        doc: Vec::new(),
    });
}

fn parse_sig_or_bind(text: &str, items: &mut Vec<Item>) {
    // Handle `private` blocks: strip keyword and parse inner declarations
    let text = if text.starts_with("private") {
        // Strip the "private" keyword line, preserving indentation of the rest
        let rest = text.strip_prefix("private").unwrap_or(text);
        // Re-parse each top-level declaration within the private block
        let private_start = items.len();
        for chunk in split_private_block(rest) {
            let chunk = chunk.trim();
            if chunk.is_empty() {
                continue;
            }
            let (doc_text, decl_text) = split_doc_and_decl(chunk);
            if let Some(doc) = doc_text {
                let lines = clean_doc_lines(&doc);
                if !lines.is_empty() {
                    items.push(Item::CommentBlock { lines });
                }
            }
            let decl_text = decl_text.trim();
            if !decl_text.is_empty() {
                // Detect kind by leading keyword
                if decl_text.starts_with("type ") || decl_text.starts_with("type\t") {
                    parse_type_decl(decl_text, items);
                } else if decl_text.starts_with("property ") || decl_text.starts_with("property\t") {
                    parse_property_decl(decl_text, items);
                } else if decl_text.starts_with("primitive ") {
                    parse_prim_or_foreign(decl_text, items);
                } else if decl_text.starts_with("newtype ") {
                    parse_newtype_decl(decl_text, items);
                } else if decl_text.starts_with("enum ") {
                    parse_enum_decl(decl_text, items);
                } else {
                    parse_sig_or_bind(decl_text, items);
                }
            }
        }
        // Mark all Function/Property items added within this private block.
        for item in items.iter_mut().skip(private_start) {
            match item {
                Item::Function { is_private, .. } => *is_private = true,
                Item::Property { is_private, .. } => *is_private = true,
                _ => {}
            }
        }
        return;
    } else {
        text
    };

    let colon_pos = find_top_level(text, ':');
    let eq_pos = find_top_level(text, '=');

    match (colon_pos, eq_pos) {
        (Some(cp), Some(ep)) if cp < ep => {
            // : before = → signature
            let lhs = text[..cp].trim();
            let rhs = text[cp + 1..].trim();
            let names = parse_name_list(lhs);
            if names.is_empty() {
                parse_binding(text, items);
            } else {
                for name in names {
                    items.push(Item::Function {
                        name,
                        signature: rhs.to_string(),
                        branches: Vec::new(),
                        body: String::new(),
                        doc: Vec::new(),
                        proof_status: None,
                        is_private: false,
                    });
                }
            }
        }
        (Some(cp), None) => {
            // Only : → pure signature
            let lhs = text[..cp].trim();
            let rhs = text[cp + 1..].trim();
            let names = parse_name_list(lhs);
            if names.is_empty() {
                parse_binding(text, items);
            } else {
                for name in names {
                    items.push(Item::Function {
                        name,
                        signature: rhs.to_string(),
                        branches: Vec::new(),
                        body: String::new(),
                        doc: Vec::new(),
                        proof_status: None,
                        is_private: false,
                    });
                }
            }
        }
        (_, Some(_)) => parse_binding(text, items),
        (None, None) => {}
    }
}

fn parse_binding(text: &str, items: &mut Vec<Item>) {
    let eq_pos = match find_top_level(text, '=') {
        Some(p) => p,
        None => return,
    };
    // Skip compound operators: =>, ==, !=, <=, >=
    if eq_pos + 1 < text.len() {
        let next = text.as_bytes()[eq_pos + 1];
        if next == b'>' || next == b'=' {
            return;
        }
    }
    if eq_pos > 0 {
        let prev = text.as_bytes()[eq_pos - 1];
        if prev == b'!' || prev == b'<' || prev == b'>' {
            return;
        }
    }

    let lhs = text[..eq_pos].trim();
    let rhs = text[eq_pos + 1..].trim();
    let name = first_ident(lhs);
    if name.is_empty() {
        return;
    }

    items.push(Item::Function {
        name,
        signature: String::new(),
        branches: extract_branches(rhs),
        body: text.to_string(),
        doc: Vec::new(),
        proof_status: None,
        is_private: false,
    });
}

// ── Helpers ─────────────────────────────────────────────────────────────────

fn skip_keyword(text: &str) -> &str {
    text.split_once(|c: char| c.is_whitespace())
        .map(|(_, rest)| rest.trim_start())
        .unwrap_or("")
}

fn dedent(text: &str) -> String {
    text.lines()
        .map(|l| l.trim())
        .collect::<Vec<_>>()
        .join("\n")
}

fn extract_params(text: &str) -> Vec<String> {
    let text = text.trim();
    if text.is_empty() {
        return Vec::new();
    }
    let mut params = Vec::new();
    let mut depth = 0i32;
    let mut start = None;
    for (i, c) in text.char_indices() {
        match c {
            '(' => {
                if depth == 0 {
                    start = Some(i);
                }
                depth += 1;
            }
            ')' => {
                depth -= 1;
                if depth == 0 {
                    if let Some(s) = start {
                        params.push(text[s..=i].trim().to_string());
                    }
                    start = None;
                }
            }
            _ => {}
        }
    }
    if params.is_empty() {
        // No parens — split by whitespace (simple identifiers)
        text.split_whitespace()
            .map(|w| w.to_string())
            .collect()
    } else {
        params
    }
}

fn find_top_level(text: &str, target: char) -> Option<usize> {
    let mut depth_paren = 0i32;
    let mut depth_bracket = 0i32;
    let mut depth_brace = 0i32;
    for (i, c) in text.char_indices() {
        match c {
            '(' => depth_paren += 1,
            ')' => depth_paren -= 1,
            '[' => depth_bracket += 1,
            ']' => depth_bracket -= 1,
            '{' => depth_brace += 1,
            '}' => depth_brace -= 1,
            _ if c == target
                && depth_paren == 0
                && depth_bracket == 0
                && depth_brace == 0 =>
            {
                return Some(i);
            }
            _ => {}
        }
    }
    None
}

fn first_ident(text: &str) -> String {
    let text = text.trim();
    if text.starts_with('(')
        && let Some(close) = text.find(')')
    {
        return text[1..close].trim().to_string();
    }
    text.split(|c: char| !c.is_alphanumeric() && c != '_' && c != '\'')
        .next()
        .unwrap_or("")
        .to_string()
}

fn parse_name_list(text: &str) -> Vec<String> {
    let mut names = Vec::new();
    for part in text.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        if part.starts_with('(') && part.ends_with(')') {
            names.push(part[1..part.len() - 1].trim().to_string());
        } else {
            let name = first_ident(part);
            if !name.is_empty()
                && name
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_alphabetic() || c == '_')
            {
                let rest = part[name.len()..].trim();
                if rest.is_empty() {
                    names.push(name);
                } else {
                    return Vec::new();
                }
            } else {
                return Vec::new();
            }
        }
    }
    names
}

fn split_property_name(name: &str) -> (String, String) {
    if let Some(pos) = name.find('_') {
        (name[..pos].to_string(), name[pos + 1..].to_string())
    } else {
        (name.to_string(), name.to_string())
    }
}

/// Strip Cryptol / C-style comment markers from a doc block, **preserving**
/// blank lines (so paragraph breaks survive into the rendered Markdown) and
/// **preserving indentation that follows the marker** (so byte-layout
/// listings and code snippets keep their alignment).
///
/// Leading and trailing blank lines are trimmed; interior blank lines are
/// kept verbatim. Lines whose only content was a section heading (`4.6 foo`)
/// or a separator (`---------`) are dropped.
pub(crate) fn clean_doc_lines(doc: &str) -> Vec<String> {
    let mut out: Vec<String> = doc
        .lines()
        .filter_map(|line| {
            // Find leading whitespace and the comment marker.
            let trimmed_start = line.trim_start();
            let (after, had_marker) = if let Some(rest) = trimmed_start.strip_prefix("///") {
                (rest, true)
            } else if let Some(rest) = trimmed_start.strip_prefix("//") {
                (rest, true)
            } else if let Some(rest) = trimmed_start.strip_prefix("/**") {
                (rest, true)
            } else if let Some(rest) = trimmed_start.strip_prefix("*/") {
                (rest, true)
            } else if let Some(rest) = trimmed_start.strip_prefix('*') {
                (rest, true)
            } else if trimmed_start.is_empty() {
                ("", true) // blank source line — paragraph break
            } else {
                (trimmed_start, false)
            };
            let after = after.strip_suffix("*/").unwrap_or(after);
            // Strip exactly one space after the marker (idiomatic `// foo`),
            // so any *further* indentation (`//   byte 0 ...`) survives.
            let cleaned = if had_marker {
                after.strip_prefix(' ').unwrap_or(after)
            } else {
                after
            };
            let cleaned = cleaned.trim_end().to_string();
            // Drop separator-only and pure section-number lines so they don't
            // pollute the rendered prose.
            if !cleaned.trim().is_empty()
                && (is_separator_content(&cleaned) || is_section_number_line(&cleaned))
            {
                return None;
            }
            Some(cleaned)
        })
        .collect();

    // Trim leading and trailing blank lines.
    while out.first().is_some_and(|l| l.trim().is_empty()) {
        out.remove(0);
    }
    while out.last().is_some_and(|l| l.trim().is_empty()) {
        out.pop();
    }
    out
}

fn split_doc_and_decl(text: &str) -> (Option<String>, &str) {
    let mut doc_end = 0;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty()
            || trimmed.starts_with("/**")
            || trimmed.starts_with("///")
            || trimmed.starts_with("//")
            || trimmed.starts_with(" *")
            || trimmed.starts_with("*/")
        {
            doc_end += line.len() + 1;
        } else {
            break;
        }
    }
    let doc_end = doc_end.min(text.len());
    if doc_end > 0 {
        let doc = text[..doc_end].trim().to_string();
        let rest = &text[doc_end..];
        if doc.is_empty() {
            (None, rest)
        } else {
            (Some(doc), rest)
        }
    } else {
        (None, text)
    }
}

fn extract_width(rhs: &str) -> String {
    let text = rhs.trim();
    if text.starts_with('[')
        && let Some(close) = text.find(']')
    {
        return text[1..close].trim().to_string();
    }
    text.to_string()
}

fn extract_record_fields(text: &str) -> Vec<(String, String)> {
    let mut fields = Vec::new();
    if let Some(open) = text.find('{')
        && let Some(close) = text.rfind('}')
    {
        let inner = &text[open + 1..close];
        for field_str in inner.split(',') {
            let field_str = field_str.trim();
            if let Some(colon_pos) = field_str.find(':') {
                let name = field_str[..colon_pos].trim().to_string();
                let typ = field_str[colon_pos + 1..].trim().to_string();
                if !name.is_empty() {
                    fields.push((name, typ));
                }
            }
        }
    }
    fields
}

// ── Branch extraction ───────────────────────────────────────────────────────

fn extract_branches(body: &str) -> Vec<Branch> {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    if !trimmed.starts_with("if ") && !trimmed.starts_with("if\n") && !trimmed.contains('\n') {
        return vec![Branch {
            condition: None,
            result: trimmed.to_string(),
        }];
    }

    let mut branches = Vec::new();
    for line in trimmed.lines() {
        let t = line.trim();
        if t.starts_with("if ") {
            if let Some(b) = parse_branch(t, "if ") {
                branches.push(b);
            }
        } else if t.starts_with("| ") || t.starts_with('|') {
            let content = t
                .strip_prefix("| ")
                .or_else(|| t.strip_prefix('|'))
                .unwrap_or(t);
            if let Some(b) = parse_branch(content.trim(), "") {
                branches.push(b);
            }
        } else if t.starts_with("else ") || t == "else" {
            let result = t.strip_prefix("else").unwrap_or("").trim().to_string();
            if !result.is_empty() {
                branches.push(Branch {
                    condition: None,
                    result,
                });
            }
        }
    }

    if branches.is_empty() {
        vec![Branch {
            condition: None,
            result: trimmed.to_string(),
        }]
    } else {
        branches
    }
}

fn parse_branch(line: &str, prefix: &str) -> Option<Branch> {
    let content = line.strip_prefix(prefix).unwrap_or(line).trim();
    let pos = find_word(content, "then")?;
    let cond = content[..pos].trim().to_string();
    let result = content[pos + 4..].trim().to_string();
    Some(Branch {
        condition: Some(cond),
        result,
    })
}

fn find_word(s: &str, word: &str) -> Option<usize> {
    let mut start = 0;
    while let Some(pos) = s[start..].find(word) {
        let abs = start + pos;
        let before_ok = abs == 0 || !s.as_bytes()[abs - 1].is_ascii_alphanumeric();
        let after_ok =
            abs + word.len() >= s.len() || !s.as_bytes()[abs + word.len()].is_ascii_alphanumeric();
        if before_ok && after_ok {
            return Some(abs);
        }
        start = abs + 1;
    }
    None
}

// ── Signature merging ───────────────────────────────────────────────────────

fn merge_signatures(items: &mut Vec<Item>) {
    let mut i = 0;
    while i < items.len() {
        if let Item::Function { name, body, .. } = &items[i]
            && !body.is_empty()
        {
            let fn_name = name.clone();
            let fn_body = body.clone();
            for j in (0..i).rev() {
                if let Item::Function {
                    name: sig_name,
                    body: sig_body,
                    branches: sig_branches,
                    signature: sig,
                    ..
                } = &mut items[j]
                    && *sig_name == fn_name
                    && sig_body.is_empty()
                    && !sig.is_empty()
                {
                    *sig_body = fn_body.clone();
                    *sig_branches = extract_branches(&fn_body);
                    items.remove(i);
                    break;
                }
            }
        }
        i += 1;
    }
}

// ── Enum grouping ───────────────────────────────────────────────────────────

fn group_enums(items: &mut Vec<Item>) {
    let mut result: Vec<Item> = Vec::new();
    let mut i = 0;

    while i < items.len() {
        if let Item::TypeAlias {
            ref name,
            ref width,
            ref doc,
        } = items[i]
        {
            let type_name = name.clone();
            let type_width = width.clone();
            let type_doc = doc.clone();

            let mut variants: Vec<EnumVariant> = Vec::new();
            let mut predicate: Option<String> = None;
            let mut j = i + 1;

            while j < items.len() {
                match &items[j] {
                    Item::Function {
                        name: fn_name,
                        signature: fn_sig,
                        body: fn_body,
                        ..
                    } if (fn_sig.trim() == type_name
                        || body_has_type_annotation(fn_body, &type_name))
                        && !fn_body.is_empty()
                        && fn_name
                            .chars()
                            .next()
                            .is_some_and(|c| c.is_uppercase()) =>
                    {
                        let value = if fn_sig.trim() == type_name {
                            fn_body.clone()
                        } else {
                            extract_variant_value(fn_body, &type_name)
                        };
                        variants.push(EnumVariant {
                            name: fn_name.clone(),
                            value,
                        });
                        j += 1;
                    }
                    Item::Function { name: fn_name, .. }
                        if *fn_name == format!("is{}", type_name) =>
                    {
                        predicate = Some(fn_name.clone());
                        j += 1;
                    }
                    Item::CommentBlock { .. } => {
                        // Only consume comment blocks that belong to *this*
                        // enum group (i.e., that sit between this type's
                        // constructors). If the next non-comment item is a
                        // different type declaration, the comment is that
                        // type's doc and must be left in place.
                        let mut k = j + 1;
                        while k < items.len() {
                            if let Item::CommentBlock { .. } = &items[k] {
                                k += 1;
                            } else {
                                break;
                            }
                        }
                        let belongs_to_this_enum = matches!(
                            items.get(k),
                            Some(Item::Function {
                                name: fn_name,
                                signature: fn_sig,
                                body: fn_body,
                                ..
                            }) if (fn_sig.trim() == type_name
                                || body_has_type_annotation(fn_body, &type_name))
                                && !fn_body.is_empty()
                                && fn_name
                                    .chars()
                                    .next()
                                    .is_some_and(|c| c.is_uppercase())
                        ) || matches!(
                            items.get(k),
                            Some(Item::Function { name: fn_name, .. })
                                if *fn_name == format!("is{}", type_name)
                        );
                        if belongs_to_this_enum {
                            j += 1;
                        } else {
                            break;
                        }
                    }
                    _ => break,
                }
            }

            if !variants.is_empty() {
                if predicate.is_none() {
                    for item in items.iter().skip(j) {
                        if let Item::Function { name: fn_name, .. } = item
                            && *fn_name == format!("is{}", type_name)
                        {
                            predicate = Some(fn_name.clone());
                            break;
                        }
                    }
                }
                result.push(Item::EnumGroup {
                    type_name,
                    width: type_width,
                    variants,
                    predicate,
                    doc: type_doc,
                });
                i = j;
            } else {
                result.push(items[i].clone());
                i += 1;
            }
        } else {
            result.push(items[i].clone());
            i += 1;
        }
    }

    // Remove absorbed predicates and variants
    let absorbed: Vec<String> = result
        .iter()
        .filter_map(|item| {
            if let Item::EnumGroup {
                predicate,
                variants,
                ..
            } = item
            {
                let mut names: Vec<String> = variants.iter().map(|v| v.name.clone()).collect();
                if let Some(p) = predicate {
                    names.push(p.clone());
                }
                Some(names)
            } else {
                None
            }
        })
        .flatten()
        .collect();

    result.retain(|item| {
        if let Item::Function { name, .. } = item {
            !absorbed.contains(name)
        } else {
            true
        }
    });

    *items = result;
}

fn body_has_type_annotation(body: &str, type_name: &str) -> bool {
    body.split(':')
        .next_back()
        .is_some_and(|after_colon| {
            let cleaned = after_colon.trim();
            // Strip trailing inline comments
            let cleaned = if let Some(pos) = cleaned.find("//") {
                cleaned[..pos].trim()
            } else {
                cleaned
            };
            cleaned == type_name
        })
}

fn extract_variant_value(body: &str, type_name: &str) -> String {
    // body is full text like "KV_Ok = 0 : KeyVaultResult"
    // We want just the value: "0"
    if let Some(colon_pos) = body.rfind(':') {
        let after = body[colon_pos + 1..].trim();
        let after_clean = if let Some(pos) = after.find("//") {
            after[..pos].trim()
        } else {
            after
        };
        if after_clean == type_name {
            let before_colon = body[..colon_pos].trim();
            // Strip "Name = " prefix if present
            if let Some(eq_pos) = before_colon.find('=') {
                return before_colon[eq_pos + 1..].trim().to_string();
            }
            return before_colon.to_string();
        }
    }
    body.to_string()
}

// ── Doc attachment ──────────────────────────────────────────────────────────

fn attach_docs(items: &mut Vec<Item>) {
    // First, merge consecutive CommentBlocks into one
    let mut i = 0;
    while i + 1 < items.len() {
        if let (Item::CommentBlock { .. }, Item::CommentBlock { .. }) =
            (&items[i], &items[i + 1])
        {
            if let Item::CommentBlock { lines: next_lines } = items.remove(i + 1)
                && let Item::CommentBlock { lines } = &mut items[i]
            {
                lines.extend(next_lines);
            }
            // Don't increment — check if more follow
        } else {
            i += 1;
        }
    }

    // Now attach each CommentBlock to the following item
    let mut i = 0;
    while i + 1 < items.len() {
        if let Item::CommentBlock { .. } = &items[i] {
            let should_attach = match &items[i + 1] {
                Item::Module { doc, .. }
                | Item::TypeAlias { doc, .. }
                | Item::EnumGroup { doc, .. }
                | Item::RecordType { doc, .. }
                | Item::Function { doc, .. }
                | Item::Property { doc, .. }
                | Item::Section { doc, .. } => doc.is_empty(),
                _ => false,
            };

            if should_attach {
                if let Item::CommentBlock { lines } = items.remove(i) {
                    match &mut items[i] {
                        Item::Module { doc, .. }
                        | Item::TypeAlias { doc, .. }
                        | Item::EnumGroup { doc, .. }
                        | Item::RecordType { doc, .. }
                        | Item::Function { doc, .. }
                        | Item::Property { doc, .. }
                        | Item::Section { doc, .. } => {
                            *doc = lines;
                        }
                        _ => {}
                    }
                }
                continue;
            }
        }
        i += 1;
    }
}

// ── Section extraction from comments ────────────────────────────────────────

fn extract_sections_with_offsets(source: &str) -> Vec<(usize, Item)> {
    let mut result = Vec::new();
    let lines: Vec<&str> = source.lines().collect();
    let mut line_offsets: Vec<usize> = Vec::with_capacity(lines.len());
    let mut offset = 0;
    for line in source.lines() {
        line_offsets.push(offset);
        offset += line.len() + 1;
    }
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i].trim();

        if is_separator_line(line)
            && i + 2 < lines.len()
            && is_comment_line(lines[i + 1])
            && is_separator_line(lines[i + 2].trim())
        {
            let title = strip_comment_prefix(lines[i + 1]).trim().to_string();
            result.push((
                line_offsets[i],
                Item::Section {
                    level: 2,
                    title,
                    doc: Vec::new(),
                },
            ));
            i += 3;
            continue;
        }

        if is_comment_line(line) && is_category_line(line) {
            let title = extract_category_title(line);
            result.push((
                line_offsets[i],
                Item::Section {
                    level: 3,
                    title,
                    doc: Vec::new(),
                },
            ));
            i += 1;
            continue;
        }

        i += 1;
    }
    result
}

fn is_separator_line(line: &str) -> bool {
    let t = line.trim();
    if !t.starts_with("//") {
        return false;
    }
    let inner = t.trim_start_matches('/');
    inner.is_empty() || inner.chars().all(|c| c == '/')
}

fn is_comment_line(line: &str) -> bool {
    line.trim().starts_with("//")
}

fn is_category_line(line: &str) -> bool {
    let inner = strip_comment_prefix(line);
    inner.starts_with("----") || inner.starts_with("── ")
}

fn extract_category_title(line: &str) -> String {
    let inner = strip_comment_prefix(line);
    inner
        .trim_matches(|c: char| c == '-' || c == ' ' || c == '─')
        .trim()
        .to_string()
}

fn strip_comment_prefix(line: &str) -> String {
    let s = line.trim().strip_prefix("//").unwrap_or(line.trim());
    s.strip_prefix(' ').unwrap_or(s).to_string()
}

fn is_separator_content(line: &str) -> bool {
    let t = line.trim();
    t.chars().all(|c| c == '/' || c == '-' || c == '─' || c == ' ')
        || (t.starts_with("----") && t.contains(':'))
}

/// Split the body of a `private` block into individual declaration chunks.
/// Declarations are identified by lines at the base indentation level;
/// continuation lines (deeper indentation) are grouped with the preceding decl.
fn split_private_block(text: &str) -> Vec<String> {
    let lines: Vec<&str> = text.lines().collect();
    if lines.is_empty() {
        return Vec::new();
    }

    // Find the minimum indentation of non-empty, non-comment-only lines
    // that look like declaration starts (contain `=` or `:` at top level).
    let base_indent = lines
        .iter()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.len() - l.trim_start().len())
        .min()
        .unwrap_or(0);

    let mut chunks: Vec<String> = Vec::new();
    let mut current = String::new();

    for line in &lines {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            if !current.is_empty() {
                current.push('\n');
            }
            continue;
        }
        let indent = line.len() - line.trim_start().len();
        // A new declaration starts at base indentation level
        if indent <= base_indent && !current.is_empty() {
            // Check if this looks like a new declaration (not a `where` continuation)
            let is_continuation = trimmed.starts_with("where")
                || trimmed.starts_with('|')
                || trimmed.starts_with("else");
            if !is_continuation {
                chunks.push(std::mem::take(&mut current));
            }
        }
        if !current.is_empty() {
            current.push('\n');
        }
        current.push_str(line);
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
}

/// Returns true if the line is a section-number heading like "4.", "4.1 provisionKey",
/// "4.3 authenticate", or standalone section titles that appear
/// in Cryptol source comments as organizational headers.
fn is_section_number_line(line: &str) -> bool {
    let t = line.trim();
    if t.is_empty() {
        return false;
    }
    // Match lines starting with a section number: "4.", "4.1", "4.2 enrollDevice (activate)"
    let first_char = t.chars().next().unwrap();
    if first_char.is_ascii_digit() {
        // Check if it's a section number pattern: digits followed by optional .digits groups
        let rest = t.trim_start_matches(|c: char| c.is_ascii_digit() || c == '.');
        // If what remains after stripping the number prefix is empty or just a short heading
        // (not real documentation), treat it as a section heading.
        if rest.is_empty() {
            return true;
        }
        let rest = rest.trim();
        // Section headings are typically short (function name, maybe with parens)
        // and don't contain sentence-like documentation.
        // Heuristic: if the line after the section number is <= 60 chars and doesn't
        // contain periods (sentences), it's a heading.
        if rest.len() <= 60 && !rest.contains(". ") {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_import_with_alias_and_hiding() {
        let items = parse(
            "module A where\n\nimport Crypto::Hash as H hiding (internalA, internalB)\n",
        );

        let import = items.into_iter().find_map(|i| match i {
            Item::Import {
                module_path,
                qualifier,
                hiding,
            } => Some((module_path, qualifier, hiding)),
            _ => None,
        });

        let (module_path, qualifier, hiding) = import.expect("import item");
        assert_eq!(module_path, "Crypto::Hash");
        assert_eq!(qualifier.as_deref(), Some("H"));
        assert_eq!(hiding, vec!["internalA".to_string(), "internalB".to_string()]);
    }
}
