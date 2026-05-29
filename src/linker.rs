// Linker: resolves cross-references between IR nodes.

use std::collections::HashMap;

use convert_case::{Case, Casing};
use regex::Regex;

use crate::ir::Item;

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
        let mut symbols: HashMap<String, (String, String)> = HashMap::new();
        let mut property_categories: HashMap<String, String> = HashMap::new();
        let mut related_properties: HashMap<String, Vec<(String, String, String)>> =
            HashMap::new();

        // Collect all function names first so we can detect back-links in properties.
        let function_names: Vec<String> = items
            .iter()
            .filter_map(|item| match item {
                Item::Function { name, .. } => Some(name.clone()),
                _ => None,
            })
            .collect();

        let mut current_category_slug = String::new();

        for item in items {
            match item {
                Item::Section { level: 3, title, .. } => {
                    current_category_slug = category_slug(title);
                }
                Item::TypeAlias { name, .. } => {
                    let anchor = name.to_lowercase();
                    symbols.insert(name.clone(), ("types.md".into(), anchor));
                }
                Item::EnumGroup {
                    type_name, variants, ..
                } => {
                    let anchor = type_name.to_lowercase();
                    symbols.insert(type_name.clone(), ("types.md".into(), anchor.clone()));
                    for v in variants {
                        symbols.insert(v.name.clone(), ("types.md".into(), anchor.clone()));
                    }
                }
                Item::RecordType { name, .. } => {
                    let anchor = name.to_lowercase();
                    symbols.insert(name.clone(), ("types.md".into(), anchor));
                }
                Item::Function { name, .. } => {
                    let file = format!("functions/{name}.md");
                    symbols.insert(name.clone(), (file, String::new()));
                }
                Item::Property {
                    label,
                    name,
                    body,
                    doc,
                    ..
                } => {
                    let slug = &current_category_slug;
                    let anchor = property_anchor(label, name);
                    let file = format!("properties/{slug}.md");
                    symbols.insert(label.clone(), (file.clone(), anchor.clone()));
                    // Also register the full name (e.g., "KeyMonotonicity")
                    symbols.insert(name.clone(), (file.clone(), anchor));
                    property_categories.insert(label.clone(), slug.clone());

                    // Back-link detection: find function names mentioned in body or doc.
                    let all_text = format!("{body} {}", doc.join(" "));
                    for fn_name in &function_names {
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

        Self {
            symbols,
            related_properties,
            property_categories,
        }
    }

    /// Resolve cross-references as anchor-only links for single-file output.
    pub fn resolve_links_single_file(&self, text: &str) -> String {
        let mut syms: Vec<(&str, &(String, String))> = self.symbols.iter()
            .map(|(k, v)| (k.as_str(), v))
            .collect();
        syms.sort_by(|a, b| b.0.len().cmp(&a.0.len()));

        let mut result = text.to_string();
        for (name, (_, anchor)) in &syms {
            let pattern = format!(r"\b{}\b", regex::escape(name));
            let re = match Regex::new(&pattern) {
                Ok(r) => r,
                Err(_) => continue,
            };

            // For functions the anchor is empty in multi-file mode;
            // in single-file mode derive it from the name (lowercased).
            let actual_anchor = if anchor.is_empty() {
                name.to_lowercase()
            } else {
                anchor.clone()
            };

            let link = format!("[{name}](#{actual_anchor})");
            result = re.replace_all(&result, link.as_str()).to_string();
        }
        result
    }

    /// Resolve cross-references in text, generating relative links from current_file.
    pub fn resolve_links(&self, text: &str, current_file: &str) -> String {
        // Sort symbols by length descending so longer names match first.
        let mut syms: Vec<(&str, &(String, String))> = self.symbols.iter()
            .map(|(k, v)| (k.as_str(), v))
            .collect();
        syms.sort_by(|a, b| b.0.len().cmp(&a.0.len()));

        let mut result = text.to_string();
        for (name, (target_file, anchor)) in &syms {
            // Don't self-link.
            if current_file == *target_file {
                continue;
            }

            let pattern = format!(r"\b{}\b", regex::escape(name));
            let re = match Regex::new(&pattern) {
                Ok(r) => r,
                Err(_) => continue,
            };

            let rel = Self::relative_path(current_file, target_file);
            let link = if anchor.is_empty() {
                format!("[{name}]({rel})")
            } else {
                format!("[{name}]({rel}#{anchor})")
            };

            result = re.replace_all(&result, link.as_str()).to_string();
        }

        result
    }

    /// Compute relative path from current_file to target_file.
    fn relative_path(from: &str, to: &str) -> String {
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

/// Derive a category slug from a section title like "Category A: Key Lifecycle Safety".
fn category_slug(title: &str) -> String {
    // Strip "Category X: " prefix if present.
    let payload = if let Some(pos) = title.find(':') {
        title[pos + 1..].trim()
    } else {
        title.trim()
    };
    payload.to_case(Case::Kebab)
}

/// Generate a property anchor from label and name.
/// e.g., "P1" + "KeyMonotonicity" → "p1--key-monotonicity"
fn property_anchor(label: &str, name: &str) -> String {
    let label_lower = label.to_lowercase();
    let name_kebab = name.to_case(Case::Kebab);
    format!("{label_lower}--{name_kebab}")
}

/// Check if `text` contains `word` as a whole word (not part of a larger identifier).
fn contains_word(text: &str, word: &str) -> bool {
    let pattern = format!(r"\b{}\b", regex::escape(word));
    Regex::new(&pattern)
        .map(|re| re.is_match(text))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse;
    use std::fs;

    fn load_items() -> Vec<Item> {
        let src = fs::read_to_string("SDEP.cry").expect("SDEP.cry not found");
        parse(&src)
    }

    #[test]
    fn symbol_table_has_types() {
        let items = load_items();
        let st = SymbolTable::build(&items);

        // Type aliases
        assert_eq!(st.symbols["FleetMode"], ("types.md".into(), "fleetmode".into()));
        assert_eq!(
            st.symbols["ProvisionResult"],
            ("types.md".into(), "provisionresult".into())
        );

        // Enum variants map to parent type anchor
        assert_eq!(st.symbols["FM_Disabled"], ("types.md".into(), "fleetmode".into()));
        assert_eq!(st.symbols["PR_Succeeded"], ("types.md".into(), "provisionresult".into()));

        // Record type
        assert_eq!(
            st.symbols["EnrollmentStatus"],
            ("types.md".into(), "enrollmentstatus".into())
        );
    }

    #[test]
    fn symbol_table_has_functions() {
        let items = load_items();
        let st = SymbolTable::build(&items);

        assert_eq!(
            st.symbols["provisionKey"],
            ("functions/provisionKey.md".into(), "".into())
        );
        assert_eq!(
            st.symbols["enrollDevice"],
            ("functions/enrollDevice.md".into(), "".into())
        );
        assert_eq!(
            st.symbols["enforceAccess"],
            ("functions/enforceAccess.md".into(), "".into())
        );
    }

    #[test]
    fn symbol_table_has_properties() {
        let items = load_items();
        let st = SymbolTable::build(&items);

        assert_eq!(
            st.symbols["P1"],
            (
                "properties/key-lifecycle-safety.md".into(),
                "p1--key-monotonicity".into()
            )
        );
        assert_eq!(st.property_categories["P1"], "key-lifecycle-safety");

        // P6 is in Authentication Security
        assert_eq!(st.property_categories["P6"], "authentication-security");

        // P11 is in Access Control
        assert_eq!(st.property_categories["P11"], "access-control");

        // P15 is in Protocol Liveness
        assert_eq!(st.property_categories["P15"], "protocol-liveness");

        // P19 is in Error Handling
        assert_eq!(st.property_categories["P19"], "error-handling");
    }

    #[test]
    fn resolve_links_in_property_doc() {
        let items = load_items();
        let st = SymbolTable::build(&items);

        // P1 doc mentions enrollDevice — should become a link
        let text = "enrollDevice can never return Succeeded";
        let resolved = st.resolve_links(text, "properties/key-lifecycle-safety.md");
        assert!(
            resolved.contains("[enrollDevice](../functions/enrollDevice.md)"),
            "Expected link to enrollDevice, got: {resolved}"
        );
    }

    #[test]
    fn back_links_for_provision_key() {
        let items = load_items();
        let st = SymbolTable::build(&items);

        let related = st.related_properties.get("provisionKey").expect(
            "provisionKey should have related properties",
        );
        let labels: Vec<&str> = related.iter().map(|(l, _, _)| l.as_str()).collect();

        assert!(labels.contains(&"P2"), "P2 should reference provisionKey");
        assert!(labels.contains(&"P5"), "P5 should reference provisionKey");
        assert!(labels.contains(&"P15"), "P15 should reference provisionKey");
        assert!(labels.contains(&"P20"), "P20 should reference provisionKey");
    }

    #[test]
    fn relative_path_computation() {
        assert_eq!(SymbolTable::relative_path("types.md", "functions/foo.md"), "functions/foo.md");
        assert_eq!(SymbolTable::relative_path("functions/foo.md", "types.md"), "../types.md");
        assert_eq!(
            SymbolTable::relative_path("functions/foo.md", "properties/bar.md"),
            "../properties/bar.md"
        );
        assert_eq!(
            SymbolTable::relative_path("properties/bar.md", "functions/foo.md"),
            "../functions/foo.md"
        );
        assert_eq!(
            SymbolTable::relative_path("properties/bar.md", "types.md"),
            "../types.md"
        );
    }

    #[test]
    fn anchor_generation() {
        assert_eq!(property_anchor("P1", "KeyMonotonicity"), "p1--key-monotonicity");
        assert_eq!(
            property_anchor("P5", "DisabledRejectsAll"),
            "p5--disabled-rejects-all"
        );
        assert_eq!(
            property_anchor("P23", "CanonicalizationInjective"),
            "p23--canonicalization-injective"
        );
    }
}
