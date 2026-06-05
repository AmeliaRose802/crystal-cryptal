// Small textual helpers used throughout the parser pipeline.

pub(super) fn skip_keyword(text: &str) -> &str {
    text.split_once(|c: char| c.is_whitespace())
        .map(|(_, rest)| rest.trim_start())
        .unwrap_or("")
}

pub(super) fn dedent(text: &str) -> String {
    text.lines()
        .map(|l| l.trim())
        .collect::<Vec<_>>()
        .join("\n")
}

pub(super) fn extract_params(text: &str) -> Vec<String> {
    let text = text.trim();
    if text.is_empty() {
        return Vec::new();
    }
    let mut params = Vec::new();
    let mut depth = 0i32;
    let mut start = None;
    for (i, c) in text.char_indices() {
        match c {
            '(' => {
                if depth == 0 {
                    start = Some(i);
                }
                depth += 1;
            }
            ')' => {
                depth -= 1;
                if depth == 0 {
                    if let Some(s) = start {
                        params.push(text[s..=i].trim().to_string());
                    }
                    start = None;
                }
            }
            _ => {}
        }
    }
    if params.is_empty() {
        // No parens — split by whitespace (simple identifiers)
        text.split_whitespace().map(|w| w.to_string()).collect()
    } else {
        params
    }
}

pub(super) fn find_top_level(text: &str, target: char) -> Option<usize> {
    let mut depth_paren = 0i32;
    let mut depth_bracket = 0i32;
    let mut depth_brace = 0i32;
    for (i, c) in text.char_indices() {
        match c {
            '(' => depth_paren += 1,
            ')' => depth_paren -= 1,
            '[' => depth_bracket += 1,
            ']' => depth_bracket -= 1,
            '{' => depth_brace += 1,
            '}' => depth_brace -= 1,
            _ if c == target && depth_paren == 0 && depth_bracket == 0 && depth_brace == 0 => {
                return Some(i);
            }
            _ => {}
        }
    }
    None
}

pub(super) fn first_ident(text: &str) -> String {
    let text = text.trim();
    if text.starts_with('(')
        && let Some(close) = text.find(')')
    {
        return text[1..close].trim().to_string();
    }
    text.split(|c: char| !c.is_alphanumeric() && c != '_' && c != '\'')
        .next()
        .unwrap_or("")
        .to_string()
}

pub(super) fn parse_name_list(text: &str) -> Vec<String> {
    let mut names = Vec::new();
    for part in text.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        if part.starts_with('(') && part.ends_with(')') {
            names.push(part[1..part.len() - 1].trim().to_string());
        } else {
            let name = first_ident(part);
            if !name.is_empty()
                && name
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_alphabetic() || c == '_')
            {
                let rest = part[name.len()..].trim();
                if rest.is_empty() {
                    names.push(name);
                } else {
                    return Vec::new();
                }
            } else {
                return Vec::new();
            }
        }
    }
    names
}

pub(super) fn split_property_name(name: &str) -> (String, String) {
    if let Some(pos) = name.find('_') {
        (name[..pos].to_string(), name[pos + 1..].to_string())
    } else {
        (name.to_string(), name.to_string())
    }
}

/// Strip Cryptol / C-style comment markers from a doc block, **preserving**
/// blank lines (so paragraph breaks survive into the rendered Markdown) and
/// **preserving indentation that follows the marker** (so byte-layout
/// listings and code snippets keep their alignment).
///
/// Leading and trailing blank lines are trimmed; interior blank lines are
/// kept verbatim. Lines whose only content was a section heading (`4.6 foo`)
/// or a separator (`---------`) are dropped.
pub(crate) fn clean_doc_lines(doc: &str) -> Vec<String> {
    let mut out: Vec<String> = doc
        .lines()
        .filter_map(|line| {
            // Find leading whitespace and the comment marker.
            let trimmed_start = line.trim_start();
            let (after, had_marker) = if let Some(rest) = trimmed_start.strip_prefix("///") {
                (rest, true)
            } else if let Some(rest) = trimmed_start.strip_prefix("//") {
                (rest, true)
            } else if let Some(rest) = trimmed_start.strip_prefix("/**") {
                (rest, true)
            } else if let Some(rest) = trimmed_start.strip_prefix("*/") {
                (rest, true)
            } else if let Some(rest) = trimmed_start.strip_prefix('*') {
                (rest, true)
            } else if trimmed_start.is_empty() {
                ("", true) // blank source line — paragraph break
            } else {
                (trimmed_start, false)
            };
            let after = after.strip_suffix("*/").unwrap_or(after);
            // Strip exactly one space after the marker (idiomatic `// foo`),
            // so any *further* indentation (`//   byte 0 ...`) survives.
            let cleaned = if had_marker {
                after.strip_prefix(' ').unwrap_or(after)
            } else {
                after
            };
            let cleaned = cleaned.trim_end().to_string();
            // Drop separator-only and pure section-number lines so they don't
            // pollute the rendered prose.
            if !cleaned.trim().is_empty()
                && (is_separator_content(&cleaned) || is_section_number_line(&cleaned))
            {
                return None;
            }
            Some(cleaned)
        })
        .collect();

    // Trim leading and trailing blank lines.
    while out.first().is_some_and(|l| l.trim().is_empty()) {
        out.remove(0);
    }
    while out.last().is_some_and(|l| l.trim().is_empty()) {
        out.pop();
    }
    out
}

pub(super) fn split_doc_and_decl(text: &str) -> (Option<String>, &str) {
    let mut doc_end = 0;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty()
            || trimmed.starts_with("/**")
            || trimmed.starts_with("///")
            || trimmed.starts_with("//")
            || trimmed.starts_with(" *")
            || trimmed.starts_with("*/")
        {
            doc_end += line.len() + 1;
        } else {
            break;
        }
    }
    let doc_end = doc_end.min(text.len());
    if doc_end > 0 {
        let doc = text[..doc_end].trim().to_string();
        let rest = &text[doc_end..];
        if doc.is_empty() {
            (None, rest)
        } else {
            (Some(doc), rest)
        }
    } else {
        (None, text)
    }
}

pub(super) fn extract_width(rhs: &str) -> String {
    let text = rhs.trim();
    if text.starts_with('[')
        && let Some(close) = text.find(']')
    {
        return text[1..close].trim().to_string();
    }
    text.to_string()
}

pub(super) fn extract_record_fields(text: &str) -> Vec<(String, String)> {
    let mut fields = Vec::new();
    if let Some(open) = text.find('{')
        && let Some(close) = text.rfind('}')
    {
        let inner = &text[open + 1..close];
        for field_str in inner.split(',') {
            let field_str = field_str.trim();
            if let Some(colon_pos) = field_str.find(':') {
                let name = field_str[..colon_pos].trim().to_string();
                let typ = field_str[colon_pos + 1..].trim().to_string();
                if !name.is_empty() {
                    fields.push((name, typ));
                }
            }
        }
    }
    fields
}

/// Split the body of a `private` block into individual declaration chunks.
/// Declarations are identified by lines at the base indentation level;
/// continuation lines (deeper indentation) are grouped with the preceding decl.
pub(super) fn split_private_block(text: &str) -> Vec<String> {
    let lines: Vec<&str> = text.lines().collect();
    if lines.is_empty() {
        return Vec::new();
    }

    // Find the minimum indentation of non-empty, non-comment-only lines
    // that look like declaration starts (contain `=` or `:` at top level).
    let base_indent = lines
        .iter()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.len() - l.trim_start().len())
        .min()
        .unwrap_or(0);

    let mut chunks: Vec<String> = Vec::new();
    let mut current = String::new();

    for line in &lines {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            if !current.is_empty() {
                current.push('\n');
            }
            continue;
        }
        let indent = line.len() - line.trim_start().len();
        // A new declaration starts at base indentation level
        if indent <= base_indent && !current.is_empty() {
            // Check if this looks like a new declaration (not a `where` continuation)
            let is_continuation = trimmed.starts_with("where")
                || trimmed.starts_with('|')
                || trimmed.starts_with("else");
            if !is_continuation {
                chunks.push(std::mem::take(&mut current));
            }
        }
        if !current.is_empty() {
            current.push('\n');
        }
        current.push_str(line);
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
}

pub(super) fn is_separator_content(line: &str) -> bool {
    let t = line.trim();
    t.chars()
        .all(|c| c == '/' || c == '-' || c == '─' || c == ' ')
        || (t.starts_with("----") && t.contains(':'))
}

/// Returns true if the line is a section-number heading like "4.", "4.1 provisionKey",
/// "4.3 authenticate", or standalone section titles that appear
/// in Cryptol source comments as organizational headers.
pub(super) fn is_section_number_line(line: &str) -> bool {
    let t = line.trim();
    if t.is_empty() {
        return false;
    }
    // Match lines starting with a section number: "4.", "4.1", "4.2 enrollDevice (activate)"
    let first_char = t.chars().next().unwrap();
    if first_char.is_ascii_digit() {
        // Check if it's a section number pattern: digits followed by optional .digits groups
        let rest = t.trim_start_matches(|c: char| c.is_ascii_digit() || c == '.');
        // If what remains after stripping the number prefix is empty or just a short heading
        // (not real documentation), treat it as a section heading.
        if rest.is_empty() {
            return true;
        }
        let rest = rest.trim();
        // Section headings are typically short (function name, maybe with parens)
        // and don't contain sentence-like documentation.
        // Heuristic: if the line after the section number is <= 60 chars and doesn't
        // contain periods (sentences), it's a heading.
        if rest.len() <= 60 && !rest.contains(". ") {
            return true;
        }
    }
    false
}
