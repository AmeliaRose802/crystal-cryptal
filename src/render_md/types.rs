// Renders the `types.md` page for a single module.

use std::collections::HashMap;
use std::fmt::Write as FmtWrite;

use crate::ir::Item;
use crate::linker::SymbolTable;

use super::util::{
    describe_type, is_simple_constructor, prefixed_file, render_type_alias_width, sanitize_type_doc,
};

pub(super) fn render_types(items: &[Item], symbols: &SymbolTable, path_prefix: &str) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "# Types\n");

    let type_names: Vec<String> = items
        .iter()
        .filter_map(|item| match item {
            Item::TypeAlias { name, .. } => Some(name.clone()),
            Item::EnumGroup { type_name, .. } => Some(type_name.clone()),
            Item::RecordType { name, .. } => Some(name.clone()),
            _ => None,
        })
        .collect();

    let functions: Vec<(String, String)> = items
        .iter()
        .filter_map(|item| match item {
            Item::Function {
                name,
                signature,
                branches,
                body,
                ..
            } => {
                if !signature.contains("->") && branches.is_empty() {
                    return None;
                }
                if is_simple_constructor(name, signature, branches, body) {
                    return None;
                }
                Some((name.clone(), signature.clone()))
            }
            _ => None,
        })
        .collect();

    let mut type_to_fns: HashMap<String, Vec<String>> = HashMap::new();
    for (fn_name, sig) in &functions {
        for type_name in &type_names {
            if crate::linker::contains_word(sig, type_name) {
                type_to_fns
                    .entry(type_name.clone())
                    .or_default()
                    .push(fn_name.clone());
            }
        }
    }

    for item in items {
        match item {
            Item::TypeAlias { name, width, doc } => {
                let _ = writeln!(out, "### {name}");
                render_type_alias_width(&mut out, width);
                if let Some(clean_doc) = sanitize_type_doc(doc) {
                    let _ = writeln!(out, "{clean_doc}");
                }
                out.push('\n');
                if let Some(fns) = type_to_fns.get(name) {
                    let links: Vec<String> = fns
                        .iter()
                        .map(|f| format!("[`{f}`](functions/{f}.md)"))
                        .collect();
                    let _ = writeln!(out, "Used by: {}\n", links.join(", "));
                }
            }
            Item::EnumGroup {
                type_name,
                width,
                variants,
                doc,
                ..
            } => {
                let _ = writeln!(out, "### {type_name}");
                if let Some(clean_doc) = sanitize_type_doc(doc) {
                    let _ = writeln!(out, "{clean_doc}\n");
                } else {
                    let n = variants.len();
                    let _ = writeln!(out, "A {width}-bit enumeration with {n} variants.\n");
                }
                let _ = writeln!(out, "| Name | Value | Description |");
                let _ = writeln!(out, "|------|-------|-------------|");
                for v in variants {
                    let _ = writeln!(out, "| `{}` | {} | |", v.name, v.value);
                }
                out.push('\n');
                if let Some(fns) = type_to_fns.get(type_name) {
                    let links: Vec<String> = fns
                        .iter()
                        .map(|f| format!("[`{f}`](functions/{f}.md)"))
                        .collect();
                    let _ = writeln!(out, "Used by: {}\n", links.join(", "));
                }
            }
            Item::RecordType { name, fields, doc } => {
                let _ = writeln!(out, "### {name}");
                if let Some(clean_doc) = sanitize_type_doc(doc) {
                    let _ = writeln!(out, "{clean_doc}\n");
                }
                out.push('\n');
                let _ = writeln!(out, "| Field | Type | Description |");
                let _ = writeln!(out, "|-------|------|-------------|");
                for (fname, ftype) in fields {
                    let current_file = prefixed_file(path_prefix, "types.md");
                    let linked_type = symbols.resolve_links(ftype, &current_file);
                    let desc = describe_type(ftype);
                    let _ = writeln!(out, "| `{fname}` | {linked_type} | {desc} |");
                }
                out.push('\n');
                if let Some(fns) = type_to_fns.get(name) {
                    let links: Vec<String> = fns
                        .iter()
                        .map(|f| format!("[`{f}`](functions/{f}.md)"))
                        .collect();
                    let _ = writeln!(out, "Used by: {}\n", links.join(", "));
                }
            }
            _ => {}
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load_items() -> Vec<Item> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("examples")
            .join("SDEP.cry");
        let src = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("SDEP.cry not found at {}: {e}", path.display()));
        crate::parser::parse(&src)
    }

    #[test]
    fn render_types_has_enums() {
        let items = load_items();
        let symbols = SymbolTable::build(&items);
        let types = render_types(&items, &symbols, "");
        assert!(
            types.contains("### FleetMode"),
            "types should contain FleetMode enum"
        );
        assert!(
            types.contains("`FM_Disabled`"),
            "types should list FM_Disabled"
        );
    }

    #[test]
    fn render_types_has_records() {
        let items = load_items();
        let symbols = SymbolTable::build(&items);
        let types = render_types(&items, &symbols, "");
        assert!(
            types.contains("### EnrollmentStatus"),
            "types should contain EnrollmentStatus record"
        );
    }

    #[test]
    fn render_types_aliases_are_headings() {
        let items = load_items();
        let symbols = SymbolTable::build(&items);
        let types = render_types(&items, &symbols, "");
        assert!(
            types.contains("### UUID"),
            "type aliases should render as headings"
        );
    }
}
