// Per-keyword declaration parsers. Dispatched from `classify_decl`.

use crate::ir::{Item, ParamKind};

use super::DeclKind;
use super::branches::extract_branches;
use super::text::{
    clean_doc_lines, dedent, extract_params, extract_record_fields, extract_width, find_top_level,
    first_ident, parse_name_list, skip_keyword, split_doc_and_decl, split_private_block,
    split_property_name,
};

pub(super) fn classify_decl(kind: DeclKind, text: &str, items: &mut Vec<Item>) {
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
        } else if !pending_doc.is_empty() {
            let mut merged = std::mem::take(&mut pending_doc);
            merged.push('\n');
            merged.push_str(&chunk);
            chunks.push(merged);
        } else {
            chunks.push(chunk);
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
        let doc: Vec<String> = doc_text.map(|d| clean_doc_lines(&d)).unwrap_or_default();

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
        let lhs = rest[..cp]
            .trim()
            .trim_start_matches('(')
            .trim_end_matches(')')
            .trim();
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
                } else if decl_text.starts_with("property ") || decl_text.starts_with("property\t")
                {
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
