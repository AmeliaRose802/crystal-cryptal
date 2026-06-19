// Badge classification: map a function's proof state, inventory entry,
// `coverage.toml` config, and in-spec directive to a single badge, plus the
// reason codes and sort order.

use crate::ir::ProofStatus;

use super::super::config::CoverageConfig;
use super::super::inventory::InventoryEntry;
use super::badge::{CoverageBadge, CoverageReason};
use super::directive::{CoverageDirective, DirectiveKind};

pub(super) fn badge_order(b: CoverageBadge) -> u8 {
    match b {
        CoverageBadge::Unverified => 0,
        CoverageBadge::ProvenBounded => 1,
        CoverageBadge::Proven => 2,
        CoverageBadge::TrustedAssumption => 3,
        CoverageBadge::AbiAdapter => 4,
        CoverageBadge::SpecOnly => 5,
    }
}

pub(super) fn classify(
    name: &str,
    proof: Option<&Option<ProofStatus>>,
    inventory: Option<&InventoryEntry>,
    config: &CoverageConfig,
    directive: Option<&CoverageDirective>,
) -> CoverageBadge {
    // An in-spec `@coverage` directive (authorial, co-located with the model
    // definition) wins, on par with a `coverage.toml` override.
    if let Some(d) = directive {
        match d.kind {
            DirectiveKind::SpecOnly => return CoverageBadge::SpecOnly,
            DirectiveKind::Trusted => return CoverageBadge::TrustedAssumption,
            DirectiveKind::Abstraction => return CoverageBadge::AbiAdapter,
            // Exclusion is handled before `classify` is reached.
            DirectiveKind::Exclude => {}
        }
    }

    // Explicit overrides win.
    if config.is_spec_only(name) {
        return CoverageBadge::SpecOnly;
    }
    if config.assumption_note(name).is_some() || matches!(proof, Some(Some(ProofStatus::Assumed))) {
        return CoverageBadge::TrustedAssumption;
    }
    if config.abstraction_note(name).is_some() {
        return CoverageBadge::AbiAdapter;
    }

    // Proven (bounded or full) outranks everything else for a real claim.
    if let Some(Some(ProofStatus::Proven { iterations, .. })) = proof {
        return if iterations.is_some() {
            CoverageBadge::ProvenBounded
        } else {
            CoverageBadge::Proven
        };
    }

    // Implementation-side with no proof → unverified.
    if inventory.is_some() {
        return CoverageBadge::Unverified;
    }

    // Model-only function with neither proof nor implementation linkage is
    // effectively spec-only in this taxonomy.
    CoverageBadge::SpecOnly
}

pub(super) fn collect_reason_codes(
    name: &str,
    inventory: Option<&InventoryEntry>,
    config: &CoverageConfig,
    allow_composition_fallback: bool,
) -> Vec<CoverageReason> {
    let mut out: Vec<CoverageReason> = Vec::new();

    if let Some(entry) = inventory {
        for code in &entry.reason_codes {
            if let Some(parsed) = CoverageReason::parse(code)
                && !out.contains(&parsed)
            {
                out.push(parsed);
            }
        }
    }

    for code in config.reason_codes(name) {
        if let Some(parsed) = CoverageReason::parse(code)
            && !out.contains(&parsed)
        {
            out.push(parsed);
        }
    }

    if out.is_empty()
        && allow_composition_fallback
        && let Some(entry) = inventory
        && !entry.composes.is_empty()
    {
        out.push(CoverageReason::R6Compositional);
    }

    out.sort();
    out
}
