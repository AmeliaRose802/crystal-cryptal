// IR: typed intermediate representation of a Cryptol spec.

use serde::{Deserialize, Serialize};

mod manifest;
#[cfg(test)]
mod tests;

pub use manifest::{ProofManifest, load_proof_manifest};

/// Proof status for a property, populated from an external proof manifest.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ProofStatus {
    Proven {
        solver: String,
        time_secs: Option<f64>,
        /// Functions whose specs were used as `*_unsafe_assume_spec` /
        /// `llvm_verify` overrides while discharging this proof. Each entry
        /// records a dependency of the verdict — the property/function is
        /// only as trustworthy as those overrides.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        overrides: Vec<String>,
        /// For bounded-loop proofs: the loop-unroll bound (or `MAX_LEN`) that
        /// the proof was discharged at. `None` for proofs that don't involve
        /// a bounded loop.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        iterations: Option<u64>,
        /// Copy-pasteable shell command that reproduces this proof from a
        /// clean checkout. Surfaced on the rendered page so readers can
        /// re-run the verification locally without grepping the pipeline.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        verify_command: Option<String>,
        /// Path (relative to the manifest) of the generated SAW script that
        /// drives this proof. Used as a fallback when `verify_command` is
        /// absent — the page can synthesise `saw <path>`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        verify_script: Option<String>,
    },
    Assumed,
    Failed {
        reason: String,
        /// Concrete counterexample emitted by the solver, when available
        /// (e.g. "x = 0, y = 1"). Rendered as a code block on the per-item
        /// page so readers can see the exact witness that broke the claim.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        counterexample: Option<String>,
        /// Excerpt of the verifier log / stderr surrounding the failure —
        /// useful when SAW errors out with a stack trace rather than a clean
        /// counterexample (e.g. memory-model failures, type mismatches).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        log_excerpt: Option<String>,
        /// Same as `ProofStatus::Proven::verify_command` — lets readers
        /// re-run a failing proof locally to inspect it interactively.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        verify_command: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        verify_script: Option<String>,
    },
    NotAttempted,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnumVariant {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Branch {
    pub condition: Option<String>, // None = "otherwise" / else
    pub result: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Item {
    Module {
        name: String,
        doc: Vec<String>,
    },
    Import {
        module_path: String,
        qualifier: Option<String>,
        hiding: Vec<String>,
    },
    Section {
        level: u8,
        title: String,
        doc: Vec<String>,
    },
    TypeAlias {
        name: String,
        width: String,
        doc: Vec<String>,
    },
    EnumGroup {
        type_name: String,
        width: String,
        variants: Vec<EnumVariant>,
        predicate: Option<String>,
        doc: Vec<String>,
    },
    RecordType {
        name: String,
        fields: Vec<(String, String)>,
        doc: Vec<String>,
    },
    Function {
        name: String,
        signature: String,
        branches: Vec<Branch>,
        body: String,
        doc: Vec<String>,
        proof_status: Option<ProofStatus>,
        /// True when the declaration appeared inside a Cryptol `private` block.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        is_private: bool,
    },
    Property {
        label: String,
        name: String,
        params: Vec<String>,
        body: String,
        doc: Vec<String>,
        proof_status: Option<ProofStatus>,
        /// True when the declaration appeared inside a Cryptol `private` block.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        is_private: bool,
    },
    CommentBlock {
        lines: Vec<String>,
    },
    ModuleParam {
        name: String,
        kind: ParamKind,
        signature: String,
        doc: Vec<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ParamKind {
    TypeParam,
    ValueParam,
    Constraint,
}
