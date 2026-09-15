mod parse;

use std::fmt::Write as FmtWrite;

use crate::ir::ProofStatus;

use parse::{SawContract, execute_arguments, parse_proof_setup, quoted_fresh_variables};

/// Turn saw-spec-gen's stable step markers into an approachable, auditable
/// account of the assumptions between compiled code and its mathematical model.
pub(super) fn render_saw_explanation(status: &Option<ProofStatus>) -> Option<String> {
    let script = match status {
        Some(ProofStatus::Proven {
            proof_script: Some(script),
            ..
        })
        | Some(ProofStatus::Failed {
            proof_script: Some(script),
            ..
        }) => script,
        _ => return None,
    };
    let setup = parse_proof_setup(script);
    if setup.bitcode.is_none()
        && setup.extern_overrides.is_empty()
        && setup.uninterpreted.is_empty()
    {
        return None;
    }

    let mut out = String::new();
    let _ = writeln!(out, "## How this proof connects to the program\n");
    let _ = writeln!(
        out,
        "This is the proof's **trust boundary**: what SAW read from the compiled program and what behavior it accepted through contracts. The summaries below are generated from the same SAW script that ran the proof; expand any **Exact SAW** panel to audit the original instructions.\n"
    );

    if let Some(bitcode) = setup.bitcode {
        let file = escape_html(bitcode.file);
        let _ = writeln!(out, "### 1. Compiled program loaded\n");
        let _ = writeln!(
            out,
            "SAW loaded <code>{file}</code>, an LLVM bitcode file containing the compiled implementation. This is the program representation SAW analyzed—not a reimplementation of the function.\n"
        );
        render_source_details(
            &mut out,
            "Show the exact bitcode-loading instruction",
            bitcode.source,
        );
    }

    if !setup.extern_overrides.is_empty() {
        let count = setup.extern_overrides.len();
        let plural = if count == 1 { "contract" } else { "contracts" };
        let _ = writeln!(out, "### 2. External runtime contracts\n");
        let _ = writeln!(
            out,
            "The compiled function calls **{count} external runtime {plural}** that SAW cannot execute here (usually operating-system or C++ standard-library code). For this proof, each call is replaced by the contract shown below. **The proof checks code that uses these results, but it does not prove that the real external function obeys the contract.**\n"
        );
        for contract in &setup.extern_overrides {
            render_extern_contract(&mut out, contract);
        }
    }

    if !setup.uninterpreted.is_empty() {
        let count = setup.uninterpreted.len();
        let plural = if count == 1 {
            "primitive"
        } else {
            "primitives"
        };
        let _ = writeln!(out, "### 3. Mathematical primitive contracts\n");
        let _ = writeln!(
            out,
            "The proof treats **{count} {plural}** as a mathematical operation. “Uninterpreted” does not mean random: SAW allows arbitrary valid inputs, then requires the compiled symbol's result to equal the named Cryptol model. What remains trusted is the connection between the real implementation and that model.\n"
        );
        for contract in &setup.uninterpreted {
            render_uninterpreted_contract(&mut out, contract);
        }
    }

    Some(out)
}

fn render_extern_contract(out: &mut String, contract: &SawContract<'_>) {
    let friendly = friendly_override_name(contract.symbol, contract.category);
    let symbol = escape_html(contract.symbol);
    let category = contract
        .category
        .map(category_label)
        .unwrap_or("external helper");
    let _ = writeln!(
        out,
        "<details class=\"proof-contract\"><summary><strong>{friendly}</strong> <small>— trusted {category}</small> <abbr title=\"Exact linked symbol: {symbol}\" aria-label=\"Exact linked symbol: {symbol}\">ⓘ</abbr></summary>\n"
    );
    let _ = writeln!(out, "**What SAW assumes**\n");
    for assumption in extern_assumptions(contract.source) {
        let _ = writeln!(out, "- {assumption}");
    }
    out.push('\n');
    render_source_details(out, "Exact SAW contract", contract.source);
    let _ = writeln!(out, "</details>\n");
}

fn render_uninterpreted_contract(out: &mut String, contract: &SawContract<'_>) {
    let name = escape_html(contract.name);
    let symbol = escape_html(contract.symbol);
    let _ = writeln!(
        out,
        "<details class=\"proof-contract\"><summary><strong><code>{name}</code></strong> <small>— trusted model connection</small> <abbr title=\"Exact linked symbol: {symbol}\" aria-label=\"Exact linked symbol: {symbol}\">ⓘ</abbr></summary>\n"
    );
    let variables: Vec<_> = quoted_fresh_variables(contract.source)
        .into_iter()
        .filter(|(name, _)| !name.ends_with("_after"))
        .collect();
    if variables.is_empty() {
        let _ = writeln!(out, "- SAW allows every input accepted by the contract.");
    } else {
        for (variable, ty) in &variables {
            let _ = writeln!(
                out,
                "- <code>{}</code> is arbitrary: the proof must work for every {} value, not a chosen test case.",
                escape_html(variable),
                friendly_type(ty)
            );
        }
    }
    if contract.source.contains("llvm_alloc_readonly") {
        let _ = writeln!(
            out,
            "- Those values are placed in read-only memory before the compiled function is called."
        );
    }
    let arguments = execute_arguments(contract.source);
    if !arguments.is_empty() {
        let _ = writeln!(
            out,
            "- SAW calls the real linked symbol with {} argument{}.",
            arguments.len(),
            if arguments.len() == 1 { "" } else { "s" }
        );
    }
    if let Some(expression) = cryptol_return_expression(contract.source) {
        let _ = writeln!(
            out,
            "- The return value must equal the Cryptol expression <code>{}</code>. This constrains the result precisely, but assumes the real symbol implements that expression.",
            escape_html(expression)
        );
    }
    out.push('\n');
    render_source_details(out, "Exact SAW contract", contract.source);
    let _ = writeln!(out, "</details>\n");
}

fn extern_assumptions(source: &str) -> Vec<String> {
    let mut assumptions = Vec::new();
    let arguments = execute_arguments(source);
    if !arguments.is_empty() {
        assumptions.push(format!(
            "The external function is called with **{} argument{}**; fresh values and pointers are unconstrained unless another bullet says otherwise.",
            arguments.len(),
            if arguments.len() == 1 { "" } else { "s" }
        ));
    }

    for (name, ty) in quoted_fresh_variables(source) {
        if name.ends_with("_after") {
            assumptions.push(format!(
                "After the call, <code>{}</code> may contain any {} value.",
                escape_html(name.trim_end_matches("_after")),
                friendly_type(ty)
            ));
        }
    }

    if source.contains("llvm_postcond {{ False }}") {
        assumptions.push(
            "The call is assumed **never to return**; execution paths after it are unreachable."
                .into(),
        );
    } else if let Some(return_line) = source.lines().find(|line| line.contains("llvm_return")) {
        if let Some((value, width)) = constant_return(return_line) {
            assumptions.push(format!(
                "It must return the fixed value <code>{}</code> as a {}-bit result.",
                escape_html(value),
                escape_html(width)
            ));
        } else if source.contains("llvm_fresh_pointer") {
            assumptions.push("It may return **any pointer**; this contract does not connect that pointer to an allocated object or to the input pointer.".into());
        } else {
            assumptions.push(
                "Its return value is constrained exactly as shown in the SAW contract below."
                    .into(),
            );
        }
    } else {
        assumptions.push(
            "It returns no value; no behavior beyond the listed memory effects is modeled.".into(),
        );
    }
    assumptions
}

fn friendly_override_name(symbol: &str, category: Option<&str>) -> &'static str {
    if symbol.contains("_Mymtx") {
        "Access the C++ mutex handle"
    } else if symbol.contains("_Verify_ownership_levels") {
        "Check mutex ownership"
    } else if symbol == "_Mtx_lock" || symbol.contains("?lock@") {
        "Lock the mutex"
    } else if symbol == "_Mtx_unlock" || symbol.contains("?unlock@") {
        "Unlock the mutex"
    } else if symbol.contains("_Throw_Cpp_error") {
        "C++ runtime error path"
    } else if category == Some("declare-only") {
        "External function without a body"
    } else {
        "External runtime function"
    }
}

fn category_label(category: &str) -> &str {
    match category {
        "msvc-mutex-helper" => "MSVC mutex contract",
        "declare-only" => "declaration-only contract",
        _ => "runtime contract",
    }
}

fn friendly_type(ty: &str) -> String {
    if let Some(rest) = ty.strip_prefix("llvm_array ")
        && let Some((length, element)) = rest.split_once(' ')
        && let Some(width) = element
            .strip_prefix("(llvm_int ")
            .and_then(|value| value.strip_suffix(')'))
    {
        return if width == "8" {
            format!("{length}-byte array")
        } else {
            format!("array of {length} {width}-bit elements")
        };
    }
    if let Some(width) = ty
        .strip_prefix("llvm_int ")
        .and_then(|value| value.strip_suffix(')'))
    {
        return format!("{width}-bit integer");
    }
    format!("<code>{}</code>", escape_html(ty))
}

fn cryptol_return_expression(source: &str) -> Option<&str> {
    let line = source.lines().find(|line| line.contains("llvm_return"))?;
    let (_, expression) = line.split_once("{{")?;
    let (expression, _) = expression.split_once("}}")?;
    Some(expression.trim())
}

fn constant_return(line: &str) -> Option<(&str, &str)> {
    let (_, expression) = line.split_once("{{")?;
    let (expression, _) = expression.split_once("}}")?;
    let (value, width) = expression.trim().split_once(" : [")?;
    Some((value.trim(), width.trim_end_matches(']').trim()))
}

fn render_source_details(out: &mut String, summary: &str, source: &str) {
    let _ = writeln!(out, "<details><summary>{summary}</summary>\n");
    let fence = "`".repeat(longest_backtick_run(source).max(2) + 1);
    let _ = writeln!(out, "{fence}saw\n{}\n{fence}", source.trim_end());
    let _ = writeln!(
        out,
        "\n<button type=\"button\" class=\"btn btn-default btn-xs\" aria-label=\"Copy exact SAW contract\" onclick=\"navigator.clipboard.writeText(this.previousElementSibling.textContent)\">Copy exact SAW</button>\n\n</details>\n"
    );
}

fn longest_backtick_run(text: &str) -> usize {
    text.split(|character| character != '`')
        .map(str::len)
        .max()
        .unwrap_or(0)
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests;
