// Mermaid diagram renderers.

use std::collections::HashSet;
use std::fmt::Write as FmtWrite;

use crate::ir::{Branch, Item};
use crate::linker::SymbolTable;

fn sanitize_mermaid(text: &str) -> String {
    text.replace('"', "'").replace('#', "&#35;")
}

/// Render the cross-function call graph as a Mermaid diagram.
///
/// Currently unused in the rendered docs (the home-page call-graph block
/// was retired because it didn't pull its weight visually), but kept
/// around so we can resurface it elsewhere without re-deriving the logic.
#[allow(dead_code)]
pub(super) fn render_call_graph_mermaid(
    edges: &[(String, String)],
    function_names: &[String],
    items: &[Item],
) -> String {
    let mut decision_fns: HashSet<String> = HashSet::new();
    let mut stub_fns: HashSet<String> = HashSet::new();
    for item in items {
        if let Item::Function {
            name,
            branches,
            body,
            ..
        } = item
        {
            if branches.is_empty() && !body.contains('=') {
                stub_fns.insert(name.clone());
            } else if branches.len() > 1 {
                decision_fns.insert(name.clone());
            }
        }
    }

    let mut nodes: HashSet<String> = HashSet::new();
    nodes.extend(function_names.iter().cloned());
    for (caller, callee) in edges {
        nodes.insert(caller.clone());
        nodes.insert(callee.clone());
    }
    if nodes.is_empty() {
        return String::new();
    }
    let mut sorted_nodes: Vec<&String> = nodes.iter().collect();
    sorted_nodes.sort();

    let mut out = String::new();
    let _ = writeln!(out, "### Call Graph\n");
    let _ = writeln!(out, "```mermaid");
    let _ = writeln!(out, "graph LR");

    for node in &sorted_nodes {
        if stub_fns.contains(*node) {
            let _ = writeln!(out, "  {node}[\"{node}\"]:::stub");
        } else if decision_fns.contains(*node) {
            let _ = writeln!(out, "  {node}[\"{node}\"]:::decision");
        } else {
            let _ = writeln!(out, "  {node}[\"{node}\"]");
        }
        let _ = writeln!(out, "  click {node} \"functions/{node}.md\" \"{node}\"");
    }

    for (caller, callee) in edges {
        let _ = writeln!(out, "  {caller} --> {callee}");
    }

    let _ = writeln!(
        out,
        "  classDef default fill:#f8fafc,stroke:#475569,stroke-width:1.5px,color:#0f172a"
    );
    let _ = writeln!(
        out,
        "  classDef decision fill:#ecfeff,stroke:#0e7490,stroke-width:1.5px,color:#164e63"
    );
    let _ = writeln!(
        out,
        "  classDef stub fill:#fff7ed,stroke:#c2410c,stroke-width:1.5px,stroke-dasharray: 5 5,color:#7c2d12"
    );
    let _ = writeln!(out, "```\n");

    let has_decision = sorted_nodes.iter().any(|n| decision_fns.contains(*n));
    let has_stub = sorted_nodes.iter().any(|n| stub_fns.contains(*n));
    if has_decision || has_stub {
        let _ = write!(out, "**Key:** ");
        let mut parts = vec!["🔵 function"];
        if has_decision {
            parts.push("🟢 decision");
        }
        if has_stub {
            parts.push("🟠 stub");
        }
        let _ = writeln!(out, "{}\n", parts.join(" · "));
    }

    out
}

pub(super) fn render_flowchart_mermaid(name: &str, branches: &[Branch]) -> Option<String> {
    if branches.len() <= 2 {
        return None;
    }
    let mut out = String::new();
    let _ = writeln!(out, "```mermaid");
    let _ = writeln!(out, "flowchart TD");
    let _ = writeln!(out, "  Start([\"{}\"])", sanitize_mermaid(name));

    let mut cond_idx = 0usize;
    let mut res_idx = 0usize;
    let mut prev = "Start".to_string();
    let mut edge_label: Option<&str> = None;

    for branch in branches {
        if let Some(cond) = &branch.condition {
            let cid = format!("C{cond_idx}");
            cond_idx += 1;
            let clabel = sanitize_mermaid(cond);
            match edge_label {
                Some(label) => {
                    let _ = writeln!(out, "  {prev} -->|{label}| {cid}{{\"{clabel}\"}}");
                }
                None => {
                    let _ = writeln!(out, "  {prev} --> {cid}{{\"{clabel}\"}}");
                }
            }
            if !branch.result.trim().is_empty() {
                let rid = format!("R{res_idx}");
                res_idx += 1;
                let rlabel = sanitize_mermaid(&branch.result);
                let _ = writeln!(out, "  {cid} -->|Yes| {rid}(\"{rlabel}\")");
            }
            prev = cid;
            edge_label = Some("No");
        } else {
            let rid = format!("R{res_idx}");
            res_idx += 1;
            let rlabel = sanitize_mermaid(&branch.result);
            match edge_label {
                Some(label) => {
                    let _ = writeln!(out, "  {prev} -->|{label}| {rid}(\"{rlabel}\")");
                }
                None => {
                    let _ = writeln!(out, "  {prev} --> {rid}(\"{rlabel}\")");
                }
            }
            prev = rid;
            edge_label = None;
        }
    }

    let _ = writeln!(
        out,
        "  classDef default fill:#e8f4fd,stroke:#2196F3,stroke-width:2px,color:#1565C0"
    );
    let _ = writeln!(
        out,
        "  style Start fill:#1565C0,stroke:#0D47A1,color:#fff,stroke-width:2px"
    );
    let _ = writeln!(out, "```");
    Some(out)
}

pub(super) fn render_coverage_map_mermaid(symbols: &SymbolTable, fn_names: &[String]) -> String {
    if symbols.related_properties.is_empty() {
        return String::new();
    }

    let mut edges: Vec<(String, String)> = Vec::new();
    let mut covered: HashSet<String> = HashSet::new();
    let fn_set: HashSet<&str> = fn_names.iter().map(|s| s.as_str()).collect();

    for (fn_name, props) in &symbols.related_properties {
        if !fn_set.contains(fn_name.as_str()) {
            continue;
        }
        for (label, _, _) in props {
            edges.push((label.clone(), fn_name.clone()));
            covered.insert(fn_name.clone());
        }
    }

    edges.sort();

    let uncovered: Vec<&String> = fn_names
        .iter()
        .filter(|n| !covered.contains(n.as_str()))
        .collect();

    if edges.is_empty() && uncovered.is_empty() {
        return String::new();
    }

    let mut prop_nodes: HashSet<String> = HashSet::new();
    let mut func_nodes: HashSet<String> = HashSet::new();
    for (prop, func) in &edges {
        prop_nodes.insert(prop.clone());
        func_nodes.insert(func.clone());
    }
    let mut sorted_props: Vec<&String> = prop_nodes.iter().collect();
    sorted_props.sort();
    let mut sorted_funcs: Vec<&String> = func_nodes.iter().collect();
    sorted_funcs.sort();

    let mut out = String::new();
    let _ = writeln!(out, "### Property Coverage\n");
    let _ = writeln!(out, "```mermaid");
    let _ = writeln!(out, "graph LR");

    for prop in &sorted_props {
        let slug = symbols
            .property_categories
            .get(*prop)
            .cloned()
            .unwrap_or_else(|| "misc".into());
        let _ = writeln!(out, "  {prop}[\"{prop}\"]");
        let _ = writeln!(out, "  click {prop} \"properties/{slug}.md\" \"{prop}\"");
    }
    for func in &sorted_funcs {
        let _ = writeln!(out, "  {func}[\"{func}\"]");
        let _ = writeln!(out, "  click {func} \"functions/{func}.md\" \"{func}\"");
    }

    for (prop, func) in &edges {
        let _ = writeln!(out, "  {prop} --> {func}");
    }
    for func in &uncovered {
        let _ = writeln!(out, "  {func}:::gap");
    }

    let _ = writeln!(
        out,
        "  classDef default fill:#e8f4fd,stroke:#2196F3,stroke-width:2px,color:#1565C0"
    );
    if !uncovered.is_empty() {
        let _ = writeln!(
            out,
            "  classDef gap fill:#fff3e0,stroke:#FF9800,stroke-width:2px,stroke-dasharray: 5 5,color:#E65100"
        );
    }
    let _ = writeln!(out, "```\n");
    out
}
