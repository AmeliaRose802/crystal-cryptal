// Coverage taxonomy: the five-state badge and the reason codes for the
// implemented-but-unverified state.

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
    /// Assumed contract for a true external primitive/dependency.
    TrustedAssumption,
    /// A Cryptol abstraction with no real-code counterpart (placeholder,
    /// uninterpreted function, or ABI adapter). Carried in
    /// `coverage.toml [abstraction]`.
    AbiAdapter,
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
            CoverageBadge::TrustedAssumption => "🔒",
            CoverageBadge::AbiAdapter => "🧩",
            CoverageBadge::Unverified => "⚠️",
            CoverageBadge::SpecOnly => "📄",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            CoverageBadge::Proven => "Proven",
            CoverageBadge::ProvenBounded => "Proven (bounded)",
            CoverageBadge::TrustedAssumption => "Trusted assumption",
            CoverageBadge::AbiAdapter => "ABI adapter / stand-in",
            CoverageBadge::Unverified => "Implemented, unverified",
            CoverageBadge::SpecOnly => "Spec-only",
        }
    }
}

/// Reason codes for ⚠️ implemented-but-unverified entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CoverageReason {
    R1Unbounded,
    R2StlHeap,
    R3Stateful,
    R4Timing,
    R6Compositional,
}

impl CoverageReason {
    pub fn parse(input: &str) -> Option<Self> {
        let code = input.trim().to_ascii_uppercase();
        match code.as_str() {
            "R1" | "R1_UNBOUNDED" => Some(Self::R1Unbounded),
            "R2" | "R2_STL_HEAP" | "R2_STL-HEAP" => Some(Self::R2StlHeap),
            "R3" | "R3_STATEFUL" => Some(Self::R3Stateful),
            "R4" | "R4_TIMING" => Some(Self::R4Timing),
            "R6" | "R6_COMPOSITIONAL" => Some(Self::R6Compositional),
            _ => None,
        }
    }

    pub fn code(self) -> &'static str {
        match self {
            Self::R1Unbounded => "R1",
            Self::R2StlHeap => "R2",
            Self::R3Stateful => "R3",
            Self::R4Timing => "R4",
            Self::R6Compositional => "R6",
        }
    }

    pub fn short_label(self) -> &'static str {
        match self {
            Self::R1Unbounded => "unbounded",
            Self::R2StlHeap => "stl-heap",
            Self::R3Stateful => "stateful",
            Self::R4Timing => "timing",
            Self::R6Compositional => "compositional",
        }
    }
}
