// Extract section/category headings from source-level `//` comment blocks
// (e.g. `// ── Section title ────` or `// ----` separators around a title).

use crate::ir::Item;

pub(super) fn extract_sections_with_offsets(source: &str) -> Vec<(usize, Item)> {
    let mut result = Vec::new();
    let lines: Vec<&str> = source.lines().collect();
    let mut line_offsets: Vec<usize> = Vec::with_capacity(lines.len());
    let mut offset = 0;
    for line in source.lines() {
        line_offsets.push(offset);
        offset += line.len() + 1;
    }
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i].trim();

        if is_separator_line(line)
            && i + 2 < lines.len()
            && is_comment_line(lines[i + 1])
            && is_separator_line(lines[i + 2].trim())
        {
            let title = strip_comment_prefix(lines[i + 1]).trim().to_string();
            result.push((
                line_offsets[i],
                Item::Section {
                    level: 2,
                    title,
                    doc: Vec::new(),
                },
            ));
            i += 3;
            continue;
        }

        if is_comment_line(line) && is_category_line(line) {
            let title = extract_category_title(line);
            result.push((
                line_offsets[i],
                Item::Section {
                    level: 3,
                    title,
                    doc: Vec::new(),
                },
            ));
            i += 1;
            continue;
        }

        i += 1;
    }
    result
}

fn is_separator_line(line: &str) -> bool {
    let t = line.trim();
    if !t.starts_with("//") {
        return false;
    }
    let inner = t.trim_start_matches('/');
    inner.is_empty() || inner.chars().all(|c| c == '/')
}

fn is_comment_line(line: &str) -> bool {
    line.trim().starts_with("//")
}

fn is_category_line(line: &str) -> bool {
    let inner = strip_comment_prefix(line);
    inner.starts_with("----") || inner.starts_with("── ")
}

fn extract_category_title(line: &str) -> String {
    let inner = strip_comment_prefix(line);
    inner
        .trim_matches(|c: char| c == '-' || c == ' ' || c == '─')
        .trim()
        .to_string()
}

fn strip_comment_prefix(line: &str) -> String {
    let s = line.trim().strip_prefix("//").unwrap_or(line.trim());
    s.strip_prefix(' ').unwrap_or(s).to_string()
}
