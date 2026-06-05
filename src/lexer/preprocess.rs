// Pre-processing: strip block comments + line/column tracking.

pub struct DocComment {
    pub content: String,
    pub byte_pos: usize,
}

/// Replace block comments with whitespace (preserving newlines for line tracking).
/// Extract `/** ... */` doc comments for later insertion.
pub(super) fn strip_block_comments(source: &str) -> (String, Vec<DocComment>) {
    let bytes = source.as_bytes();
    let mut out = vec![b' '; bytes.len()];
    let mut docs = Vec::new();
    let mut i = 0;

    // Preserve non-comment bytes
    while i < bytes.len() {
        if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'*' {
            let is_doc = i + 2 < bytes.len()
                && bytes[i + 2] == b'*'
                && !(i + 3 < bytes.len() && bytes[i + 3] == b'*');
            let start = i;
            i += 2;
            let mut depth = 1u32;
            let content_start = i;
            while i < bytes.len() && depth > 0 {
                if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'*' {
                    depth += 1;
                    i += 2;
                } else if i + 1 < bytes.len() && bytes[i] == b'*' && bytes[i + 1] == b'/' {
                    depth -= 1;
                    i += 2;
                } else {
                    if bytes[i] == b'\n' {
                        out[i] = b'\n'; // preserve newlines
                    }
                    i += 1;
                }
            }
            if is_doc {
                let end = if i >= 2 { i - 2 } else { i };
                let content = String::from_utf8_lossy(&bytes[content_start..end])
                    .trim_start_matches('*')
                    .trim()
                    .to_string();
                if !content.is_empty() {
                    docs.push(DocComment {
                        content,
                        byte_pos: start,
                    });
                }
            }
            // bytes from start..i are already spaces in `out`
            // but preserve newlines
            for j in start..i.min(bytes.len()) {
                if bytes[j] == b'\n' {
                    out[j] = b'\n';
                }
            }
        } else {
            out[i] = bytes[i];
            i += 1;
        }
    }

    let cleaned = String::from_utf8_lossy(&out).into_owned();
    (cleaned, docs)
}

pub(super) fn compute_line_starts(source: &str) -> Vec<usize> {
    let mut starts = vec![0usize];
    for (i, b) in source.bytes().enumerate() {
        if b == b'\n' {
            starts.push(i + 1);
        }
    }
    starts
}

pub(super) fn byte_to_line_col(offset: usize, line_starts: &[usize]) -> (usize, usize) {
    let line = match line_starts.binary_search(&offset) {
        Ok(l) => l,
        Err(l) => l.saturating_sub(1),
    };
    let col = offset - line_starts[line] + 1;
    (line + 1, col) // 1-based
}
