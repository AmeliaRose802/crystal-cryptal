// Linker: resolves cross-references between IR nodes.

use std::collections::HashMap;

use crate::ir::Item;

mod call_graph;
mod resolve;
#[cfg(test)]
mod tests;
mod util;

pub use call_graph::function_call_graph;
use util::{category_slug, is_simple_constructor, property_anchor};
pub(crate) use util::{contains_word, sanitize_slug};

pub struct ModuleSpec<'a> {
    pub name: &'a str,
    pub output_prefix: &'a str,
    pub items: &'a [Item],
}

pub struct SymbolTable {
    /// Maps symbol name → (target_file, anchor)
    /// e.g., "FleetMode" → ("types.md", "fleetmode")
    /// e.g., "provisionKey" → ("functions/provisionKey.md", "")
    /// e.g., "P1" → ("properties/key-lifecycle-safety.md", "p1--key-monotonicity")
    pub symbols: HashMap<String, (String, String)>,

    /// Maps function name → Vec<(property_label, property_name, category_file)>
    pub related_properties: HashMap<String, Vec<(String, String, String)>>,

    /// Maps property label → category slug (e.g., "P1" → "key-lifecycle-safety")
    pub property_categories: HashMap<String, String>,
}

impl SymbolTable {
    pub fn build(items: &[Item]) -> Self {
        Self::build_for_module_with_prefix(items, "")
    }

    pub fn build_for_modules(modules: &[ModuleSpec<'_>]) -> Self {
        let mut symbols: HashMap<String, (String, String)> = HashMap::new();
        let mut property_categories: HashMap<String, String> = HashMap::new();
        let mut related_properties: HashMap<String, Vec<(String, String, String)>> = HashMap::new();

        let mut function_names: Vec<String> = Vec::new();
        for module in modules {
            for item in module.items {
                if let Item::Function {
                    name,
                    signature,
                    branches,
                    body,
                    ..
                } = item
                {
                    if !signature.contains("->") && branches.is_empty() {
                        continue;
                    }
                    if is_simple_constructor(name, branches, body) {
                        continue;
                    }
                    function_names.push(name.clone());
                }
            }
        }

        for module in modules {
            Self::collect_module_symbols(
                module.name,
                module.output_prefix,
                module.items,
                &function_names,
                &mut symbols,
                &mut property_categories,
                &mut related_properties,
            );
        }

        Self {
            symbols,
            related_properties,
            property_categories,
        }
    }

    pub fn build_for_module_with_prefix(items: &[Item], output_prefix: &str) -> Self {
        let mut symbols: HashMap<String, (String, String)> = HashMap::new();
        let mut property_categories: HashMap<String, String> = HashMap::new();
        let mut related_properties: HashMap<String, Vec<(String, String, String)>> = HashMap::new();

        // Collect all function names first so we can detect back-links in properties.
        let function_names: Vec<String> = items
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
                    if is_simple_constructor(name, branches, body) {
                        return None;
                    }
                    Some(name.clone())
                }
                _ => None,
            })
            .collect();

        Self::collect_module_symbols(
            "",
            output_prefix,
            items,
            &function_names,
            &mut symbols,
            &mut property_categories,
            &mut related_properties,
        );

        Self {
            symbols,
            related_properties,
            property_categories,
        }
    }

    fn collect_module_symbols(
        module_name: &str,
        output_prefix: &str,
        items: &[Item],
        function_names: &[String],
        symbols: &mut HashMap<String, (String, String)>,
        property_categories: &mut HashMap<String, String>,
        related_properties: &mut HashMap<String, Vec<(String, String, String)>>,
    ) {
        let mut current_category_slug = String::new();
        let prefix = if output_prefix.is_empty() {
            String::new()
        } else {
            format!("{}/", output_prefix.trim_matches('/'))
        };

        for item in items {
            match item {
                Item::Section {
                    level: 3, title, ..
                } => {
                    current_category_slug = category_slug(title);
                }
                Item::TypeAlias { name, .. } => {
                    let anchor = name.to_lowercase();
                    let file = format!("{prefix}types.md");
                    symbols.insert(name.clone(), (file.clone(), anchor.clone()));
                    if !module_name.is_empty() {
                        symbols.insert(
                            format!("{module_name}::{name}"),
                            (file.clone(), anchor.clone()),
                        );
                    }
                }
                Item::EnumGroup {
                    type_name,
                    variants,
                    ..
                } => {
                    let anchor = type_name.to_lowercase();
                    let file = format!("{prefix}types.md");
                    symbols.insert(type_name.clone(), (file.clone(), anchor.clone()));
                    if !module_name.is_empty() {
                        symbols.insert(
                            format!("{module_name}::{type_name}"),
                            (file.clone(), anchor.clone()),
                        );
                    }
                    for v in variants {
                        symbols.insert(v.name.clone(), (file.clone(), anchor.clone()));
                        if !module_name.is_empty() {
                            symbols.insert(
                                format!("{module_name}::{}", v.name),
                                (file.clone(), anchor.clone()),
                            );
                        }
                    }
                }
                Item::RecordType { name, .. } => {
                    let anchor = name.to_lowercase();
                    let file = format!("{prefix}types.md");
                    symbols.insert(name.clone(), (file.clone(), anchor.clone()));
                    if !module_name.is_empty() {
                        symbols.insert(
                            format!("{module_name}::{name}"),
                            (file.clone(), anchor.clone()),
                        );
                    }
                }
                Item::Function {
                    name,
                    signature,
                    branches,
                    body,
                    ..
                } => {
                    if !signature.contains("->") && branches.is_empty() {
                        continue;
                    }
                    if is_simple_constructor(name, branches, body) {
                        continue;
                    }
                    let file = format!("{prefix}functions/{name}.md");
                    symbols.insert(name.clone(), (file.clone(), String::new()));
                    if !module_name.is_empty() {
                        symbols.insert(format!("{module_name}::{name}"), (file, String::new()));
                    }
                }
                Item::Property {
                    label,
                    name,
                    body,
                    doc,
                    ..
                } => {
                    let slug = if current_category_slug.is_empty() {
                        "misc"
                    } else {
                        &current_category_slug
                    };
                    let anchor = property_anchor(label, name);
                    let file = format!("{prefix}properties/{slug}.md");
                    symbols.insert(label.clone(), (file.clone(), anchor.clone()));
                    symbols.insert(name.clone(), (file.clone(), anchor));
                    property_categories.insert(label.clone(), slug.to_string());

                    let all_text = format!("{body} {}", doc.join(" "));
                    for fn_name in function_names {
                        if contains_word(&all_text, fn_name) {
                            related_properties
                                .entry(fn_name.clone())
                                .or_default()
                                .push((label.clone(), name.clone(), file.clone()));
                        }
                    }
                }
                _ => {}
            }
        }
    }

    /// Compute relative path from current_file to target_file.
    pub fn relative_path(from: &str, to: &str) -> String {
        let from_parts: Vec<&str> = from.split('/').collect();
        let to_parts: Vec<&str> = to.split('/').collect();

        // Directory of the "from" file (everything except the filename).
        let from_dir = &from_parts[..from_parts.len() - 1];
        let to_dir = &to_parts[..to_parts.len() - 1];

        // Find common prefix length.
        let common = from_dir
            .iter()
            .zip(to_dir.iter())
            .take_while(|(a, b)| a == b)
            .count();

        let ups = from_dir.len() - common;
        let mut parts: Vec<&str> = vec![".."; ups];
        for &segment in &to_dir[common..] {
            parts.push(segment);
        }
        parts.push(to_parts.last().unwrap());

        parts.join("/")
    }
}
