// Cross-reference resolution: rewrite Markdown text so symbol names become
// links to their generated pages.

use regex::Regex;

use super::SymbolTable;

impl SymbolTable {
    /// Resolve cross-references as anchor-only links for single-file output.
    pub fn resolve_links_single_file(&self, text: &str) -> String {
        let mut syms: Vec<(&str, &(String, String))> =
            self.symbols.iter().map(|(k, v)| (k.as_str(), v)).collect();
        syms.sort_by(|a, b| {
            let a_qual = a.0.contains("::");
            let b_qual = b.0.contains("::");
            a_qual.cmp(&b_qual).then_with(|| b.0.len().cmp(&a.0.len()))
        });

        rewrite_outside_code_and_links(text, |segment| {
            let mut result = segment.to_string();
            for (name, (_, anchor)) in &syms {
                let pattern = symbol_pattern(name);
                let re = match Regex::new(&pattern) {
                    Ok(r) => r,
                    Err(_) => continue,
                };

                // For functions the anchor is empty in multi-file mode;
                // in single-file mode derive it from the name (lowercased).
                let actual_anchor = if anchor.is_empty() {
                    name.to_lowercase()
                } else {
                    anchor.clone()
                };

                let link = format!("[{name}](#{actual_anchor})");
                if name.contains("::") {
                    result = re.replace_all(&result, link.as_str()).to_string();
                } else {
                    result = re
                        .replace_all(&result, |caps: &regex::Captures<'_>| {
                            format!("{}{}", &caps[1], link)
                        })
                        .to_string();
                }
            }
            result
        })
    }

    /// Resolve cross-references in text, generating relative links from current_file.
    pub fn resolve_links(&self, text: &str, current_file: &str) -> String {
        // Sort symbols by length descending so longer names match first.
        let mut syms: Vec<(&str, &(String, String))> =
            self.symbols.iter().map(|(k, v)| (k.as_str(), v)).collect();
        syms.sort_by(|a, b| {
            let a_qual = a.0.contains("::");
            let b_qual = b.0.contains("::");
            a_qual.cmp(&b_qual).then_with(|| b.0.len().cmp(&a.0.len()))
        });

        rewrite_outside_code_and_links(text, |segment| {
            let mut result = segment.to_string();
            for (name, (target_file, anchor)) in &syms {
                // Don't self-link.
                if current_file == *target_file {
                    continue;
                }

                let pattern = symbol_pattern(name);
                let re = match Regex::new(&pattern) {
                    Ok(r) => r,
                    Err(_) => continue,
                };

                let rel = Self::relative_path(current_file, target_file);
                let link = if anchor.is_empty() {
                    format!("[{name}]({rel})")
                } else {
                    format!("[{name}]({rel}#{anchor})")
                };

                if name.contains("::") {
                    result = re.replace_all(&result, link.as_str()).to_string();
                } else {
                    result = re
                        .replace_all(&result, |caps: &regex::Captures<'_>| {
                            format!("{}{}", &caps[1], link)
                        })
                        .to_string();
                }
            }
            result
        })
    }
}

fn symbol_pattern(name: &str) -> String {
    let escaped = regex::escape(name);
    if name.contains("::") {
        escaped
    } else {
        format!(r"(^|[^:A-Za-z0-9_'])({escaped})\b")
    }
}

/// Apply `rewrite` to slices of `text` that are *outside* Markdown code spans
/// (`` `…` ``, ```` ``…`` ````, fenced ```` ``` ````-blocks) and Markdown link
/// targets (the `(…)` half of `[label](target)`). Backtick-delimited spans and
/// link targets are passed through verbatim so we don't, e.g., autolink a type
/// name inside a code span (which would turn `` `Foo` `` into the broken
/// `` `[Foo](url)` ``) or rewrite the URL of an existing link.
fn rewrite_outside_code_and_links<F>(text: &str, mut rewrite: F) -> String
where
    F: FnMut(&str) -> String,
{
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    let mut plain_start = 0;

    let flush = |out: &mut String, rewrite: &mut F, src: &str, from: usize, to: usize| {
        if from < to {
            out.push_str(&rewrite(&src[from..to]));
        }
    };

    while i < bytes.len() {
        let b = bytes[i];

        // Fenced code block: ``` ... ```  (must start at column 0 after a newline
        // or at very beginning).
        if b == b'`'
            && i + 2 < bytes.len()
            && bytes[i + 1] == b'`'
            && bytes[i + 2] == b'`'
            && (i == 0 || bytes[i - 1] == b'\n')
        {
            flush(&mut out, &mut rewrite, text, plain_start, i);
            // Find closing fence on its own line.
            let after_open = i + 3;
            let close = find_fence_close(&bytes[after_open..]);
            let end = after_open + close;
            out.push_str(&text[i..end.min(bytes.len())]);
            i = end.min(bytes.len());
            plain_start = i;
            continue;
        }

        // Inline code span: `…` or ``…`` (count run length, match same run).
        if b == b'`' {
            flush(&mut out, &mut rewrite, text, plain_start, i);
            let run = bytes[i..].iter().take_while(|&&c| c == b'`').count();
            let after_open = i + run;
            // Find matching closing run of the same length.
            let mut j = after_open;
            let close = loop {
                if j >= bytes.len() {
                    break bytes.len();
                }
                if bytes[j] == b'`' {
                    let rj = bytes[j..].iter().take_while(|&&c| c == b'`').count();
                    if rj == run {
                        break j + rj;
                    }
                    j += rj;
                } else {
                    j += 1;
                }
            };
            out.push_str(&text[i..close]);
            i = close;
            plain_start = i;
            continue;
        }

        // Markdown link target: `](...)` — skip the (...) part.
        // Detect when we're sitting at the `(` immediately after a `]` that is
        // itself preceded by an unescaped `[…]` label.
        if b == b'(' && i > 0 && bytes[i - 1] == b']' {
            // Find matching close paren (no nesting in well-formed Markdown links).
            let mut depth = 1usize;
            let mut j = i + 1;
            while j < bytes.len() && depth > 0 {
                match bytes[j] {
                    b'(' => depth += 1,
                    b')' => depth -= 1,
                    b'\\' if j + 1 < bytes.len() => {
                        j += 1;
                    }
                    _ => {}
                }
                j += 1;
            }
            flush(&mut out, &mut rewrite, text, plain_start, i);
            out.push_str(&text[i..j]);
            i = j;
            plain_start = i;
            continue;
        }

        i += 1;
    }

    flush(&mut out, &mut rewrite, text, plain_start, bytes.len());
    out
}

fn find_fence_close(rest: &[u8]) -> usize {
    let mut i = 0;
    while i < rest.len() {
        if rest[i] == b'\n' {
            let line_start = i + 1;
            // Skip up to 3 leading spaces.
            let mut k = line_start;
            let mut spaces = 0;
            while k < rest.len() && rest[k] == b' ' && spaces < 3 {
                k += 1;
                spaces += 1;
            }
            if k + 2 < rest.len() && rest[k] == b'`' && rest[k + 1] == b'`' && rest[k + 2] == b'`' {
                // Consume the rest of this line (the closing fence).
                let mut end = k + 3;
                while end < rest.len() && rest[end] != b'\n' {
                    end += 1;
                }
                if end < rest.len() {
                    end += 1; // include trailing newline
                }
                return end;
            }
        }
        i += 1;
    }
    rest.len()
}
