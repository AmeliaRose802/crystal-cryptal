// Post-processing passes that run after every declaration has been parsed:
//   • merge_signatures — pair up `f : Sig` lines with their bodies
//   • group_enums       — collapse a TypeAlias + constructor functions into
//                         a single EnumGroup item
//   • attach_docs       — fold preceding CommentBlock items into the
//                         following item's `doc` field

use crate::ir::{EnumVariant, Item};

use super::branches::extract_branches;

pub(super) fn merge_signatures(items: &mut Vec<Item>) {
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

pub(super) fn group_enums(items: &mut Vec<Item>) {
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
                        && fn_name.chars().next().is_some_and(|c| c.is_uppercase()) =>
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
    body.split(':').next_back().is_some_and(|after_colon| {
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

pub(super) fn attach_docs(items: &mut Vec<Item>) {
    // First, merge consecutive CommentBlocks into one
    let mut i = 0;
    while i + 1 < items.len() {
        if let (Item::CommentBlock { .. }, Item::CommentBlock { .. }) = (&items[i], &items[i + 1]) {
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
