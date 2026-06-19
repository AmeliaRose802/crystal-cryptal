// In-spec `@coverage …` directives: doc-comment annotations that classify a
// model function next to its definition.

/// The doc-comment tag that introduces an in-spec coverage directive.
const DIRECTIVE_TAG: &str = "@coverage";

/// True when a rendered doc line is a `@coverage …` directive. Callers in the
/// Markdown renderer use this to hide the directive from displayed prose — the
/// resulting badge and per-page banner already convey its meaning.
pub fn is_coverage_directive_line(line: &str) -> bool {
    line.trim_start()
        .to_ascii_lowercase()
        .starts_with(DIRECTIVE_TAG)
}

/// The classification a `@coverage` directive maps to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DirectiveKind {
    /// 🔒 Trusted assumption (deliberate override / uninterpreted primitive).
    Trusted,
    /// 🧩 ABI adapter / model abstraction / stand-in.
    Abstraction,
    /// 📄 Spec-only reference function with no implementation.
    SpecOnly,
    /// Drop the function from the matrix entirely.
    Exclude,
}

/// A parsed `@coverage <kind>[: note]` directive.
#[derive(Debug, Clone)]
pub(crate) struct CoverageDirective {
    pub kind: DirectiveKind,
    pub note: Option<String>,
}

/// Parse the first `@coverage …` directive out of a function's doc comment.
///
/// Recognised forms (case-insensitive, note optional):
///
/// ```text
/// @coverage trusted: real SHA-256 is not proven here.
/// @coverage abstraction: bounded fixed-width encoder model.
/// @coverage spec-only
/// @coverage exclude
/// ```
///
/// Kind aliases: `trusted`/`assumption`/`override`, `abstraction`/`adapter`/
/// `abi`/`stand-in`, `spec-only`/`spec_only`/`spec`, `exclude`/`internal`.
/// An unrecognised kind token is ignored (the next directive line, if any, is
/// tried) so a typo degrades to "no directive" rather than a wrong badge.
pub(crate) fn parse_coverage_directive(doc: &[String]) -> Option<CoverageDirective> {
    for line in doc {
        if !is_coverage_directive_line(line) {
            continue;
        }
        let body = line.trim_start()[DIRECTIVE_TAG.len()..].trim_start();
        let (kind_tok, note) = match body.split_once(':') {
            Some((k, n)) => {
                let n = n.trim();
                (k.trim(), (!n.is_empty()).then(|| n.to_string()))
            }
            None => (body.trim(), None),
        };
        let kind = match kind_tok.to_ascii_lowercase().as_str() {
            "trusted" | "assumption" | "override" => DirectiveKind::Trusted,
            "abstraction" | "adapter" | "abi" | "stand-in" => DirectiveKind::Abstraction,
            "spec-only" | "spec_only" | "speconly" | "spec" => DirectiveKind::SpecOnly,
            "exclude" | "excluded" | "internal" => DirectiveKind::Exclude,
            _ => continue,
        };
        return Some(CoverageDirective { kind, note });
    }
    None
}
