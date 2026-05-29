// Parser: builds IR from token stream.
//
// This is a LINE-LEVEL structural parser. It doesn't understand arbitrary
// Cryptol expressions — just enough structure to extract the constructs
// listed in DESIGN.md (modules, types, enums, records, functions,
// properties, sections, doc comments).

use crate::ir::{Branch, EnumVariant, Item};
use crate::lexer::{lex, Token};

// ── public API ──────────────────────────────────────────────────────────────

/// Parse a Cryptol source string into a vector of IR items.
pub fn parse(input: &str) -> Vec<Item> {
    let raw_lines: Vec<&str> = input.lines().collect();
    let tokens = lex(input);
    let token_lines = group_token_lines(&tokens);

    let mut items: Vec<Item> = Vec::new();
    let mut pending_doc: Vec<String> = Vec::new();
    let mut i = 0;

    while i < token_lines.len() {
        let tl = &token_lines[i];

        // ── Comment / section lines ─────────────────────────────────────
        if tl.tokens.len() == 1 && tl.tokens[0].0 == Token::Comment {
            let comment_text = &tl.tokens[0].1;

            // Check for separator line (4+ slashes only)
            if is_separator_line(comment_text) {
                // Look for section pattern: separator / title / separator
                if i + 2 < token_lines.len()
                    && token_lines[i + 2].tokens.len() == 1
                    && token_lines[i + 2].tokens[0].0 == Token::Comment
                    && is_separator_line(&token_lines[i + 2].tokens[0].1)
                {
                    let title_tl = &token_lines[i + 1];
                    if title_tl.tokens.len() == 1
                        && title_tl.tokens[0].0 == Token::Comment
                    {
                        let title =
                            extract_section_title(&title_tl.tokens[0].1);
                        flush_doc(&mut pending_doc, &mut items);
                        items.push(Item::Section {
                            level: 2,
                            title,
                            doc: Vec::new(),
                        });
                        i += 3;
                        continue;
                    }
                }
                // Standalone separator — treat as regular comment
                pending_doc.push(strip_comment_prefix(comment_text));
                i += 1;
                continue;
            }

            // Check for category pattern: // ---- Category X: Title -----
            if is_category_line(comment_text) {
                let title = extract_category_title(comment_text);
                flush_doc(&mut pending_doc, &mut items);
                items.push(Item::Section {
                    level: 3,
                    title,
                    doc: Vec::new(),
                });
                i += 1;
                continue;
            }

            // Regular comment line — accumulate as doc
            pending_doc.push(strip_comment_prefix(comment_text));
            i += 1;
            continue;
        }

        // ── Module declaration ──────────────────────────────────────────
        if matches_pattern(tl, &[Token::Module]) {
            let name = extract_after(tl, Token::Module);
            let doc = take_doc(&mut pending_doc);
            items.push(Item::Module { name, doc });
            i += 1;
            continue;
        }

        // ── Import (skip) ───────────────────────────────────────────────
        if matches_pattern(tl, &[Token::Import]) {
            pending_doc.clear();
            i += 1;
            continue;
        }

        // ── Type declaration ────────────────────────────────────────────
        if matches_pattern(tl, &[Token::Type]) {
            let doc = take_doc(&mut pending_doc);
            let (consumed, item) =
                parse_type_decl(&token_lines, i, &raw_lines, doc);
            items.push(item);
            i += consumed;
            continue;
        }

        // ── Property ────────────────────────────────────────────────────
        if matches_pattern(tl, &[Token::Property]) {
            let doc = take_doc(&mut pending_doc);
            let (consumed, item) =
                parse_property(&token_lines, i, &raw_lines, doc);
            items.push(item);
            i += consumed;
            continue;
        }

        // ── Function signature (Ident : ... -> ...) ─────────────────────
        if is_function_sig(tl) {
            let doc = take_doc(&mut pending_doc);
            let (consumed, item) =
                parse_function(&token_lines, i, &raw_lines, doc);
            items.push(item);
            i += consumed;
            continue;
        }

        // ── Constant definition (IDENT = value : Type at col 0) ─────────
        if is_constant_def(tl, &raw_lines) {
            let doc = take_doc(&mut pending_doc);
            let (name, value, type_ann) = extract_constant(tl);
            // We'll store as a Function for now; the enum-grouping post-pass
            // will absorb constants into EnumGroups.
            items.push(Item::Function {
                name,
                signature: type_ann.clone(),
                branches: Vec::new(),
                body: value,
                doc,
            });
            i += 1;
            continue;
        }

        // ── Function definition at col 0 (name params = body) ───────────
        if is_top_level_def(tl, &raw_lines) {
            let doc = take_doc(&mut pending_doc);
            let (consumed, item) =
                parse_bare_function(&token_lines, i, &raw_lines, doc);
            items.push(item);
            i += consumed;
            continue;
        }

        // ── Skip unrecognised lines ─────────────────────────────────────
        i += 1;
    }

    // Flush any trailing doc
    flush_doc(&mut pending_doc, &mut items);

    // Post-pass: group enums
    group_enums(&mut items);

    // Post-pass: attach doc comments to subsequent items
    attach_docs(&mut items);

    items
}

// ── Token-line grouping ─────────────────────────────────────────────────────

struct TokenLine {
    tokens: Vec<(Token, String)>,
    line_number: usize, // 1-based
}

fn group_token_lines(tokens: &[(Token, String, usize)]) -> Vec<TokenLine> {
    let mut lines: Vec<TokenLine> = Vec::new();
    let mut current_tokens: Vec<(Token, String)> = Vec::new();
    let mut current_line: usize = 1;

    for (tok, text, line) in tokens {
        if *tok == Token::Newline {
            if !current_tokens.is_empty() {
                lines.push(TokenLine {
                    tokens: std::mem::take(&mut current_tokens),
                    line_number: current_line,
                });
            }
            current_line = line + 1;
            continue;
        }
        if current_tokens.is_empty() {
            current_line = *line;
        }
        current_tokens.push((tok.clone(), text.clone()));
    }
    if !current_tokens.is_empty() {
        lines.push(TokenLine {
            tokens: current_tokens,
            line_number: current_line,
        });
    }
    lines
}

// ── Comment / section helpers ───────────────────────────────────────────────

fn is_separator_line(comment: &str) -> bool {
    let trimmed = comment.trim_start_matches('/');
    trimmed.is_empty() || trimmed.chars().all(|c| c == '/')
}

fn is_category_line(comment: &str) -> bool {
    let inner = strip_comment_prefix(comment);
    inner.starts_with("----") || inner.starts_with("── ")
}

fn extract_section_title(comment: &str) -> String {
    let inner = strip_comment_prefix(comment);
    inner.trim().to_string()
}

fn extract_category_title(comment: &str) -> String {
    let inner = strip_comment_prefix(comment);
    // Strip leading/trailing dashes and whitespace
    let trimmed = inner.trim_matches(|c: char| c == '-' || c == ' ' || c == '─');
    trimmed.trim().to_string()
}

fn strip_comment_prefix(comment: &str) -> String {
    let s = comment.strip_prefix("//").unwrap_or(comment);
    // Strip at most one leading space
    if let Some(stripped) = s.strip_prefix(' ') {
        stripped.to_string()
    } else {
        s.to_string()
    }
}

// ── Pattern matching helpers ────────────────────────────────────────────────

fn matches_pattern(tl: &TokenLine, expected: &[Token]) -> bool {
    if tl.tokens.is_empty() {
        return false;
    }
    expected
        .iter()
        .zip(tl.tokens.iter())
        .all(|(e, (t, _))| *e == *t)
}

fn extract_after(tl: &TokenLine, after: Token) -> String {
    let mut found = false;
    let mut parts = Vec::new();
    for (tok, text) in &tl.tokens {
        if !found {
            if *tok == after {
                found = true;
            }
            continue;
        }
        if *tok == Token::Where {
            break;
        }
        parts.push(text.clone());
    }
    parts.join(" ")
}

fn take_doc(pending: &mut Vec<String>) -> Vec<String> {
    std::mem::take(pending)
}

fn flush_doc(pending: &mut Vec<String>, items: &mut Vec<Item>) {
    if !pending.is_empty() {
        items.push(Item::CommentBlock {
            lines: std::mem::take(pending),
        });
    }
}

// ── Detection helpers ───────────────────────────────────────────────────────

/// Function signature: Ident : ... -> ... (at column 0, with Arrow token)
fn is_function_sig(tl: &TokenLine) -> bool {
    if tl.tokens.len() < 3 {
        return false;
    }
    if tl.tokens[0].0 != Token::Ident {
        return false;
    }
    if tl.tokens[1].0 != Token::Colon {
        return false;
    }
    // Must have at least one Arrow somewhere to be a function sig
    tl.tokens.iter().any(|(t, _)| *t == Token::Arrow)
}

/// Constant definition: IDENT = value : TypeName (col 0, all-uppercase or
/// mixed with underscore, has `=` and `:` but no `->`)
fn is_constant_def(tl: &TokenLine, raw_lines: &[&str]) -> bool {
    if tl.tokens.len() < 5 {
        return false;
    }
    if tl.tokens[0].0 != Token::Ident {
        return false;
    }
    // Must be at column 0
    let line_idx = tl.line_number.saturating_sub(1);
    if line_idx < raw_lines.len() {
        let line = raw_lines[line_idx];
        if line.starts_with(' ') || line.starts_with('\t') {
            return false;
        }
    }
    // Must have = and : but no ->
    let has_eq = tl.tokens.iter().any(|(t, _)| *t == Token::Eq);
    let has_colon = tl.tokens.iter().any(|(t, _)| *t == Token::Colon);
    let has_arrow = tl.tokens.iter().any(|(t, _)| *t == Token::Arrow);
    has_eq && has_colon && !has_arrow
}

/// Top-level definition: Ident at col 0 followed by params and = (no : before =)
fn is_top_level_def(tl: &TokenLine, raw_lines: &[&str]) -> bool {
    if tl.tokens.is_empty() {
        return false;
    }
    if tl.tokens[0].0 != Token::Ident {
        return false;
    }
    let line_idx = tl.line_number.saturating_sub(1);
    if line_idx < raw_lines.len() {
        let line = raw_lines[line_idx];
        if line.starts_with(' ') || line.starts_with('\t') {
            return false;
        }
    }
    // Has `=` somewhere
    tl.tokens.iter().any(|(t, _)| *t == Token::Eq)
}

// ── Type declaration parsing ────────────────────────────────────────────────

fn parse_type_decl(
    token_lines: &[TokenLine],
    start: usize,
    raw_lines: &[&str],
    doc: Vec<String>,
) -> (usize, Item) {
    let tl = &token_lines[start];

    // Extract name: type <Name> = ...
    let name = if tl.tokens.len() > 1 && tl.tokens[1].0 == Token::Ident {
        tl.tokens[1].1.clone()
    } else {
        String::new()
    };

    // Check for record type: type Name = \n { ... } or type Name = { ... }
    // Look for LBrace on this line or next
    let has_brace_this_line = tl.tokens.iter().any(|(t, _)| *t == Token::LBrace);
    let has_brace_next_line = start + 1 < token_lines.len()
        && token_lines[start + 1]
            .tokens
            .iter()
            .any(|(t, _)| *t == Token::LBrace);

    if has_brace_this_line || has_brace_next_line {
        return parse_record_type(token_lines, start, raw_lines, name, doc);
    }

    // Type alias: type Name = [N]
    let width = extract_width(tl);
    (
        1,
        Item::TypeAlias {
            name,
            width,
            doc,
        },
    )
}

fn extract_width(tl: &TokenLine) -> String {
    // Find [ ... ] after =
    let mut after_eq = false;
    let mut in_bracket = false;
    let mut width_parts: Vec<String> = Vec::new();

    for (tok, text) in &tl.tokens {
        if *tok == Token::Eq {
            after_eq = true;
            continue;
        }
        if !after_eq {
            continue;
        }
        if *tok == Token::LBracket {
            in_bracket = true;
            continue;
        }
        if *tok == Token::RBracket {
            break;
        }
        if in_bracket {
            width_parts.push(text.clone());
        }
    }
    if width_parts.is_empty() && after_eq {
        // Maybe it's just a bare value after =
        let mut parts = Vec::new();
        let mut found_eq = false;
        for (tok, text) in &tl.tokens {
            if *tok == Token::Eq {
                found_eq = true;
                continue;
            }
            if found_eq {
                parts.push(text.clone());
            }
        }
        return parts.join(" ");
    }
    width_parts.join(" ")
}

fn parse_record_type(
    token_lines: &[TokenLine],
    start: usize,
    _raw_lines: &[&str],
    name: String,
    doc: Vec<String>,
) -> (usize, Item) {
    let mut fields: Vec<(String, String)> = Vec::new();
    let mut consumed = 1;
    let mut brace_depth = 0;
    let mut found_open = false;

    // Collect all tokens from start until matching }
    let mut all_tokens: Vec<(Token, String)> = Vec::new();
    for (j, line) in token_lines.iter().enumerate().skip(start) {
        for (tok, text) in &line.tokens {
            if *tok == Token::LBrace {
                brace_depth += 1;
                found_open = true;
            } else if *tok == Token::RBrace {
                brace_depth -= 1;
            }
            all_tokens.push((tok.clone(), text.clone()));
            if found_open && brace_depth == 0 {
                consumed = j - start + 1;
                break;
            }
        }
        if found_open && brace_depth == 0 {
            break;
        }
        if j > start {
            consumed = j - start + 1;
        }
    }

    // Parse fields from collected tokens: field_name : Type
    let mut i = 0;
    while i < all_tokens.len() {
        if all_tokens[i].0 == Token::Ident {
            // Check for : after ident (possibly with comma before)
            if i + 1 < all_tokens.len() && all_tokens[i + 1].0 == Token::Colon {
                let field_name = all_tokens[i].1.clone();
                let mut field_type_parts: Vec<String> = Vec::new();
                let mut j = i + 2;
                while j < all_tokens.len() {
                    match all_tokens[j].0 {
                        Token::Comma | Token::RBrace => break,
                        Token::LBrace | Token::Type => break,
                        _ => {
                            field_type_parts.push(all_tokens[j].1.clone());
                            j += 1;
                        }
                    }
                }
                let field_type = field_type_parts.join(" ");
                // Skip fields from the `type Name =` part
                if field_name != name {
                    fields.push((field_name, field_type));
                }
                i = j;
                continue;
            }
        }
        i += 1;
    }

    (
        consumed,
        Item::RecordType { name, fields, doc },
    )
}

// ── Constant extraction ─────────────────────────────────────────────────────

fn extract_constant(tl: &TokenLine) -> (String, String, String) {
    let name = tl.tokens[0].1.clone();

    // Find = position
    let eq_pos = tl
        .tokens
        .iter()
        .position(|(t, _)| *t == Token::Eq)
        .unwrap_or(1);

    // Find : position after =
    let colon_pos = tl.tokens[eq_pos..]
        .iter()
        .position(|(t, _)| *t == Token::Colon)
        .map(|p| p + eq_pos)
        .unwrap_or(tl.tokens.len());

    let value = tl.tokens[eq_pos + 1..colon_pos]
        .iter()
        .map(|(_, t)| t.as_str())
        .collect::<Vec<_>>()
        .join(" ");

    let type_ann = tl.tokens[colon_pos + 1..]
        .iter()
        .map(|(_, t)| t.as_str())
        .collect::<Vec<_>>()
        .join(" ");

    (name, value, type_ann)
}

// ── Function parsing ────────────────────────────────────────────────────────

fn parse_function(
    token_lines: &[TokenLine],
    start: usize,
    raw_lines: &[&str],
    doc: Vec<String>,
) -> (usize, Item) {
    let sig_tl = &token_lines[start];
    let name = sig_tl.tokens[0].1.clone();

    // Build signature text from tokens after ':'
    let sig = sig_tl.tokens[2..]
        .iter()
        .map(|(_, t)| t.as_str())
        .collect::<Vec<_>>()
        .join(" ");

    // The signature may continue on the next line(s) if they're indented
    // and contain Arrow tokens
    let mut consumed = 1;
    let mut full_sig = sig.clone();

    // Check for multi-line signature
    let mut j = start + 1;
    while j < token_lines.len() {
        let line_idx = token_lines[j].line_number.saturating_sub(1);
        if line_idx < raw_lines.len() {
            let line = raw_lines[line_idx];
            if line.starts_with(' ') || line.starts_with('\t') {
                // Check if this is a continuation of the signature (has Arrow)
                // or is the body definition line (has name followed by params = ...)
                if token_lines[j]
                    .tokens
                    .iter()
                    .any(|(t, _)| *t == Token::Arrow)
                    && !token_lines[j]
                        .tokens
                        .iter()
                        .any(|(t, _)| *t == Token::Eq)
                {
                    let cont = token_lines[j]
                        .tokens
                        .iter()
                        .map(|(_, t)| t.as_str())
                        .collect::<Vec<_>>()
                        .join(" ");
                    full_sig.push(' ');
                    full_sig.push_str(&cont);
                    consumed += 1;
                    j += 1;
                    continue;
                }
            }
        }
        break;
    }

    // Now look for the definition line: name params = body
    // It could start on the next line at column 0
    let body_start = start + consumed;
    let mut body_lines_raw: Vec<String> = Vec::new();

    if body_start < token_lines.len() {
        let def_tl = &token_lines[body_start];
        // Check if this line starts with the function name
        if !def_tl.tokens.is_empty() && def_tl.tokens[0].1 == name {
            let line_idx = def_tl.line_number.saturating_sub(1);
            if line_idx < raw_lines.len() {
                body_lines_raw.push(raw_lines[line_idx].to_string());
            }
            consumed += 1;

            // Collect continuation lines (indented)
            let mut k = body_start + 1;
            while k < token_lines.len() {
                let li = token_lines[k].line_number.saturating_sub(1);
                if li < raw_lines.len() {
                    let line = raw_lines[li];
                    if line.starts_with(' ') || line.starts_with('\t') {
                        body_lines_raw.push(line.to_string());
                        consumed += 1;
                        k += 1;
                    } else {
                        break;
                    }
                } else {
                    break;
                }
            }
        }
    }

    let body = body_lines_raw.join("\n");
    let branches = extract_branches(&body);

    (
        consumed,
        Item::Function {
            name,
            signature: full_sig,
            branches,
            body,
            doc,
        },
    )
}

/// Parse a bare function definition (no separate signature line).
/// Pattern: name params = body at column 0
fn parse_bare_function(
    token_lines: &[TokenLine],
    start: usize,
    raw_lines: &[&str],
    doc: Vec<String>,
) -> (usize, Item) {
    let tl = &token_lines[start];
    let name = tl.tokens[0].1.clone();

    // Build body from this line + continuation lines
    let mut body_lines: Vec<String> = Vec::new();
    let line_idx = tl.line_number.saturating_sub(1);
    if line_idx < raw_lines.len() {
        body_lines.push(raw_lines[line_idx].to_string());
    }

    let mut consumed = 1;
    let mut k = start + 1;
    while k < token_lines.len() {
        let li = token_lines[k].line_number.saturating_sub(1);
        if li < raw_lines.len() {
            let line = raw_lines[li];
            if line.starts_with(' ') || line.starts_with('\t') {
                body_lines.push(line.to_string());
                consumed += 1;
                k += 1;
            } else {
                break;
            }
        } else {
            break;
        }
    }

    // Extract signature: everything between name and = on the first line
    // For "isFleetMode m = ..." → signature is implicit
    // Extract params
    let eq_pos = tl
        .tokens
        .iter()
        .position(|(t, _)| *t == Token::Eq)
        .unwrap_or(tl.tokens.len());

    let body = body_lines.join("\n");
    let branches = extract_branches(&body);

    // Build a simple signature from the tokens
    let sig_parts: Vec<String> = tl.tokens[1..eq_pos]
        .iter()
        .map(|(_, t)| t.clone())
        .collect();

    (
        consumed,
        Item::Function {
            name,
            signature: sig_parts.join(" "),
            branches,
            body,
            doc,
        },
    )
}

// ── Branch extraction ───────────────────────────────────────────────────────

fn extract_branches(body: &str) -> Vec<Branch> {
    let mut branches: Vec<Branch> = Vec::new();

    // Find the part after the first definition `=`
    let after_eq_start = find_body_eq(body);
    if after_eq_start.is_none() {
        return branches;
    }
    let after_eq = &body[after_eq_start.unwrap()..];
    let trimmed = after_eq.trim();

    if !trimmed.starts_with("if ")
        && !trimmed.starts_with("if\n")
        && !trimmed.contains("\n")
    {
        // Simple single-line body, no if/then/else
        branches.push(Branch {
            condition: None,
            result: trimmed.to_string(),
        });
        return branches;
    }

    // Parse if/then/else chains from the body text
    parse_if_then_else(trimmed, &mut branches);

    // If we didn't find any branches but have a body, store as single branch
    if branches.is_empty() && !trimmed.is_empty() {
        branches.push(Branch {
            condition: None,
            result: trimmed.to_string(),
        });
    }

    branches
}

/// Find the position right after the definition `=` (not `==`, `!=`, `<=`, `>=`, `==>`)
fn find_body_eq(body: &str) -> Option<usize> {
    let bytes = body.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'=' {
            // Check it's not ==, !=, <=, >=, ==>
            let prev = if i > 0 { Some(bytes[i - 1]) } else { None };
            let next = if i + 1 < bytes.len() {
                Some(bytes[i + 1])
            } else {
                None
            };
            if prev == Some(b'!') || prev == Some(b'<') || prev == Some(b'>') {
                i += 1;
                continue;
            }
            if next == Some(b'=') || next == Some(b'>') {
                i += 2;
                continue;
            }
            return Some(i + 1);
        }
        i += 1;
    }
    None
}

fn parse_if_then_else(text: &str, branches: &mut Vec<Branch>) {
    // Strategy: scan for `if COND then RESULT`, `| COND then RESULT`, `else RESULT`
    // We work line-by-line but also handle multi-line conditions/results.
    let lines: Vec<&str> = text.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        let trimmed = lines[i].trim();

        // `if COND then RESULT` or `if ~ COND then RESULT`
        if trimmed.starts_with("if ") {
            if let Some(branch) = parse_branch_line(trimmed, "if ") {
                branches.push(branch);
            }
            i += 1;
            continue;
        }

        // `| COND then RESULT`
        if trimmed.starts_with("| ") || trimmed.starts_with("|") {
            let content = trimmed.strip_prefix("| ").or_else(|| trimmed.strip_prefix("|")).unwrap_or(trimmed);
            if let Some(branch) = parse_branch_line(content.trim(), "") {
                branches.push(branch);
            }
            i += 1;
            continue;
        }

        // `else RESULT`
        if trimmed.starts_with("else ") || trimmed == "else" {
            let result = trimmed
                .strip_prefix("else")
                .unwrap_or("")
                .trim()
                .to_string();
            if !result.is_empty() {
                branches.push(Branch {
                    condition: None,
                    result,
                });
            }
            i += 1;
            continue;
        }

        // Nested `(if ... then ... else ...)` inside a then-clause
        if trimmed.starts_with("(if ") {
            let inner = trimmed.strip_prefix("(").unwrap_or(trimmed);
            // Remove trailing ) if present
            let inner = if let Some(stripped) = inner.strip_suffix(')') {
                stripped
            } else {
                inner
            };
            if let Some(branch) = parse_branch_line(inner.trim(), "if ") {
                branches.push(branch);
            }
            i += 1;
            continue;
        }

        i += 1;
    }
}

fn parse_branch_line(line: &str, prefix: &str) -> Option<Branch> {
    let content = line.strip_prefix(prefix).unwrap_or(line).trim();

    // Find `then` keyword
    if let Some(then_pos) = find_word(content, "then") {
        let condition = content[..then_pos].trim().to_string();
        let result_text = content[then_pos + 4..].trim();

        // If result is empty, the result may be on the next line
        if result_text.is_empty() {
            return Some(Branch {
                condition: Some(condition),
                result: String::new(),
            });
        }

        return Some(Branch {
            condition: Some(condition),
            result: result_text.to_string(),
        });
    }

    None
}

/// Find a whole word in a string (not part of an identifier).
fn find_word(s: &str, word: &str) -> Option<usize> {
    let mut start = 0;
    while let Some(pos) = s[start..].find(word) {
        let abs_pos = start + pos;
        let before_ok = abs_pos == 0
            || !s.as_bytes()[abs_pos - 1].is_ascii_alphanumeric();
        let after_ok = abs_pos + word.len() >= s.len()
            || !s.as_bytes()[abs_pos + word.len()].is_ascii_alphanumeric();
        if before_ok && after_ok {
            return Some(abs_pos);
        }
        start = abs_pos + 1;
    }
    None
}

// ── Property parsing ────────────────────────────────────────────────────────

fn parse_property(
    token_lines: &[TokenLine],
    start: usize,
    raw_lines: &[&str],
    doc: Vec<String>,
) -> (usize, Item) {
    let tl = &token_lines[start];

    // property P1_KeyMonotonicity params = body
    // The name is the token after `property`
    let full_name = if tl.tokens.len() > 1 {
        tl.tokens[1].1.clone()
    } else {
        String::new()
    };

    // Split on first underscore: P1 → label, rest → name
    let (label, name) = if let Some(us_pos) = full_name.find('_') {
        (
            full_name[..us_pos].to_string(),
            full_name[us_pos + 1..].to_string(),
        )
    } else {
        (full_name.clone(), full_name.clone())
    };

    // Extract params: everything between name and =
    let mut params: Vec<String> = Vec::new();
    let mut found_name = false;
    let mut paren_depth = 0;
    let mut current_param = String::new();

    for (tok, text) in &tl.tokens {
        if !found_name {
            if *tok == Token::Ident && *text == full_name {
                found_name = true;
            }
            continue;
        }
        if *tok == Token::Eq
            && paren_depth == 0
        {
            if !current_param.is_empty() {
                params.push(current_param.trim().to_string());
            }
            break;
        }
        if *tok == Token::LParen {
            paren_depth += 1;
            current_param.push('(');
            continue;
        }
        if *tok == Token::RParen {
            paren_depth -= 1;
            current_param.push(')');
            if paren_depth == 0 {
                params.push(current_param.trim().to_string());
                current_param = String::new();
            }
            continue;
        }
        if paren_depth > 0 {
            if !current_param.ends_with('(') {
                current_param.push(' ');
            }
            current_param.push_str(text);
        } else {
            params.push(text.clone());
        }
    }

    // Collect body lines
    let mut body_lines: Vec<String> = Vec::new();
    let line_idx = tl.line_number.saturating_sub(1);
    if line_idx < raw_lines.len() {
        // Extract body part (after =) from first line
        let line = raw_lines[line_idx];
        if let Some(eq_p) = find_body_eq(line) {
            let after = line[eq_p..].trim();
            if !after.is_empty() {
                body_lines.push(after.to_string());
            }
        }
    }

    let mut consumed = 1;
    let mut k = start + 1;
    while k < token_lines.len() {
        let li = token_lines[k].line_number.saturating_sub(1);
        if li < raw_lines.len() {
            let line = raw_lines[li];
            if line.starts_with(' ') || line.starts_with('\t') {
                body_lines.push(line.trim().to_string());
                consumed += 1;
                k += 1;
            } else {
                break;
            }
        } else {
            break;
        }
    }

    let body = body_lines.join("\n");

    (
        consumed,
        Item::Property {
            label,
            name,
            params,
            body,
            doc,
            proof_status: None,
        },
    )
}

// ── Enum grouping post-pass ─────────────────────────────────────────────────

fn group_enums(items: &mut Vec<Item>) {
    // Find TypeAlias items and group subsequent constant definitions + predicates
    let mut result: Vec<Item> = Vec::new();
    let mut i = 0;

    while i < items.len() {
        if let Item::TypeAlias { ref name, ref width, ref doc } = items[i] {
            let type_name = name.clone();
            let type_width = width.clone();
            let type_doc = doc.clone();

            // Look ahead for constants of this type
            let mut variants: Vec<EnumVariant> = Vec::new();
            let mut predicate: Option<String> = None;
            let mut j = i + 1;

            while j < items.len() {
                match &items[j] {
                    Item::Function {
                        name: fn_name,
                        signature: fn_sig,
                        branches: _,
                        body: fn_body,
                        doc: _,
                    } if fn_sig.trim() == type_name
                        && !fn_body.is_empty()
                        && fn_name.chars().next().is_some_and(|c| c.is_uppercase()) =>
                    {
                        // This is a constant: NAME = value : TypeName
                        variants.push(EnumVariant {
                            name: fn_name.clone(),
                            value: fn_body.clone(),
                        });
                        j += 1;
                    }
                    Item::Function {
                        name: fn_name,
                        ..
                    } if fn_name == &format!("is{}", type_name) => {
                        predicate = Some(fn_name.clone());
                        j += 1;
                    }
                    Item::CommentBlock { .. } => {
                        // Skip comment blocks between constants
                        j += 1;
                    }
                    _ => break,
                }
            }

            if !variants.is_empty() {
                // Also look further ahead for the predicate (it may come
                // after a comment block or other constants of different types)
                if predicate.is_none() {
                    for item in items.iter().skip(j) {
                        if let Item::Function { name: fn_name, .. } = item
                            && *fn_name == format!("is{}", type_name)
                        {
                            predicate = Some(fn_name.clone());
                            break;
                        }
                    }
                }

                result.push(Item::EnumGroup {
                    type_name,
                    width: type_width,
                    variants,
                    predicate,
                    doc: type_doc,
                });
                i = j;
            } else {
                result.push(items[i].clone());
                i += 1;
            }
        } else {
            result.push(items[i].clone());
            i += 1;
        }
    }

    // Remove predicate functions that were absorbed into enum groups
    let predicate_names: Vec<String> = result
        .iter()
        .filter_map(|item| {
            if let Item::EnumGroup { predicate: Some(p), .. } = item {
                Some(p.clone())
            } else {
                None
            }
        })
        .collect();

    result.retain(|item| {
        if let Item::Function { name, .. } = item {
            !predicate_names.contains(name)
        } else {
            true
        }
    });

    // Also remove the constant Functions that were absorbed into EnumGroups
    // (they were stored as Functions with signature == type_name)
    let enum_variant_names: Vec<String> = result
        .iter()
        .filter_map(|item| {
            if let Item::EnumGroup { variants, .. } = item {
                Some(variants.iter().map(|v| v.name.clone()).collect::<Vec<_>>())
            } else {
                None
            }
        })
        .flatten()
        .collect();

    result.retain(|item| {
        if let Item::Function { name, .. } = item {
            !enum_variant_names.contains(name)
        } else {
            true
        }
    });

    *items = result;
}

// ── Doc attachment post-pass ────────────────────────────────────────────────

fn attach_docs(items: &mut Vec<Item>) {
    // Walk items: if a CommentBlock is followed by a non-comment item,
    // attach the comment block's lines as doc to the next item.
    let mut i = 0;
    while i + 1 < items.len() {
        if let Item::CommentBlock { .. } = &items[i] {
            // Check if next item can receive doc and has empty doc
            let should_attach = match &items[i + 1] {
                Item::Module { doc, .. }
                | Item::TypeAlias { doc, .. }
                | Item::EnumGroup { doc, .. }
                | Item::RecordType { doc, .. }
                | Item::Function { doc, .. }
                | Item::Property { doc, .. } => doc.is_empty(),
                Item::Section { doc, .. } => doc.is_empty(),
                _ => false,
            };

            if should_attach
                && let Item::CommentBlock { lines } = items.remove(i)
            {
                    match &mut items[i] {
                        Item::Module { doc, .. }
                        | Item::TypeAlias { doc, .. }
                        | Item::EnumGroup { doc, .. }
                        | Item::RecordType { doc, .. }
                        | Item::Function { doc, .. }
                        | Item::Property { doc, .. }
                        | Item::Section { doc, .. } => {
                            *doc = lines;
                        }
                        _ => {}
                    }
                    continue; // Don't increment i
            }
        }
        i += 1;
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_module() {
        let items = parse("module SDEP where\n");
        assert!(items.iter().any(|i| matches!(i, Item::Module { name, .. } if name == "SDEP")));
    }

    #[test]
    fn test_parse_type_alias() {
        let items = parse("type FleetMode = [1]\n");
        assert!(items.iter().any(|i| matches!(i, Item::TypeAlias { name, width, .. }
            if name == "FleetMode" && width == "1")));
    }

    #[test]
    fn test_parse_enum_group() {
        let input = "\
type FleetMode = [1]
FM_Disabled = 0 : FleetMode
FM_Enabled  = 1 : FleetMode
";
        let items = parse(input);
        let eg = items.iter().find(|i| matches!(i, Item::EnumGroup { .. }));
        assert!(eg.is_some(), "Expected EnumGroup, got: {:#?}", items);
        if let Some(Item::EnumGroup {
            type_name,
            width,
            variants,
            ..
        }) = eg
        {
            assert_eq!(type_name, "FleetMode");
            assert_eq!(width, "1");
            assert_eq!(variants.len(), 2);
            assert_eq!(variants[0].name, "FM_Disabled");
            assert_eq!(variants[0].value, "0");
            assert_eq!(variants[1].name, "FM_Enabled");
            assert_eq!(variants[1].value, "1");
        }
    }

    #[test]
    fn test_parse_record_type() {
        let input = "\
type EnrollmentStatus =
  { fleetMode : FleetMode
  , hasKey    : Bit
  , keyId     : OptUUID
  , isActive  : Bit
  }
";
        let items = parse(input);
        let rt = items
            .iter()
            .find(|i| matches!(i, Item::RecordType { .. }));
        assert!(rt.is_some(), "Expected RecordType, got: {:#?}", items);
        if let Some(Item::RecordType { name, fields, .. }) = rt {
            assert_eq!(name, "EnrollmentStatus");
            assert_eq!(fields.len(), 4);
            assert_eq!(fields[0].0, "fleetMode");
            assert_eq!(fields[0].1, "FleetMode");
            assert_eq!(fields[1].0, "hasKey");
            assert_eq!(fields[1].1, "Bit");
        }
    }

    #[test]
    fn test_parse_provision_key_branches() {
        let input = "\
provisionKey :
  Bit -> Bit -> KeyVaultResult -> Bit -> ProvisionResult
provisionKey fleetEnabled validRequest vaultResult keyIsActive =
  if ~ fleetEnabled        then PR_Disabled
   | ~ validRequest        then PR_BadRequest
   | vaultResult != KV_Ok  then PR_InternalError
   | keyIsActive           then PR_Unauthorized
  else                          PR_Succeeded
";
        let items = parse(input);
        let func = items
            .iter()
            .find(|i| matches!(i, Item::Function { name, .. } if name == "provisionKey"));
        assert!(func.is_some(), "Expected provisionKey, got: {:#?}", items);
        if let Some(Item::Function { branches, .. }) = func {
            assert_eq!(branches.len(), 5, "Expected 5 branches, got: {:#?}", branches);
            assert!(branches[0].condition.is_some());
            assert_eq!(branches[4].condition, None); // else branch
            assert!(branches[4].result.contains("PR_Succeeded"));
        }
    }

    #[test]
    fn test_parse_enroll_device_nested() {
        let input = "\
enrollDevice :
  Bit -> Bit -> AuthResult -> ActivationResult -> EnrollmentResult
enrollDevice fleetEnabled validMetadata authResult activationResult =
  if ~ fleetEnabled                       then ER_Disabled
   | ~ validMetadata                      then ER_Unauthorized
   | authResult == AR_Authenticated       then
        (if activationResult == AC_Success       then ER_Succeeded
          | activationResult == AC_AlreadyActive then ER_Unauthorized
         else                                         ER_InternalError)
   | authResult == AR_VaultUnavailable    then ER_InternalError
  else                                         ER_Unauthorized
";
        let items = parse(input);
        let func = items
            .iter()
            .find(|i| matches!(i, Item::Function { name, .. } if name == "enrollDevice"));
        assert!(func.is_some(), "Expected enrollDevice, got: {:#?}", items);
        if let Some(Item::Function { branches, .. }) = func {
            // Should have outer branches + nested branches
            assert!(
                branches.len() >= 5,
                "Expected at least 5 branches (with nested), got {}: {:#?}",
                branches.len(),
                branches
            );
        }
    }

    #[test]
    fn test_parse_authenticate_simple() {
        let input = "\
authenticate : Bit -> Bit -> Bit -> Bit
authenticate dateValid signatureValid claimsValid =
  dateValid && signatureValid && claimsValid
";
        let items = parse(input);
        let func = items
            .iter()
            .find(|i| matches!(i, Item::Function { name, .. } if name == "authenticate"));
        assert!(func.is_some(), "Expected authenticate, got: {:#?}", items);
        if let Some(Item::Function { branches, .. }) = func {
            assert_eq!(branches.len(), 1, "Expected 1 branch, got: {:#?}", branches);
            assert!(branches[0].condition.is_none());
            assert!(branches[0].result.contains("&&"));
        }
    }

    #[test]
    fn test_parse_property() {
        let input = "\
property P1_KeyMonotonicity fleetEnabled validMetadata authResult keyAlreadyActive =
  isAuthResult authResult ==>
    keyAlreadyActive ==>
      enrollDevice fleetEnabled validMetadata authResult AC_AlreadyActive
        != ER_Succeeded
";
        let items = parse(input);
        let prop = items
            .iter()
            .find(|i| matches!(i, Item::Property { .. }));
        assert!(prop.is_some(), "Expected Property, got: {:#?}", items);
        if let Some(Item::Property {
            label,
            name,
            params,
            ..
        }) = prop
        {
            assert_eq!(label, "P1");
            assert_eq!(name, "KeyMonotonicity");
            assert_eq!(
                params,
                &[
                    "fleetEnabled",
                    "validMetadata",
                    "authResult",
                    "keyAlreadyActive"
                ]
            );
        }
    }

    #[test]
    fn test_parse_property_typed_params() {
        let input = "\
property P8_CorrectSignatureVerifies (k : HmacKey) (r : Request) =
  isValidSignature k r (hmacSha256 k r) == True
";
        let items = parse(input);
        let prop = items
            .iter()
            .find(|i| matches!(i, Item::Property { .. }));
        assert!(prop.is_some(), "Expected Property, got: {:#?}", items);
        if let Some(Item::Property {
            label,
            name,
            params,
            ..
        }) = prop
        {
            assert_eq!(label, "P8");
            assert_eq!(name, "CorrectSignatureVerifies");
            assert_eq!(params.len(), 2);
            assert!(params[0].contains("k : HmacKey"));
            assert!(params[1].contains("r : Request"));
        }
    }

    #[test]
    fn test_parse_full_sdep() {
        let input = std::fs::read_to_string("tests/fixtures/SDEP.cry")
            .expect("Could not read test fixture");
        let items = parse(&input);

        // Count item types
        let modules: Vec<_> = items
            .iter()
            .filter(|i| matches!(i, Item::Module { .. }))
            .collect();
        let sections: Vec<_> = items
            .iter()
            .filter(|i| matches!(i, Item::Section { .. }))
            .collect();
        let enum_groups: Vec<_> = items
            .iter()
            .filter(|i| matches!(i, Item::EnumGroup { .. }))
            .collect();
        let records: Vec<_> = items
            .iter()
            .filter(|i| matches!(i, Item::RecordType { .. }))
            .collect();
        let functions: Vec<_> = items
            .iter()
            .filter(|i| matches!(i, Item::Function { .. }))
            .collect();
        let properties: Vec<_> = items
            .iter()
            .filter(|i| matches!(i, Item::Property { .. }))
            .collect();

        // 1 Module
        assert_eq!(modules.len(), 1, "Expected 1 module");

        // Multiple sections
        assert!(
            sections.len() >= 3,
            "Expected at least 3 sections, got {}",
            sections.len()
        );

        // 8 enum types
        assert!(
            enum_groups.len() >= 8,
            "Expected at least 8 enum groups, got {}: {:?}",
            enum_groups.len(),
            enum_groups
                .iter()
                .filter_map(|i| if let Item::EnumGroup { type_name, .. } = i {
                    Some(type_name.as_str())
                } else {
                    None
                })
                .collect::<Vec<_>>()
        );

        // 1 RecordType (EnrollmentStatus)
        assert!(
            !records.is_empty(),
            "Expected at least 1 record type, got {}",
            records.len()
        );

        // Functions parsed (provisionKey, enrollDevice, authenticate, etc.)
        let fn_names: Vec<&str> = functions
            .iter()
            .filter_map(|i| {
                if let Item::Function { name, .. } = i {
                    Some(name.as_str())
                } else {
                    None
                }
            })
            .collect();
        assert!(
            fn_names.contains(&"provisionKey"),
            "Missing provisionKey in {:?}",
            fn_names
        );
        assert!(
            fn_names.contains(&"authenticate"),
            "Missing authenticate in {:?}",
            fn_names
        );

        // All 29 properties (P1-P29)
        assert_eq!(
            properties.len(),
            29,
            "Expected 29 properties, got {}: {:?}",
            properties.len(),
            properties
                .iter()
                .filter_map(|i| if let Item::Property { label, name, .. } = i {
                    Some(format!("{}_{}", label, name))
                } else {
                    None
                })
                .collect::<Vec<_>>()
        );

        // Verify provisionKey has 5 branches
        if let Some(Item::Function { branches, .. }) = functions
            .iter()
            .find(|i| matches!(i, Item::Function { name, .. } if name == "provisionKey"))
        {
            assert_eq!(
                branches.len(),
                5,
                "provisionKey should have 5 branches, got {}",
                branches.len()
            );
        }

        // Check EnrollmentStatus record
        if let Some(Item::RecordType { name, fields, .. }) = records.first() {
            assert_eq!(*name, "EnrollmentStatus");
            assert_eq!(fields.len(), 4);
        }
    }
}
