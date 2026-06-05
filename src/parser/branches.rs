// Branch extraction: turn an `if … then … else …` / pattern-style body into
// a list of `Branch` records the renderer can present as a table.

use crate::ir::Branch;

pub(super) fn extract_branches(body: &str) -> Vec<Branch> {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    if !trimmed.starts_with("if ") && !trimmed.starts_with("if\n") && !trimmed.contains('\n') {
        return vec![Branch {
            condition: None,
            result: trimmed.to_string(),
        }];
    }

    let mut branches = Vec::new();
    for line in trimmed.lines() {
        let t = line.trim();
        if t.starts_with("if ") {
            if let Some(b) = parse_branch(t, "if ") {
                branches.push(b);
            }
        } else if t.starts_with("| ") || t.starts_with('|') {
            let content = t
                .strip_prefix("| ")
                .or_else(|| t.strip_prefix('|'))
                .unwrap_or(t);
            if let Some(b) = parse_branch(content.trim(), "") {
                branches.push(b);
            }
        } else if t.starts_with("else ") || t == "else" {
            let result = t.strip_prefix("else").unwrap_or("").trim().to_string();
            if !result.is_empty() {
                branches.push(Branch {
                    condition: None,
                    result,
                });
            }
        }
    }

    if branches.is_empty() {
        vec![Branch {
            condition: None,
            result: trimmed.to_string(),
        }]
    } else {
        branches
    }
}

fn parse_branch(line: &str, prefix: &str) -> Option<Branch> {
    let content = line.strip_prefix(prefix).unwrap_or(line).trim();
    let pos = find_word(content, "then")?;
    let cond = content[..pos].trim().to_string();
    let result = content[pos + 4..].trim().to_string();
    Some(Branch {
        condition: Some(cond),
        result,
    })
}

fn find_word(s: &str, word: &str) -> Option<usize> {
    let mut start = 0;
    while let Some(pos) = s[start..].find(word) {
        let abs = start + pos;
        let before_ok = abs == 0 || !s.as_bytes()[abs - 1].is_ascii_alphanumeric();
        let after_ok =
            abs + word.len() >= s.len() || !s.as_bytes()[abs + word.len()].is_ascii_alphanumeric();
        if before_ok && after_ok {
            return Some(abs);
        }
        start = abs + 1;
    }
    None
}
