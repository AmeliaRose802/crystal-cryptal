// Layout processing: insert virtual tokens (VCurlyL/VCurlyR/VSemi) following
// Cryptol's Haskell-style indentation rules.
//
// After `where`, `let`, `of`, `parameter`, if the next token is not `{`, insert
// VCurlyL and track indentation.
// At each new line:
//   col == context  → insert VSemi
//   col <  context  → insert VCurlyR, pop, recheck
//   col >  context  → continuation

use super::{PosToken, Tok};

fn is_layout_keyword(tok: &Tok) -> bool {
    matches!(
        tok,
        Tok::KwWhere | Tok::KwLet | Tok::KwOf | Tok::KwParameter
    )
}

pub(super) fn apply_layout(tokens: Vec<PosToken>) -> Vec<(usize, Tok, usize)> {
    if tokens.is_empty() {
        return Vec::new();
    }

    let mut result: Vec<(usize, Tok, usize)> = Vec::new();
    let mut layout_stack: Vec<usize> = Vec::new(); // stack of context columns
    let mut expect_layout = false; // true after a layout keyword
    let mut prev_line = 0usize;

    for pt in &tokens {
        let col = pt.col;
        let line = pt.line;

        if expect_layout {
            expect_layout = false;
            if pt.tok == Tok::CurlyL {
                // Explicit brace — no layout
                result.push((pt.start, pt.tok.clone(), pt.end));
                prev_line = line;
                continue;
            }
            // Insert VCurlyL and push context.
            // The opening token defines the context — skip layout checks for it.
            result.push((pt.start, Tok::VCurlyL, pt.start));
            layout_stack.push(col);
        } else if line > prev_line && !layout_stack.is_empty() {
            // At a new line, check layout
            // Use the end of the previous real token for virtual token positions
            // so that spans don't extend into whitespace/block-comment gaps.
            let prev_end = result.last().map(|(_, _, e)| *e).unwrap_or(pt.start);
            // Close contexts that are deeper than current column
            while let Some(&ctx) = layout_stack.last() {
                if col < ctx {
                    // Only insert VSemi if the block isn't empty
                    if !matches!(result.last(), Some((_, Tok::VCurlyL, _))) {
                        result.push((prev_end, Tok::VSemi, prev_end));
                    }
                    result.push((prev_end, Tok::VCurlyR, prev_end));
                    layout_stack.pop();
                } else {
                    break;
                }
            }
            // Insert VSemi if at same level
            if let Some(&ctx) = layout_stack.last()
                && col == ctx
            {
                result.push((prev_end, Tok::VSemi, prev_end));
            }
        }

        // Check for layout keyword
        if is_layout_keyword(&pt.tok) {
            expect_layout = true;
        }

        result.push((pt.start, pt.tok.clone(), pt.end));
        prev_line = line;
    }

    // If expect_layout is still pending (layout keyword at end of input),
    // insert an empty layout block
    if expect_layout {
        let last_pos = tokens.last().map(|t| t.end).unwrap_or(0);
        result.push((last_pos, Tok::VCurlyL, last_pos));
        layout_stack.push(0);
    }

    // Close all remaining layout contexts
    let last_pos = tokens.last().map(|t| t.end).unwrap_or(0);
    while layout_stack.pop().is_some() {
        // Only insert VSemi if the block isn't empty
        if !matches!(result.last(), Some((_, Tok::VCurlyL, _))) {
            result.push((last_pos, Tok::VSemi, last_pos));
        }
        result.push((last_pos, Tok::VCurlyR, last_pos));
    }

    result
}
