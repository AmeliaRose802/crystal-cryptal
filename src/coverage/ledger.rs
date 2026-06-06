// Build the joined coverage ledger from model items, proof manifest data,
// the implementation inventory, and the `coverage.toml` overrides.
//
// The ledger is the set (Implementation ∪ Model). Each entry is classified
// into exactly one of five badges. See module docs for the rationale.

use std::collections::{BTreeMap, HashSet};

use crate::ir::{Item, ProofStatus};

use super::config::CoverageConfig;
use super::inventory::{ImplementationInventory, InventoryEntry};

/// Five-state coverage taxonomy. See `01-coverage-clarity.md` at the repo
/// root for the design.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoverageBadge {
    /// Machine-checked equivalence across **all** ABI inputs.
    Proven,
    /// Proven only up to an iteration / size bound (the `iterations`
    /// metadata on a `ProofStatus::Proven`). The general case is a prose
    /// argument, not a machine proof.
    ProvenBounded,
    /// A Cryptol abstraction with no real-code counterpart (placeholder,
    /// uninterpreted function, or ABI adapter). Carried in
    /// `coverage.toml [abstraction]`.
    ModelAbstraction,
    /// Real function in the inventory (or a model function with no proof)
    /// that has no proof and no exclusion.
    Unverified,
    /// Lives in the model on purpose with no implementation (`secure*`
    /// reference functions, etc.). Carried in
    /// `coverage.toml [spec_only].functions`.
    SpecOnly,
}

impl CoverageBadge {
    pub fn emoji(self) -> &'static str {
        match self {
            CoverageBadge::Proven => "✅",
            CoverageBadge::ProvenBounded => "🔲",
            CoverageBadge::ModelAbstraction => "🧩",
            CoverageBadge::Unverified => "⚠️",
            CoverageBadge::SpecOnly => "📄",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            CoverageBadge::Proven => "Proven",
            CoverageBadge::ProvenBounded => "Proven (bounded)",
            CoverageBadge::ModelAbstraction => "Model abstraction",
            CoverageBadge::Unverified => "Implemented, unverified",
            CoverageBadge::SpecOnly => "Spec-only",
        }
    }
}

/// Which side(s) of the (Implementation ∪ Model) union an entry came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LedgerSource {
    ModelOnly,
    ImplementationOnly,
    Both,
}

#[derive(Debug, Clone)]
pub struct LedgerEntry {
    pub name: String,
    pub source: LedgerSource,
    pub badge: CoverageBadge,

    /// Cryptol module the model definition came from (if any). Used by the
    /// matrix renderer to link back to the rendered page.
    pub module: Option<String>,

    /// `output_prefix` (multi-module) or "" (single-module) so the renderer
    /// can build a working relative link to the function page.
    pub module_prefix: Option<String>,

    pub impl_lang: Option<String>,
    pub impl_symbol: Option<String>,
    pub impl_file: Option<String>,
    pub models: Option<String>,
    pub models_note: Option<String>,
    pub composes: Vec<String>,
    pub abstraction_note: Option<String>,
    pub proof: Option<ProofStatus>,
}

#[derive(Debug, Default, Clone)]
pub struct Ledger {
    pub entries: Vec<LedgerEntry>,
    /// Functions suppressed by `coverage.toml [exclude].functions`. Surfaced
    /// as a footnote on the matrix so the suppression is documented rather
    /// than silent.
    pub excluded: Vec<String>,
}

impl Ledger {
    pub fn lookup(&self, name: &str) -> Option<&LedgerEntry> {
        self.entries.iter().find(|e| e.name == name)
    }

    pub fn count(&self, badge: CoverageBadge) -> usize {
        self.entries.iter().filter(|e| e.badge == badge).count()
    }
}

/// Build the ledger from already-loaded inputs.
///
/// - `modules` is `(module_name, output_prefix, items)` per module. The
///   `items` slice must already have `ProofStatus` populated (the main
///   binary does this via `apply_proof_manifest` *before* calling us).
/// - `inventory` is parsed from `implementation_inventory.json`.
/// - `config` is the parsed `coverage.toml`.
pub fn build_ledger(
    modules: &[(String, String, &[Item])],
    inventory: &ImplementationInventory,
    config: &CoverageConfig,
) -> Ledger {
    // Collect model functions, keyed by name. A model function "wins" the
    // module/prefix attribution from the first occurrence — duplicate names
    // across modules are rare in this codebase and would shadow each other
    // anyway.
    let mut model_fns: BTreeMap<String, ModelFn> = BTreeMap::new();
    for (module_name, prefix, items) in modules {
        for item in items.iter() {
            if let Item::Function {
                name,
                signature,
                branches,
                is_private,
                proof_status,
                ..
            } = item
            {
                // Skip pure constants / aliases that wouldn't get a page
                // anyway — matching the rendering filter so the matrix
                // doesn't list functions that have no detail page.
                if !signature.contains("->") && branches.is_empty() {
                    continue;
                }
                if *is_private {
                    continue;
                }
                model_fns.entry(name.clone()).or_insert(ModelFn {
                    module: module_name.clone(),
                    prefix: prefix.clone(),
                    proof: proof_status.clone(),
                });
            }
        }
    }

    // Index inventory by name for the join. Inventory entries with the
    // same `name` (e.g. cpp + rust mirror of the same function) collapse
    // to one ledger row, keyed by name, listing the first language seen.
    let mut inv_by_name: BTreeMap<String, &InventoryEntry> = BTreeMap::new();
    for entry in &inventory.functions {
        inv_by_name.entry(entry.name.clone()).or_insert(entry);
    }

    let mut entries: Vec<LedgerEntry> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut excluded: Vec<String> = Vec::new();

    // Model side first, so a name that is in both model and inventory
    // gets the model attribution (module + page link).
    for (name, mf) in &model_fns {
        if config.is_excluded(name) {
            excluded.push(name.clone());
            seen.insert(name.clone());
            continue;
        }
        let inv = inv_by_name.get(name).copied();
        let source = if inv.is_some() {
            LedgerSource::Both
        } else {
            LedgerSource::ModelOnly
        };
        let badge = classify(name, Some(&mf.proof), inv, config);
        entries.push(LedgerEntry {
            name: name.clone(),
            source,
            badge,
            module: Some(mf.module.clone()),
            module_prefix: Some(mf.prefix.clone()),
            impl_lang: inv.map(|e| e.lang.clone()),
            impl_symbol: inv.and_then(|e| e.symbol.clone()),
            impl_file: inv.and_then(|e| e.file.clone()),
            models: inv.and_then(|e| e.models.clone()),
            models_note: inv.and_then(|e| e.models_note.clone()),
            composes: inv.map(|e| e.composes.clone()).unwrap_or_default(),
            abstraction_note: config.abstraction_note(name).map(|s| s.to_string()),
            proof: mf.proof.clone(),
        });
        seen.insert(name.clone());
    }

    // Implementation-only side.
    for (name, entry) in &inv_by_name {
        if seen.contains(name) {
            continue;
        }
        if config.is_excluded(name) {
            excluded.push(name.clone());
            continue;
        }
        let badge = classify(name, None, Some(entry), config);
        entries.push(LedgerEntry {
            name: name.clone(),
            source: LedgerSource::ImplementationOnly,
            badge,
            module: None,
            module_prefix: None,
            impl_lang: Some(entry.lang.clone()),
            impl_symbol: entry.symbol.clone(),
            impl_file: entry.file.clone(),
            models: entry.models.clone(),
            models_note: entry.models_note.clone(),
            composes: entry.composes.clone(),
            abstraction_note: None,
            proof: None,
        });
    }

    // Stable ordering: badge severity then name. ⚠️ first so the gap is
    // immediately visible at the top of the matrix.
    entries.sort_by(|a, b| {
        badge_order(a.badge)
            .cmp(&badge_order(b.badge))
            .then_with(|| a.name.cmp(&b.name))
    });
    excluded.sort();
    excluded.dedup();

    Ledger { entries, excluded }
}

fn badge_order(b: CoverageBadge) -> u8 {
    match b {
        CoverageBadge::Unverified => 0,
        CoverageBadge::ProvenBounded => 1,
        CoverageBadge::Proven => 2,
        CoverageBadge::ModelAbstraction => 3,
        CoverageBadge::SpecOnly => 4,
    }
}

fn classify(
    name: &str,
    proof: Option<&Option<ProofStatus>>,
    inventory: Option<&InventoryEntry>,
    config: &CoverageConfig,
) -> CoverageBadge {
    // Explicit overrides win.
    if config.is_spec_only(name) {
        return CoverageBadge::SpecOnly;
    }
    if config.abstraction_note(name).is_some() {
        return CoverageBadge::ModelAbstraction;
    }

    // Proven (bounded or full) outranks everything else for a real claim.
    if let Some(Some(ProofStatus::Proven { iterations, .. })) = proof {
        return if iterations.is_some() {
            CoverageBadge::ProvenBounded
        } else {
            CoverageBadge::Proven
        };
    }

    // Implementation-side with no proof → unverified. Same for model-only
    // with Failed / NotAttempted / None — there's no positive coverage.
    let _ = inventory; // currently unused for classification, but kept in
    // the signature for future per-language nuance.
    CoverageBadge::Unverified
}

struct ModelFn {
    module: String,
    prefix: String,
    proof: Option<ProofStatus>,
}
