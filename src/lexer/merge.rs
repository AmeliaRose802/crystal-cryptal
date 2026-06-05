// Qualified identifier merging: collapse `Foo :: Bar :: baz` into a single token.

use super::{PosToken, Tok};

pub(super) fn merge_qualified(tokens: &mut Vec<PosToken>) {
    // Pattern: Ident(A) :: Ident(B) :: ... :: Ident(Z) → QualIdent("A::B::...::", "Z")
    // We look for Ident followed by Op("::") followed by Ident
    let mut i = 0;
    while i + 2 < tokens.len() {
        let is_qual_start = matches!(&tokens[i].tok, Tok::Ident(_))
            && matches!(&tokens[i + 1].tok, Tok::Op(s) if s == "::")
            && matches!(&tokens[i + 2].tok, Tok::Ident(_) | Tok::Op(_));

        if is_qual_start {
            let mut qual = String::new();
            if let Tok::Ident(ref s) = tokens[i].tok {
                qual.push_str(s);
                qual.push_str("::");
            }
            let start = tokens[i].start;
            let line = tokens[i].line;
            let col = tokens[i].col;

            let mut j = i + 2;
            // Keep merging if we see more ::Ident
            while j + 1 < tokens.len() {
                if matches!(&tokens[j + 1].tok, Tok::Op(s) if s == "::")
                    && j + 2 < tokens.len()
                    && matches!(&tokens[j + 2].tok, Tok::Ident(_) | Tok::Op(_))
                {
                    if let Tok::Ident(ref s) = tokens[j].tok {
                        qual.push_str(s);
                        qual.push_str("::");
                    }
                    j += 2;
                } else {
                    break;
                }
            }

            let end = tokens[j].end;
            let final_tok = match &tokens[j].tok {
                Tok::Ident(s) => Tok::QualIdent(qual, s.clone()),
                Tok::Op(s) => Tok::QualOp(qual, s.clone()),
                _ => unreachable!(),
            };

            // Replace tokens[i..=j] with a single token
            let merged = PosToken {
                tok: final_tok,
                start,
                end,
                line,
                col,
            };
            tokens.splice(i..=j, [merged]);
            // Don't increment i — check the merged token against the next
        } else {
            i += 1;
        }
    }
}
