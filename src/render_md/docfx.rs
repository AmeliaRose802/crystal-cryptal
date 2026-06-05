// DocFX-specific output: per-module frontmatter and `toc.yml`.

use crate::ir::Item;

use super::util::{category_slug_from_title, strip_category_prefix};

pub(super) fn docfx_frontmatter(uid: &str, title: &str) -> String {
    format!("---\nuid: {uid}\ntitle: {title}\n---\n\n")
}

/// Emit a `toc.yml` for a module's output directory.
pub(super) fn render_docfx_toc(title: &str, items: &[Item]) -> String {
    let has_functions = items.iter().any(|i| matches!(i, Item::Function { .. }));
    let has_types = items.iter().any(|i| {
        matches!(
            i,
            Item::TypeAlias { .. } | Item::EnumGroup { .. } | Item::RecordType { .. }
        )
    });

    let mut cats: Vec<(String, String)> = Vec::new();
    let mut cur_title = String::new();
    let mut cur_slug = String::new();
    for item in items {
        if let Item::Section {
            level: 3,
            title: sec_title,
            ..
        } = item
        {
            cur_title = strip_category_prefix(sec_title);
            cur_slug = category_slug_from_title(sec_title);
        }
        if let Item::Property { label, .. } = item {
            let slug = if cur_slug.is_empty() {
                "misc".to_string()
            } else {
                cur_slug.clone()
            };
            let ttl = if cur_title.is_empty() {
                "Miscellaneous".to_string()
            } else {
                cur_title.clone()
            };
            if !cats.iter().any(|(s, _)| s == &slug) {
                cats.push((slug, ttl));
            }
            let _ = label;
        }
    }

    let mut out = String::new();
    out.push_str(&format!("- name: {title}\n  href: index.md\n"));
    if has_types {
        out.push_str("- name: Types\n  href: types.md\n");
    }
    if has_functions {
        out.push_str("- name: Functions\n  href: functions/index.md\n");
    }
    if !cats.is_empty() {
        out.push_str("- name: Properties\n  items:\n");
        for (slug, cat_title) in &cats {
            out.push_str(&format!(
                "  - name: {cat_title}\n    href: properties/{slug}.md\n"
            ));
        }
    }
    out
}
