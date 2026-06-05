// Function→function call-graph extraction from IR.

use crate::ir::Item;

use super::contains_word;

/// Compute function→function cross-reference edges by scanning bodies.
/// Only includes edges where both caller and callee appear in `fn_names`.
pub fn function_call_graph(items: &[Item], fn_names: &[String]) -> Vec<(String, String)> {
    let name_set: std::collections::HashSet<&str> = fn_names.iter().map(|s| s.as_str()).collect();
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
