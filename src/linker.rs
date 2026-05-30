// Linker: resolves cross-references between IR nodes.

use std::collections::HashMap;

use convert_case::{Case, Casing};
use regex::Regex;

use crate::ir::{Branch, Item};

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
        let mut related_properties: HashMap<String, Vec<(String, String, String)>> =
            HashMap::new();

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
        let mut related_properties: HashMap<String, Vec<(String, String, String)>> =
            HashMap::new();

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
                Item::Section { level: 3, title, .. } => {
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
                    type_name, variants, ..
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
                        symbols.insert(
                            format!("{module_name}::{name}"),
                            (file, String::new()),
                        );
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

    /// Resolve cross-references as anchor-only links for single-file output.
    pub fn resolve_links_single_file(&self, text: &str) -> String {
        let mut syms: Vec<(&str, &(String, String))> = self.symbols.iter()
            .map(|(k, v)| (k.as_str(), v))
            .collect();
        syms.sort_by(|a, b| {
            let a_qual = a.0.contains("::");
            let b_qual = b.0.contains("::");
            a_qual.cmp(&b_qual).then_with(|| b.0.len().cmp(&a.0.len()))
        });

        let mut result = text.to_string();
        for (name, (_, anchor)) in &syms {
            let pattern = symbol_pattern(name);
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
            if name.contains("::") {
                result = re.replace_all(&result, link.as_str()).to_string();
            } else {
                result = re
                    .replace_all(&result, |caps: &regex::Captures<'_>| {
                        format!("{}{}", &caps[1], link)
                    })
                    .to_string();
            }
        }
        result
    }

    /// Resolve cross-references in text, generating relative links from current_file.
    pub fn resolve_links(&self, text: &str, current_file: &str) -> String {
        // Sort symbols by length descending so longer names match first.
        let mut syms: Vec<(&str, &(String, String))> = self.symbols.iter()
            .map(|(k, v)| (k.as_str(), v))
            .collect();
        syms.sort_by(|a, b| {
            let a_qual = a.0.contains("::");
            let b_qual = b.0.contains("::");
            a_qual.cmp(&b_qual).then_with(|| b.0.len().cmp(&a.0.len()))
        });

        let mut result = text.to_string();
        for (name, (target_file, anchor)) in &syms {
            // Don't self-link.
            if current_file == *target_file {
                continue;
            }

            let pattern = symbol_pattern(name);
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

            if name.contains("::") {
                result = re.replace_all(&result, link.as_str()).to_string();
            } else {
                result = re
                    .replace_all(&result, |caps: &regex::Captures<'_>| {
                        format!("{}{}", &caps[1], link)
                    })
                    .to_string();
            }
        }

        result
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

/// Keep linker behavior aligned with renderer: simple tuple constructors such as
/// `none`/`some` are not rendered as standalone function pages.
fn is_simple_constructor(name: &str, branches: &[Branch], body: &str) -> bool {
    if name.chars().next().is_some_and(|c| c.is_uppercase()) {
        return false;
    }
    if branches.len() > 1 {
        return false;
    }
    if branches.iter().any(|b| b.condition.is_some()) {
        return false;
    }

    let rhs = body
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.trim())
        .collect::<Vec<_>>()
        .join(" ");
    let rhs = rhs
        .find('=')
        .map(|p| rhs[p + 1..].trim())
        .unwrap_or(&rhs);
    rhs.starts_with('(') && rhs.contains(',') && rhs.len() < 40
}

/// Replace characters that are illegal in Windows filenames and collapse runs of `-`.
///
/// After `to_case(Case::Kebab)` converts a section title to a slug, any character
/// that survived the conversion but is rejected by Windows (e.g. `*` from `void*`)
/// is replaced with `-`.  Consecutive dashes are collapsed and leading/trailing
/// dashes are stripped.  Windows reserved base-names (CON, NUL, COM1, …) get a
/// trailing `_` appended so they remain usable as file-stems.
pub(crate) fn sanitize_slug(s: &str) -> String {
    const INVALID: &[char] = &['*', '?', '<', '>', ':', '|', '"', '\\', '/'];
    const RESERVED: &[&str] = &[
        "CON", "PRN", "AUX", "NUL",
        "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9",
        "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];

    let mut slug = String::with_capacity(s.len());
    let mut last_dash = true; // treat start as dash to trim leading `-`
    for c in s.chars() {
        if INVALID.contains(&c) || c == '-' {
            if !last_dash {
                slug.push('-');
                last_dash = true;
            }
        } else {
            slug.push(c);
            last_dash = false;
        }
    }
    // Trim trailing dash.
    while slug.ends_with('-') {
        slug.pop();
    }
    // Avoid Windows reserved base-names (case-insensitive).
    if RESERVED.iter().any(|&r| slug.eq_ignore_ascii_case(r)) {
        slug.push('_');
    }
    if slug.is_empty() {
        slug.push_str("unnamed");
    }
    slug
}

/// Derive a category slug from a section title like "Category A: Key Lifecycle Safety".
fn category_slug(title: &str) -> String {
    // Strip "Category X: " prefix if present.
    let payload = if let Some(pos) = title.find(':') {
        title[pos + 1..].trim()
    } else {
        title.trim()
    };
    sanitize_slug(&payload.to_case(Case::Kebab))
}

/// Generate a property anchor from label and name.
/// e.g., "P1" + "KeyMonotonicity" → "p1--key-monotonicity"
fn property_anchor(label: &str, name: &str) -> String {
    let label_lower = label.to_lowercase();
    let name_kebab = name.to_case(Case::Kebab);
    format!("{label_lower}--{name_kebab}")
}

fn symbol_pattern(name: &str) -> String {
    let escaped = regex::escape(name);
    if name.contains("::") {
        escaped
    } else {
        format!(r"(^|[^:A-Za-z0-9_'])({escaped})\b")
    }
}

/// Check if `text` contains `word` as a whole word (not part of a larger identifier).
pub(crate) fn contains_word(text: &str, word: &str) -> bool {
    let pattern = format!(r"\b{}\b", regex::escape(word));
    Regex::new(&pattern)
        .map(|re| re.is_match(text))
        .unwrap_or(false)
}

/// Compute function→function cross-reference edges by scanning bodies.
/// Only includes edges where both caller and callee appear in `fn_names`.
pub fn function_call_graph(items: &[Item], fn_names: &[String]) -> Vec<(String, String)> {
    let name_set: std::collections::HashSet<&str> =
        fn_names.iter().map(|s| s.as_str()).collect();
    let mut edges = Vec::new();
    for item in items {
        if let Item::Function {
            name,
            body,
            branches,
            ..
        } = item
        {
            if !name_set.contains(name.as_str()) {
                continue;
            }
            let mut search_text = body.clone();
            for branch in branches {
                if let Some(cond) = &branch.condition {
                    search_text.push(' ');
                    search_text.push_str(cond);
                }
                search_text.push(' ');
                search_text.push_str(&branch.result);
            }
            for other in fn_names {
                if other != name && contains_word(&search_text, other) {
                    edges.push((name.clone(), other.clone()));
                }
            }
        }
    }
    edges.sort();
    edges
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse;
    use std::fs;

    fn load_items() -> Vec<Item> {
        let src = fs::read_to_string("examples/SDEP.cry").expect("SDEP.cry not found");
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

    #[test]
    fn build_for_modules_resolves_cross_module_links() {
        let a_items = parse("module A where\n\nfoo : [8]\nfoo = 0\n");
        let b_items = parse("module B where\n\nbar : [8]\nbar = A::foo\n");
        let specs = vec![
            ModuleSpec {
                name: "A",
                output_prefix: "A",
                items: &a_items,
            },
            ModuleSpec {
                name: "B",
                output_prefix: "B",
                items: &b_items,
            },
        ];

        let st = SymbolTable::build_for_modules(&specs);
        let resolved = st.resolve_links("A::foo", "B/functions/bar.md");
        assert!(
            resolved.contains("[A::foo](../../A/functions/foo.md)"),
            "resolved = {resolved}"
        );
    }

    #[test]
    fn sanitize_slug_strips_windows_invalid_chars() {
        // `*` in "void* pointer-value" must be removed.
        assert_eq!(sanitize_slug("void-pointer-value"), "void-pointer-value");
        assert_eq!(sanitize_slug("void*-pointer-value"), "void-pointer-value");

        // All Windows-invalid chars are replaced and consecutive dashes collapsed.
        assert_eq!(sanitize_slug("a*b?c<d>e:f|g\"h"), "a-b-c-d-e-f-g-h");

        // Back- and forward-slashes are also invalid.
        assert_eq!(sanitize_slug("a\\b/c"), "a-b-c");

        // Leading/trailing dashes are stripped.
        assert_eq!(sanitize_slug("*foo*"), "foo");
        assert_eq!(sanitize_slug("-foo-"), "foo");

        // Purely-invalid input falls back to "unnamed".
        assert_eq!(sanitize_slug("***"), "unnamed");
        assert_eq!(sanitize_slug(""), "unnamed");

        // Windows reserved base-names get a trailing `_`.
        assert_eq!(sanitize_slug("con"), "con_");
        assert_eq!(sanitize_slug("NUL"), "NUL_");
        assert_eq!(sanitize_slug("com1"), "com1_");

        // Normal slugs are unchanged.
        assert_eq!(sanitize_slug("key-lifecycle-safety"), "key-lifecycle-safety");
    }
}
