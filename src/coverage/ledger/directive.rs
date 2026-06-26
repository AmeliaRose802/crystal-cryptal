// In-spec `@coverage …` / `@uninterpreted` directives: doc-comment annotations
// that classify a model function next to its definition.

/// The doc-comment tag that introduces an in-spec coverage directive.
const DIRECTIVE_TAG: &str = "@coverage";

/// saw-spec-gen's `@uninterpreted` annotation (see its
/// `src/uninterpreted.rs`). The same marker that tells saw-spec-gen to bind
/// the implementation symbol to its Cryptol model via
/// `llvm_unsafe_assume_spec` — an *assumed* spec, not a discharged
/// equivalence proof — also classifies the function as a 🔒 trusted
/// assumption here, so a single annotation keeps both tools in agreement.
const UNINTERPRETED_TAG: &str = "@uninterpreted";

/// Default banner note for an `@uninterpreted` annotation with no explicit
/// `: note`. Spells out the assumed-spec semantics saw-spec-gen applies.
const DEFAULT_UNINTERPRETED_NOTE: &str = "Uninterpreted primitive: saw-spec-gen binds the \
    implementation symbol to this Cryptol model via an assumed spec \
    (`llvm_unsafe_assume_spec`) rather than unfolding it.";

/// True when a rendered doc line is a `@coverage …` or `@uninterpreted`
/// directive. Callers in the Markdown renderer use this to hide the directive
/// from displayed prose — the resulting badge and per-page banner already
/// convey its meaning.
pub fn is_coverage_directive_line(line: &str) -> bool {
    let lower = line.trim_start().to_ascii_lowercase();
    lower.starts_with(DIRECTIVE_TAG) || lower.starts_with(UNINTERPRETED_TAG)
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

/// Parse the first `@coverage …` (or `@uninterpreted`) directive out of a
/// function's doc comment.
///
/// Recognised forms (case-insensitive, note optional):
///
/// ```text
/// @coverage trusted: real SHA-256 is not proven here.
/// @coverage abstraction: bounded fixed-width encoder model.
/// @coverage spec-only
/// @coverage exclude
/// @uninterpreted
/// @uninterpreted symbol="?HmacSha256@@..."
/// @uninterpreted: real HMAC contract; SHA-256 not proven here.
/// ```
///
/// Kind aliases: `trusted`/`assumption`/`override`, `abstraction`/`adapter`/
/// `abi`/`stand-in`, `spec-only`/`spec_only`/`spec`, `exclude`/`internal`.
/// An unrecognised kind token is ignored (the next directive line, if any, is
/// tried) so a typo degrades to "no directive" rather than a wrong badge.
///
/// `@uninterpreted` is saw-spec-gen's own annotation and always maps to the
/// 🔒 trusted-assumption badge; its `symbol="…"` attribute (the implementation
/// symbol, not human prose) is stripped, and any trailing `: note` becomes the
/// banner note.
pub(crate) fn parse_coverage_directive(doc: &[String]) -> Option<CoverageDirective> {
    for line in doc {
        let trimmed = line.trim_start();
        let lower = trimmed.to_ascii_lowercase();
        if lower.starts_with(UNINTERPRETED_TAG) {
            return Some(CoverageDirective {
                kind: DirectiveKind::Trusted,
                note: uninterpreted_note(&trimmed[UNINTERPRETED_TAG.len()..]),
            });
        }
        if !lower.starts_with(DIRECTIVE_TAG) {
            continue;
        }
        let body = trimmed[DIRECTIVE_TAG.len()..].trim_start();
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

/// Build the banner note for an `@uninterpreted` annotation tail (everything
/// after the `@uninterpreted` marker). Drops a `symbol="…"` attribute, honours
/// an explicit `: note`, and otherwise falls back to the default note.
fn uninterpreted_note(rest: &str) -> Option<String> {
    let without_symbol = strip_symbol_attr(rest);
    let explicit = without_symbol
        .trim()
        .strip_prefix(':')
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .map(str::to_string);
    Some(explicit.unwrap_or_else(|| DEFAULT_UNINTERPRETED_NOTE.to_string()))
}

/// Remove a `symbol="…"` (or `symbol='…'`) attribute from an annotation tail,
/// returning the text with that token excised. Best-effort: an unterminated or
/// malformed attribute simply truncates at `symbol`.
fn strip_symbol_attr(s: &str) -> String {
    let Some(idx) = s.to_ascii_lowercase().find("symbol") else {
        return s.to_string();
    };
    let after = s[idx + "symbol".len()..].trim_start();
    let after = after
        .strip_prefix('=')
        .map(str::trim_start)
        .unwrap_or(after);
    let quote = after
        .strip_prefix('"')
        .map(|r| ('"', r))
        .or_else(|| after.strip_prefix('\'').map(|r| ('\'', r)));
    let mut out = s[..idx].to_string();
    if let Some((q, rest)) = quote
        && let Some(end) = rest.find(q)
    {
        out.push_str(&rest[end + 1..]);
    }
    out
}
