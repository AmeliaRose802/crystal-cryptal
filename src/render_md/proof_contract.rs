use std::fmt::Write as FmtWrite;

use crate::coverage::Ledger;
use crate::ir::{Item, ProofClause, ProofStatus};
use crate::linker::SymbolTable;

pub(super) fn render_implementation_contract(
    name: &str,
    status: &Option<ProofStatus>,
    items: &[Item],
    symbols: &SymbolTable,
    ledger: Option<&Ledger>,
    current_file: &str,
) -> Option<String> {
    let (solver, time, clauses, script, command) = match status {
        Some(ProofStatus::Proven {
            solver,
            time_secs,
            clauses,
            proof_script,
            verify_command,
            ..
        }) if !clauses.is_empty() => (
            solver.as_str(),
            *time_secs,
            clauses,
            proof_script.as_deref(),
            verify_command.as_deref(),
        ),
        _ => return None,
    };

    let entry = ledger.and_then(|ledger| ledger.lookup(name));
    let implementation = entry
        .and_then(|entry| entry.impl_name.as_deref())
        .unwrap_or(name);
    let target = script.and_then(verify_target);
    let assumptions = script.map(trusted_contract_count).unwrap_or(0);
    let mut out = String::new();

    let _ = writeln!(out, "## What was proved\n");
    let duration = time
        .map(|seconds| format!(" in {seconds:.2}s"))
        .unwrap_or_default();
    let assumption_text = match assumptions {
        0 => "no generated trusted contracts".to_string(),
        1 => "1 generated trusted contract".to_string(),
        count => format!("{count} generated trusted contracts"),
    };
    let _ = writeln!(
        out,
        "> ✅ **VERIFIED** — SAW executed the compiled `{implementation}` implementation and discharged **{} observable contract clauses** together with `{solver}`{duration}; the proof uses {assumption_text}.\n",
        clauses.len()
    );

    let _ = writeln!(out, "**Production function**\n");
    if let Some(symbol) = target.or_else(|| entry.and_then(|entry| entry.impl_symbol.as_deref())) {
        let _ = writeln!(
            out,
            "- `{implementation}` <abbr title=\"Exact linked symbol: {}\" aria-label=\"Exact linked symbol: {}\">ⓘ</abbr>",
            escape_html(symbol),
            escape_html(symbol)
        );
    } else {
        let _ = writeln!(out, "- `{implementation}`");
    }
    if let Some(script) = script {
        render_symbolic_setup(&mut out, script);
    }

    render_clause_table(&mut out, clauses, items, symbols, current_file);
    render_clause_details(&mut out, clauses, items, symbols, current_file);
    render_contract_flow(&mut out, implementation, clauses);
    render_proof_sources(
        &mut out,
        entry,
        clauses,
        symbols,
        current_file,
        ledger,
        command,
    );
    Some(out)
}

fn render_symbolic_setup(out: &mut String, script: &str) {
    let variables = fresh_variables(script);
    if !variables.is_empty() {
        let _ = writeln!(out, "\n**Inputs and pre-state**\n");
        for (name, ty) in variables {
            let role = if name.ends_with("_pre") || name == "preBytes" {
                "symbolic bytes representing the value before execution"
            } else {
                "an arbitrary symbolic input"
            };
            let _ = writeln!(out, "- `{name}`: {role} ({})", friendly_type(ty));
        }
    }

    let preconditions: Vec<_> = script
        .lines()
        .filter_map(|line| expression_in(line, "llvm_precond {{"))
        .collect();
    if !preconditions.is_empty() {
        let _ = writeln!(out, "\n**Preconditions**\n");
        let _ = writeln!(
            out,
            "These are restrictions on valid starting states—not conclusions produced by the proof:"
        );
        for condition in preconditions {
            let _ = writeln!(
                out,
                "- <code>{}</code> — {}",
                escape_html(condition),
                explain_precondition(condition)
            );
        }
    }

    if script.contains("// sret:") {
        let _ = writeln!(
            out,
            "\n**ABI note:** the return value uses **sret**: C++ passes a hidden output pointer where the function writes the aggregate result. SAW checks that buffer as the function's return value.\n"
        );
    }
    if let Some((allocated, asserted)) = sret_widths(script)
        && allocated > asserted
    {
        let _ = writeln!(
            out,
            "**Padding scope:** the ABI return buffer is **{allocated} bytes**, while the proof asserts its **{asserted} meaningful bytes**. The remaining **{} padding bytes** carry no modeled value and are deliberately outside the equality claim.\n",
            allocated - asserted
        );
    }
}

fn render_clause_table(
    out: &mut String,
    clauses: &[ProofClause],
    items: &[Item],
    symbols: &SymbolTable,
    current_file: &str,
) {
    let _ = writeln!(out, "\n### Contract clauses checked together\n");
    let _ = writeln!(
        out,
        "| Observable | Cryptol model | SAW assertion | Plain-language meaning |"
    );
    let _ = writeln!(out, "|---|---|---|---|");
    for clause in clauses {
        let model = model_link(&clause.cryptol_fn, symbols, current_file);
        let _ = writeln!(
            out,
            "| {} | {model} | `{}` | {} |",
            observable_label(clause),
            clause.assertion,
            clause_summary(clause, items).replace('|', "\\|")
        );
    }
    let _ = writeln!(
        out,
        "\nThese rows are **parts of one implementation proof** and share one verdict; they are not separate claims that happened to run independently.\n"
    );
}

fn render_clause_details(
    out: &mut String,
    clauses: &[ProofClause],
    items: &[Item],
    symbols: &SymbolTable,
    current_file: &str,
) {
    for clause in clauses {
        let model = model_link(&clause.cryptol_fn, symbols, current_file);
        let _ = writeln!(
            out,
            "<details class=\"proof-clause\"><summary><strong>{}</strong> — <code>{}</code></summary>\n",
            escape_html(&observable_label(clause).replace('`', "")),
            escape_html(&clause.cryptol_fn)
        );
        let _ = writeln!(out, "Model definition: {model}.\n");
        let _ = writeln!(out, "{}\n", clause_summary(clause, items));
        if let Some(body) = function_body(items, &clause.cryptol_fn) {
            let _ = writeln!(out, "```haskell\n{body}\n```\n");
        }
        let _ = writeln!(out, "</details>\n");
    }
}

fn render_contract_flow(out: &mut String, implementation: &str, clauses: &[ProofClause]) {
    let _ = writeln!(out, "### Proof data flow\n");
    let _ = writeln!(out, "```mermaid\nflowchart LR");
    let _ = writeln!(
        out,
        "  Inputs[\"Symbolic inputs + pre-state\"] --> Preconditions{{\"Preconditions hold?\"}}"
    );
    let _ = writeln!(
        out,
        "  Preconditions -->|Yes| Execute[\"Execute compiled {}\"]",
        mermaid_text(implementation)
    );
    for (index, clause) in clauses.iter().enumerate() {
        let _ = writeln!(
            out,
            "  Execute --> C{index}[\"Check {} against {}\"]",
            mermaid_text(&observable_label(clause)),
            mermaid_text(&clause.cryptol_fn)
        );
        let _ = writeln!(out, "  C{index} --> Verified([\"One VERIFIED verdict\"])");
    }
    let _ = writeln!(
        out,
        "  classDef check fill:#ecfeff,stroke:#0e7490,color:#164e63"
    );
    if !clauses.is_empty() {
        let ids = (0..clauses.len())
            .map(|index| format!("C{index}"))
            .collect::<Vec<_>>()
            .join(",");
        let _ = writeln!(out, "  class {ids} check");
    }
    let _ = writeln!(out, "```\n");
}

fn render_proof_sources(
    out: &mut String,
    entry: Option<&crate::coverage::LedgerEntry>,
    clauses: &[ProofClause],
    symbols: &SymbolTable,
    current_file: &str,
    ledger: Option<&Ledger>,
    command: Option<&str>,
) {
    let _ = writeln!(out, "### Proof-relevant sources\n");
    let spec = command.and_then(|command| command_option(command, "--cryptol-spec"));
    let spec_link = spec.as_deref().and_then(|path| github_link(ledger, path));
    if let Some(entry) = entry
        && let Some(file) = entry.impl_file.as_deref()
    {
        let path = repository_path(file);
        let link = github_link(ledger, &path).unwrap_or_else(|| format!("`{path}`"));
        let _ = writeln!(out, "- Production implementation: {link}");
    }
    for clause in clauses {
        let source = spec_link
            .as_deref()
            .map(|link| format!("; Cryptol source: {link}"))
            .unwrap_or_default();
        let _ = writeln!(
            out,
            "- `{}` contract: {}{source}",
            clause.name,
            model_link(&clause.cryptol_fn, symbols, current_file)
        );
    }
    if let Some(command) = command {
        if let Some(spec) = spec {
            let link = github_link(ledger, &spec).unwrap_or_else(|| format!("`{spec}`"));
            let _ = writeln!(out, "- Cryptol source: {link}");
        }
        if let Some(config) = command_option(command, "--config") {
            let link = github_link(ledger, &config).unwrap_or_else(|| format!("`{config}`"));
            let _ = writeln!(out, "- Proof configuration: {link}");
        }
    }
    out.push('\n');
}

fn function_body<'a>(items: &'a [Item], name: &str) -> Option<&'a str> {
    items.iter().find_map(|item| match item {
        Item::Function {
            name: item_name,
            body,
            ..
        } if item_name == name => Some(body.as_str()),
        _ => None,
    })
}

fn model_link(name: &str, symbols: &SymbolTable, current_file: &str) -> String {
    symbols.symbols.get(name).map_or_else(
        || format!("`{name}`"),
        |(target, anchor)| {
            let relative = SymbolTable::relative_path(current_file, target);
            if anchor.is_empty() {
                format!("[`{name}`]({relative})")
            } else {
                format!("[`{name}`]({relative}#{anchor})")
            }
        },
    )
}

fn observable_label(clause: &ProofClause) -> String {
    if clause.name == "return" || clause.assertion == "llvm_return" {
        "Return value".into()
    } else if let Some(region) = clause.region.as_deref() {
        format!("`{region}` post-state")
    } else {
        format!("`{}` post-state", clause.name)
    }
}

fn clause_meaning(clause: &ProofClause) -> String {
    if clause.name == "return" || clause.assertion == "llvm_return" {
        "The value returned by the compiled function must equal this model.".into()
    } else if let Some(region) = clause.region.as_deref() {
        format!(
            "After execution, the mutated `{region}` memory region must equal this model; memory outside the asserted region is not claimed by this clause."
        )
    } else {
        "The post-execution memory selected by this assertion must equal this model.".into()
    }
}

fn clause_summary(clause: &ProofClause, items: &[Item]) -> String {
    items
        .iter()
        .find_map(|item| match item {
            Item::Function { name, doc, .. } if name == &clause.cryptol_fn => doc
                .iter()
                .find(|line| {
                    let line = line.trim();
                    !line.is_empty() && !line.starts_with("@coverage")
                })
                .cloned(),
            _ => None,
        })
        .unwrap_or_else(|| clause_meaning(clause))
}

fn fresh_variables(script: &str) -> Vec<(&str, &str)> {
    script
        .lines()
        .filter_map(|line| {
            let (_, rest) = line.split_once("llvm_fresh_var \"")?;
            let (name, rest) = rest.split_once('"')?;
            let start = rest.find('(')? + 1;
            let ty = rest[start..]
                .trim()
                .trim_end_matches(';')
                .strip_suffix(')')?;
            (!name.ends_with("_after")).then_some((name, ty))
        })
        .collect()
}

fn friendly_type(ty: &str) -> String {
    if let Some(rest) = ty.strip_prefix("llvm_array ")
        && let Some((count, _)) = rest.split_once(' ')
    {
        return format!("{count}-byte memory image");
    }
    ty.replace("llvm_int ", "").to_string() + "-bit value"
}

fn expression_in<'a>(line: &'a str, marker: &str) -> Option<&'a str> {
    let (_, rest) = line.split_once(marker)?;
    Some(
        rest.split_once("}}")
            .map_or(rest, |(value, _)| value)
            .trim(),
    )
}

fn explain_precondition(condition: &str) -> String {
    let plain = condition.replace('`', "");
    if let Some((limit, name)) = plain.split_once(" >= ") {
        return format!("{name} is limited to at most {limit}.");
    }
    if let Some((left, limit)) = plain.split_once(" <= ") {
        if let Some((object, offset)) = left.trim_matches(['(', ')']).split_once(" @ ") {
            return format!(
                "byte {offset} of {object} is restricted to 0 or 1, the canonical C++ Boolean representation."
            );
        }
        return format!("{left} is limited to at most {limit}.");
    }
    "Only starting states satisfying this condition are inside the proof's scope.".into()
}

fn verify_target(script: &str) -> Option<&str> {
    let line = script
        .lines()
        .find(|line| line.trim_start().starts_with("llvm_verify "))?;
    let first = line.find('"')? + 1;
    let rest = &line[first..];
    Some(&rest[..rest.find('"')?])
}

fn trusted_contract_count(script: &str) -> usize {
    script.matches("llvm_unsafe_assume_spec").count()
}

fn sret_widths(script: &str) -> Option<(usize, usize)> {
    let allocation = script
        .lines()
        .find(|line| line.contains("result_ptr <- llvm_alloc"))?;
    let assertion = script
        .lines()
        .find(|line| line.contains("llvm_points_to_at_type result_ptr"))?;
    Some((array_width(allocation)?, array_width(assertion)?))
}

fn array_width(line: &str) -> Option<usize> {
    let (_, rest) = line.split_once("llvm_array ")?;
    rest.split_whitespace().next()?.parse().ok()
}

fn command_option(command: &str, option: &str) -> Option<String> {
    let arguments: Vec<_> = command.split_whitespace().collect();
    arguments.iter().enumerate().find_map(|(index, argument)| {
        argument
            .strip_prefix(&format!("{option}="))
            .map(str::to_string)
            .or_else(|| {
                (*argument == option)
                    .then(|| arguments.get(index + 1).copied())
                    .flatten()
                    .map(str::to_string)
            })
    })
}

fn repository_path(path: &str) -> String {
    let normalized = path.replace('\\', "/");
    ["/cpp/", "/rust/", "/src/", "/tests/"]
        .iter()
        .find_map(|marker| {
            normalized
                .rfind(marker)
                .map(|index| normalized[index + 1..].to_string())
        })
        .unwrap_or_else(|| normalized.trim_start_matches("../").to_string())
}

fn github_link(ledger: Option<&Ledger>, path: &str) -> Option<String> {
    let base = ledger?.source_url_base.as_deref()?;
    Some(format!("[`{path}`]({base}{path})"))
}

fn mermaid_text(value: &str) -> String {
    value
        .replace('"', "'")
        .replace('#', "&#35;")
        .replace('`', "")
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
