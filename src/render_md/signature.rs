// Signature parsing & structured rendering.

use std::fmt::Write as FmtWrite;

#[derive(Debug, Default)]
pub(super) struct ParsedSignature {
    pub(super) type_params: Vec<String>,
    pub(super) constraints: Vec<String>,
    pub(super) param_types: Vec<String>,
    pub(super) return_type: Option<String>,
    pub(super) raw: String,
}

pub(super) fn parse_signature(signature: &str) -> ParsedSignature {
    // Strip `//` line comments BEFORE whitespace-normalising so any `(`/`)`/`[`/`]`
    // inside a comment can't desync the bracket counter.
    let stripped: String = signature
        .lines()
        .map(strip_line_comment)
        .collect::<Vec<_>>()
        .join("\n");

    let normalized = stripped
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_string();

    if normalized.is_empty() {
        return ParsedSignature::default();
    }

    let mut parsed = ParsedSignature {
        raw: normalized.clone(),
        ..ParsedSignature::default()
    };

    let (schema_part, core_part) =
        if let Some((left, right)) = split_top_level_once(&normalized, "=>") {
            (Some(left.trim()), right.trim())
        } else {
            (None, normalized.as_str())
        };

    if let Some(schema) = schema_part {
        let mut rest = schema.to_string();
        if rest.starts_with('{')
            && let Some((inside, after)) = extract_group(&rest, '{', '}')
        {
            parsed.type_params = split_top_level_char(&inside, ',')
                .into_iter()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            rest = after.trim().to_string();
        }

        if rest.starts_with('(')
            && let Some((inside, _after)) = extract_group(&rest, '(', ')')
        {
            parsed.constraints = split_top_level_char(&inside, ',')
                .into_iter()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
        }
    }

    let parts = split_top_level_token(core_part, "->");
    if parts.is_empty() {
        return parsed;
    }
    if parts.len() == 1 {
        parsed.return_type = Some(parts[0].trim().to_string());
    } else {
        parsed.param_types = parts[..parts.len() - 1]
            .iter()
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty())
            .collect();
        parsed.return_type = Some(parts[parts.len() - 1].trim().to_string());
    }

    parsed
}

pub(super) fn render_structured_signature<F>(
    out: &mut String,
    parsed: &ParsedSignature,
    param_names: &[String],
    show_raw_signature: bool,
    mut resolve_type: F,
) where
    F: FnMut(&str) -> String,
{
    if parsed.raw.is_empty() {
        return;
    }

    let _ = writeln!(out, "### Signature\n");

    if !parsed.type_params.is_empty() {
        let _ = writeln!(out, "**Type Parameters**");
        for tp in &parsed.type_params {
            let _ = writeln!(out, "- `{}`", tp);
        }
        out.push('\n');
    }

    if !parsed.constraints.is_empty() {
        let _ = writeln!(out, "**Constraints**");
        for c in &parsed.constraints {
            let _ = writeln!(out, "- {}", resolve_type(c));
        }
        out.push('\n');
    }

    let _ = writeln!(out, "**Parameters**");
    if parsed.param_types.is_empty() {
        let _ = writeln!(out, "- *(none)*");
    } else {
        for (idx, param_ty) in parsed.param_types.iter().enumerate() {
            let default_name = format!("arg{}", idx + 1);
            let pname = param_names.get(idx).unwrap_or(&default_name);
            let _ = writeln!(out, "- `{}`: {}", pname, resolve_type(param_ty));
        }
    }
    out.push('\n');

    let _ = writeln!(out, "**Returns**");
    if let Some(ret) = &parsed.return_type {
        let _ = writeln!(out, "- {}\n", resolve_type(ret));
    } else {
        let _ = writeln!(out, "- *(unknown)*\n");
    }

    if show_raw_signature {
        let _ = writeln!(out, "<details><summary>Raw signature</summary>\n");
        let _ = writeln!(out, "`{}`\n", parsed.raw);
        let _ = writeln!(out, "</details>\n");
    }
}

pub(super) fn extract_param_names(body: &str, fn_name: &str) -> Vec<String> {
    let first_line = body.lines().next().unwrap_or("").trim();
    let lhs = first_line
        .split_once('=')
        .map(|(left, _)| left.trim())
        .unwrap_or(first_line);

    let mut tokens = lhs.split_whitespace();
    if tokens.next() != Some(fn_name) {
        return Vec::new();
    }

    tokens
        .map(|tok| tok.trim_matches(|c: char| c == '(' || c == ')' || c == ','))
        .filter(|tok| !tok.is_empty() && *tok != "=")
        .map(|tok| tok.to_string())
        .collect()
}

fn split_top_level_once<'a>(s: &'a str, token: &str) -> Option<(&'a str, &'a str)> {
    if token.is_empty() || s.len() < token.len() {
        return None;
    }

    let bytes = s.as_bytes();
    let token_bytes = token.as_bytes();
    let mut i = 0usize;
    let mut paren = 0i32;
    let mut bracket = 0i32;
    let mut brace = 0i32;

    while i + token_bytes.len() <= bytes.len() {
        match bytes[i] {
            b'(' => paren += 1,
            b')' => paren -= 1,
            b'[' => bracket += 1,
            b']' => bracket -= 1,
            b'{' => brace += 1,
            b'}' => brace -= 1,
            _ => {}
        }

        if paren == 0
            && bracket == 0
            && brace == 0
            && &bytes[i..i + token_bytes.len()] == token_bytes
        {
            let left = &s[..i];
            let right = &s[i + token_bytes.len()..];
            return Some((left, right));
        }
        i += 1;
    }

    None
}

fn strip_line_comment(line: &str) -> &str {
    match line.find("//") {
        Some(idx) => &line[..idx],
        None => line,
    }
}

fn split_top_level_token(s: &str, token: &str) -> Vec<String> {
    if token.is_empty() {
        return vec![s.trim().to_string()];
    }
    let bytes = s.as_bytes();
    let token_bytes = token.as_bytes();
    let mut parts = Vec::new();
    let mut start = 0usize;
    let mut i = 0usize;
    let mut paren = 0i32;
    let mut bracket = 0i32;
    let mut brace = 0i32;

    while i + token_bytes.len() <= bytes.len() {
        match bytes[i] {
            b'(' => paren += 1,
            b')' => paren -= 1,
            b'[' => bracket += 1,
            b']' => bracket -= 1,
            b'{' => brace += 1,
            b'}' => brace -= 1,
            _ => {}
        }

        if paren == 0
            && bracket == 0
            && brace == 0
            && &bytes[i..i + token_bytes.len()] == token_bytes
        {
            parts.push(s[start..i].trim().to_string());
            start = i + token_bytes.len();
            i = start;
            continue;
        }
        i += 1;
    }

    parts.push(s[start..].trim().to_string());
    parts.into_iter().filter(|p| !p.is_empty()).collect()
}

fn split_top_level_char(s: &str, sep: char) -> Vec<String> {
    let mut parts = Vec::new();
    let mut part = String::new();
    let mut paren = 0i32;
    let mut bracket = 0i32;
    let mut brace = 0i32;

    for ch in s.chars() {
        match ch {
            '(' => paren += 1,
            ')' => paren -= 1,
            '[' => bracket += 1,
            ']' => bracket -= 1,
            '{' => brace += 1,
            '}' => brace -= 1,
            _ => {}
        }

        if ch == sep && paren == 0 && bracket == 0 && brace == 0 {
            let trimmed = part.trim();
            if !trimmed.is_empty() {
                parts.push(trimmed.to_string());
            }
            part.clear();
        } else {
            part.push(ch);
        }
    }

    let trimmed = part.trim();
    if !trimmed.is_empty() {
        parts.push(trimmed.to_string());
    }
    parts
}

fn extract_group(s: &str, open: char, close: char) -> Option<(String, String)> {
    let mut chars = s.chars();
    if chars.next()? != open {
        return None;
    }

    let mut depth = 0i32;
    let mut end_idx: Option<usize> = None;
    for (idx, ch) in s.char_indices() {
        if ch == open {
            depth += 1;
        } else if ch == close {
            depth -= 1;
            if depth == 0 {
                end_idx = Some(idx);
                break;
            }
        }
    }

    let end = end_idx?;
    let inside = s[1..end].to_string();
    let after = s[end + close.len_utf8()..].to_string();
    Some((inside, after))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_signature_splits_schema_params_and_return() {
        let sig =
            "{k, n} (width (8 * k) <= B, width (8 * (n + B)) <= B) => [k][8] -> [n][8] -> [L][8]";
        let parsed = parse_signature(sig);

        assert_eq!(parsed.type_params, vec!["k", "n"]);
        assert_eq!(
            parsed.constraints,
            vec!["width (8 * k) <= B", "width (8 * (n + B)) <= B"]
        );
        assert_eq!(parsed.param_types, vec!["[k][8]", "[n][8]"]);
        assert_eq!(parsed.return_type.as_deref(), Some("[L][8]"));
    }

    #[test]
    fn parse_signature_handles_higher_order_parameter() {
        let sig = "{ a } (fin c) => (a -> b) -> [c]a -> [c]b";
        let parsed = parse_signature(sig);

        assert_eq!(parsed.param_types, vec!["(a -> b)", "[c]a"]);
        assert_eq!(parsed.return_type.as_deref(), Some("[c]b"));
    }
}
