// Auto-generate human-readable descriptions from IR items.
//
// These heuristic descriptions fill in when a function or property has no
// doc comment in the Cryptol source.  They are deterministic (no LLM),
// zero-cost, and can always be overridden by writing a `//` comment in the
// source file.

use convert_case::{Case, Casing};

use crate::ir::Branch;

/// Auto-generate a plain-English description for a property that has no doc comment.
/// Returns `None` when no useful description can be inferred.
pub fn auto_describe_property(
    name: &str,
    params: &[String],
    body: &str,
) -> Option<String> {
    let readable = name.to_case(Case::Lower);
    let rhs = prop_rhs_flat(body);

    // Pattern: `expr == True` or `expr == False` or `expr != value`
    if rhs.contains("==>") {
        // Implication chain: "given preconditions, asserts that ..."
        let conclusions = rhs.rsplit("==>").next().unwrap_or("").trim();
        if conclusions.contains("== True") || (!conclusions.contains("==") && !conclusions.contains("!=")) {
            return Some(format!(
                "{}: given the stated preconditions, asserts the result holds.",
                capitalize(&readable),
            ));
        } else if conclusions.contains("== False") {
            return Some(format!(
                "{}: given the stated preconditions, asserts the result does not hold.",
                capitalize(&readable),
            ));
        } else if conclusions.contains("!=") {
            return Some(format!(
                "{}: given the stated preconditions, asserts the outcomes are distinct.",
                capitalize(&readable),
            ));
        } else if conclusions.contains("==") {
            return Some(format!(
                "{}: given the stated preconditions, asserts the expected outcome.",
                capitalize(&readable),
            ));
        }
    }

    if rhs.contains("== True") {
        return Some(format!(
            "{}: asserts the condition holds{}.",
            capitalize(&readable),
            for_all_suffix(params),
        ));
    }
    if rhs.contains("== False") {
        return Some(format!(
            "{}: asserts the condition does not hold{}.",
            capitalize(&readable),
            for_all_suffix(params),
        ));
    }
    if rhs.contains("!=") && !rhs.contains("==") {
        return Some(format!(
            "{}: asserts the outcomes are distinct{}.",
            capitalize(&readable),
            for_all_suffix(params),
        ));
    }
    if rhs.contains("==") {
        return Some(format!(
            "{}: asserts the expected outcome{}.",
            capitalize(&readable),
            for_all_suffix(params),
        ));
    }

    // Fallback
    if !params.is_empty() {
        Some(format!(
            "{}: verifies a constraint{}.",
            capitalize(&readable),
            for_all_suffix(params),
        ))
    } else {
        Some(format!(
            "{}: verifies a constraint on the specification.",
            capitalize(&readable),
        ))
    }
}

fn for_all_suffix(params: &[String]) -> String {
    if params.is_empty() {
        String::new()
    } else {
        let list = backtick_list(params);
        format!(" for all {list}")
    }
}

fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
    }
}

/// Flatten the body of a property (everything after the first `=` on the
/// first line) into a single whitespace-normalized string.
fn prop_rhs_flat(body: &str) -> String {
    // Property body may be multi-line; join all lines.
    body.lines()
        .map(|l| l.trim())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Auto-generate a description for a function that has no doc comment.
/// Returns an empty `Vec` when no useful description can be inferred.
pub fn auto_describe_function(
    name: &str,
    signature: &str,
    branches: &[Branch],
    body: &str,
) -> Vec<String> {
    let params = extract_params(body, name);
    let ret = return_type(signature);

    if branches.len() > 1 {
        describe_decision_table(&params, &ret, branches)
    } else if params.len() >= 2 && is_and_chain(body, &params) {
        describe_and_chain(&params)
    } else if params.len() >= 2 && is_or_chain(body, &params) {
        describe_or_chain(&params)
    } else if name.starts_with("is") && ret == "Bit" {
        describe_predicate(name, &params, body)
    } else if is_equality_check(body) && ret == "Bit" {
        describe_equality_check(name, &params)
    } else if is_record_body(body) && ret != "Bit" {
        describe_record_ctor(&ret)
    } else if ret == "Bit" && params.len() == 1 {
        vec![format!(
            "Tests whether `{}` is well-formed.",
            params[0]
        )]
    } else if !params.is_empty() {
        describe_generic(&params, &ret)
    } else {
        vec![]
    }
}

// ── Pattern detection ───────────────────────────────────────────────────────

fn extract_params(body: &str, fn_name: &str) -> Vec<String> {
    let first_line = body.lines().next().unwrap_or("");
    let lhs = match first_line.find('=') {
        Some(idx) => &first_line[..idx],
        None => first_line,
    };
    lhs.split_whitespace()
        .skip_while(|tok| *tok == fn_name || tok.starts_with('(') || tok.starts_with(':'))
        .skip(if lhs.starts_with(fn_name) { 0 } else { 1 })
        .take_while(|tok| {
            // Stop at `=` or type annotations
            !tok.starts_with(':') && *tok != "="
        })
        .map(|s| s.trim_matches(|c: char| c == '(' || c == ')').to_string())
        .filter(|s| !s.is_empty() && s != fn_name)
        .collect()
}

fn return_type(sig: &str) -> String {
    sig.rsplit("->")
        .next()
        .unwrap_or(sig)
        .trim()
        .to_string()
}

/// Flatten the RHS of a binding (everything after the first `=`) into a
/// single whitespace-normalized string.
fn rhs_flat(body: &str) -> String {
    body.split_once('=')
        .map(|(_, rhs)| rhs)
        .unwrap_or("")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn is_and_chain(body: &str, params: &[String]) -> bool {
    let expr = rhs_flat(body);
    let parts: Vec<&str> = expr.split("&&").map(|s| s.trim()).collect();
    parts.len() == params.len()
        && parts
            .iter()
            .all(|p| params.iter().any(|param| param == p))
}

fn is_or_chain(body: &str, params: &[String]) -> bool {
    let expr = rhs_flat(body);
    let parts: Vec<&str> = expr.split("||").map(|s| s.trim()).collect();
    parts.len() == params.len()
        && parts
            .iter()
            .all(|p| params.iter().any(|param| param == p))
}

fn is_equality_check(body: &str) -> bool {
    let rhs = rhs_flat(body);
    // "expr == expr" with no if/then
    rhs.contains("==") && !rhs.contains("if ") && !rhs.contains("then ")
}

fn is_record_body(body: &str) -> bool {
    let rhs = rhs_flat(body);
    // A record constructor starts with `{` (possibly after whitespace).
    // Fields may contain conditional expressions, so don't exclude `if`.
    rhs.starts_with("{ ") && rhs.contains(" = ")
}

// ── Description generators ──────────────────────────────────────────────────

fn describe_decision_table(
    params: &[String],
    ret: &str,
    branches: &[Branch],
) -> Vec<String> {
    let n = branches.len();
    let param_refs = backtick_list(params);
    let ret_desc = humanize_ret(ret);

    let mut desc = format!(
        "Evaluates {n} conditions on {param_refs} in priority order, \
         returning the first applicable {ret_desc}."
    );

    // Add hint about the default branch if present.
    if let Some(last) = branches.last()
        && last.condition.is_none()
    {
        // Strip trailing Cryptol comments from the result text.
        let result = strip_inline_comment(&last.result);
        desc.push_str(&format!(
            " Defaults to `{result}` when no prior condition matches.",
        ));
    }

    vec![desc]
}

fn describe_and_chain(params: &[String]) -> Vec<String> {
    let list = backtick_list(params);
    vec![format!(
        "Returns `True` only when all of {list} are true."
    )]
}

fn describe_or_chain(params: &[String]) -> Vec<String> {
    let list = backtick_list(params);
    vec![format!(
        "Returns `True` when any of {list} is true."
    )]
}

fn describe_predicate(name: &str, params: &[String], body: &str) -> Vec<String> {
    let subject = name
        .strip_prefix("isValid")
        .or_else(|| name.strip_prefix("is"))
        .unwrap_or(name);
    // Use lowercase to avoid the linker accidentally creating type links
    // (e.g. "Request Date" would link "Request" to the Request type alias).
    let readable = subject.to_case(Case::Lower);

    // Try to describe the structure of the check.
    let rhs = rhs_flat(body);
    if rhs.contains("<=") && rhs.contains("&&") {
        vec![format!(
            "Checks whether the {readable} is valid: validates a bounded condition over {}.",
            backtick_list(params),
        )]
    } else if rhs.contains("==") {
        vec![format!(
            "Checks whether the {readable} is valid by comparing the computed and expected values.",
        )]
    } else {
        vec![format!(
            "Checks whether the {readable} is valid for the given inputs.",
        )]
    }
}

fn describe_equality_check(_name: &str, params: &[String]) -> Vec<String> {
    let list = backtick_list(params);
    vec![format!(
        "Compares computed and provided values over {list}, returning `True` on match.",
    )]
}

fn describe_record_ctor(ret: &str) -> Vec<String> {
    let ret_desc = humanize_ret(ret);
    vec![format!(
        "Constructs {ret_desc} from the given inputs.",
    )]
}

fn describe_generic(params: &[String], ret: &str) -> Vec<String> {
    let list = backtick_list(params);
    if ret == "Bit" {
        vec![format!(
            "Evaluates a boolean condition over {list}.",
        )]
    } else if ret.starts_with('(') {
        // Tuple return
        vec![format!(
            "Computes a result tuple from {list}.",
        )]
    } else {
        let ret_desc = humanize_ret(ret);
        vec![format!(
            "Computes {ret_desc} from {list}.",
        )]
    }
}

// ── Type humanization ───────────────────────────────────────────────────────

/// Translate common Cryptol type notation into plain English.
///
/// Examples:
///   `[N][8]`    → "N bytes"
///   `[32][8]`   → "32 bytes"
///   `[N]`       → "a sequence of N bits"
///   `[128]`     → "128 bits"
///   `Bit`       → "a boolean"
///   `Integer`   → "an integer"
///   `(A, B)`    → "a tuple"
///   Other       → returns `None` (caller keeps the raw type)
pub fn humanize_cryptol_type(ty: &str) -> Option<String> {
    let ty = ty.trim();
    if ty == "Bit" {
        return Some("a boolean".into());
    }
    if ty == "Integer" {
        return Some("an integer".into());
    }
    if ty.starts_with('(') && ty.ends_with(')') && ty.contains(',') {
        return Some("a tuple".into());
    }

    // [N][8] → "N bytes"
    if let Some(rest) = ty.strip_prefix('[')
        && let Some((inner, after)) = rest.split_once(']')
    {
        let inner = inner.trim();
        let after = after.trim();
        if after == "[8]" {
            // Whether `inner` is a literal digit string or a type variable,
            // we render the same "<inner> bytes" phrase.
            return Some(format!("{inner} bytes"));
        }
        if after.is_empty() {
            if inner.chars().all(|c| c.is_ascii_digit()) {
                return Some(format!("{inner} bits"));
            } else {
                return Some(format!("a sequence of {inner} bits"));
            }
        }
        // [N][M] for other M
        if let Some(rest2) = after.strip_prefix('[')
            && let Some(m) = rest2.strip_suffix(']')
        {
            let m = m.trim();
            return Some(format!("a sequence of {inner} values, each {m} bits wide"));
        }
    }

    None
}

/// Return a human-readable description of a return type, falling back to
/// backtick-quoted raw type when no translation is available.
fn humanize_ret(ret: &str) -> String {
    humanize_cryptol_type(ret).unwrap_or_else(|| format!("`{ret}`"))
}

// ── Formatting helpers ──────────────────────────────────────────────────────

/// Strip a trailing `// comment` from a Cryptol expression.
fn strip_inline_comment(s: &str) -> String {
    if let Some(idx) = s.find("//") {
        s[..idx].trim().to_string()
    } else {
        s.trim().to_string()
    }
}

fn backtick_list(items: &[String]) -> String {
    let formatted: Vec<String> = items.iter().map(|s| format!("`{s}`")).collect();
    english_list(&formatted)
}

fn english_list(items: &[String]) -> String {
    match items.len() {
        0 => String::new(),
        1 => items[0].clone(),
        2 => format!("{} and {}", items[0], items[1]),
        _ => {
            let last = &items[items.len() - 1];
            let rest = &items[..items.len() - 1];
            format!("{}, and {}", rest.join(", "), last)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::Branch;

    #[test]
    fn and_chain_detected() {
        let body = "authenticate dateValid signatureValid claimsValid =\n  dateValid && signatureValid && claimsValid";
        let sig = "Bit -> Bit -> Bit -> Bit";
        let desc = auto_describe_function("authenticate", sig, &[], body);
        assert!(
            desc[0].contains("all of"),
            "should describe AND chain: {desc:?}"
        );
    }

    #[test]
    fn decision_table_detected() {
        let branches = vec![
            Branch { condition: Some("~ fleetEnabled".into()), result: "PR_Disabled".into() },
            Branch { condition: Some("~ validRequest".into()), result: "PR_BadRequest".into() },
            Branch { condition: None, result: "PR_Succeeded".into() },
        ];
        let body = "provisionKey fleetEnabled validRequest vaultResult keyIsActive =\n  if ...";
        let sig = "Bit -> Bit -> KeyVaultResult -> Bit -> ProvisionResult";
        let desc = auto_describe_function("provisionKey", sig, &branches, body);
        assert!(
            desc[0].contains("3 conditions") && desc[0].contains("ProvisionResult"),
            "should describe decision table: {desc:?}"
        );
    }

    #[test]
    fn predicate_detected() {
        let body = "isValidRequestDate requestTs currentTime windowSeconds =\n  (requestTs <= currentTime) && ((currentTime - requestTs) <= windowSeconds)";
        let sig = "Timestamp -> Timestamp -> Window -> Bit";
        let desc = auto_describe_function("isValidRequestDate", sig, &[], body);
        assert!(
            desc[0].contains("request date"),
            "should mention the subject: {desc:?}"
        );
    }

    #[test]
    fn record_ctor_detected() {
        let body = "getStatus fleetEnabled hasKey keyIsActive keyId =\n  { fleetMode = if fleetEnabled then FM_Enabled else FM_Disabled\n  , hasKey = hasKey }";
        let sig = "Bit -> Bit -> Bit -> UUID -> EnrollmentStatus";
        let desc = auto_describe_function("getStatus", sig, &[], body);
        assert!(
            desc[0].contains("EnrollmentStatus"),
            "should mention return type: {desc:?}"
        );
    }

    #[test]
    fn english_list_formats() {
        assert_eq!(english_list(&[]), "");
        assert_eq!(english_list(&["a".into()]), "a");
        assert_eq!(english_list(&["a".into(), "b".into()]), "a and b");
        assert_eq!(
            english_list(&["a".into(), "b".into(), "c".into()]),
            "a, b, and c"
        );
    }
}
