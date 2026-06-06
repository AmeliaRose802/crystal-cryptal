# One-pager: The Coverage Ledger — knowing the *full* set of real functions

**Status:** proposal · **Audience:** pretty-specs maintainers · **Owner:** (you)
**Companion:** [01-coverage-clarity.md](01-coverage-clarity.md) (how it's rendered)

## Problem

pretty-specs only knows about functions that appear in the **Cryptol model**.
That set is, by construction, a *subset* of the real codebase: it contains the
decision functions we chose to model, plus modeling scaffolding (placeholders,
ABI adapters). It cannot, on its own, tell you what real functions exist that
were **never modeled** — which is exactly the set a reader most needs to see to
avoid assuming full coverage.

To render the ⚠️ "implemented, unverified" set from one-pager #1, pretty-specs
needs a second input: the **full inventory of real implementation functions.**

## Proposal: join three sets into one ledger

Compute coverage from three sources that already exist, and treat the result —
not the Cryptol module alone — as the thing to render.

```
  M = model functions        ← Cryptol module (already parsed today)
  I = implementation funcs    ← clang AST (C++) + mir-json (Rust)
  P = proof results           ← proof_manifest.json  (status per fn)

  Ledger = (I ∪ M), each entry classified by membership + P:
     in I, proven in P, maps to M      → ✅ / 🔲   (real, verified)
     in M only, no real counterpart    → 🧩         (abstraction / adapter)
     in I, not proven                  → ⚠️         (real, unverified)  ← the gap
     in M, intentionally no impl        → 📄         (spec-only)
```

`I` is the new ingredient, and it is **free**: saw-spec-gen already walks the
clang AST and mir-json to generate specs, so it already enumerates every
function, its mangled symbol, signature, and source file. Emit that list as a
sidecar (`implementation_inventory.json`) next to `proof_manifest.json`. The
two together let pretty-specs build the ledger by set membership alone — no new
parsing in pretty-specs.

## Suggested sidecar shape

```jsonc
// implementation_inventory.json  (emitted by saw-spec-gen)
{
  "functions": [
    { "name": "hmac_sha256", "lang": "cpp", "symbol": "?hmac_sha256@sdep@@…",
      "file": "cpp/src/hmac.cpp", "models": "hmacSha256" },
    { "name": "canonicalizePayload", "lang": "cpp",
      "file": "cpp/src/canonical.cpp", "models": "canonicalize_lp",
      "models_note": "bounded model only" },
    { "name": "handle_provision", "lang": "cpp",
      "file": "cpp/src/controller.cpp", "composes": ["provisionKey"] }
    // …every real function, whether modeled or not…
  ]
}
```

`models` / `composes` are optional cross-links that drive the 🧩↔⚠️ hinge in
the rendered matrix. If absent, the function still appears — as ⚠️ with no
model — which is the safe default (a real function with no declared coverage
shows up as uncovered, never silently dropped).

## Keeping the ⚠️ list honest (and short)

Raw `I` contains noise you don't want flagged as "missing verification":
private helpers (`to_lower`, `trim`, `hex_nibble`), and ABI adapters. Add a
small, **explicit** allow/exclude file so suppression is a documented decision,
not an accident:

```toml
# coverage.toml  (checked in; consumed by pretty-specs)
[exclude]                       # not security-relevant; omit from ⚠️ count
functions = ["to_lower", "trim", "starts_with", "ends_with", "hex_nibble"]

[abstraction]                   # force-classify model defs as 🧩 with a note
hmacSha256       = "Algebraic placeholder; NOT SHA-256. Real HMAC unverified."
canonLenPrefixed = "Bounded fixed-width encoder; real string encoder unverified."
packOutcome      = "ABI adapter (i16 pack of enforceAccess result)."
```

Anything excluded is **counted in a footnote** ("+5 helpers excluded, see
coverage.toml"), so the suppression is visible rather than hidden. The default
for an unlisted real function is to appear as ⚠️ — you opt *out* of the gap
list explicitly, never *into* it by omission.

## Why default-visible matters

The current failure mode is silent omission: a function is uncovered *because
nobody modeled it*, and the docs never mention it. Inverting the default — real
functions are listed unless explicitly excluded — means adding a new endpoint
or crypto routine **automatically** shows up as ⚠️ until someone either proves
it or records a deliberate exclusion. The docs can no longer drift into
overselling just by the codebase growing.

## Acceptance criteria

1. saw-spec-gen emits `implementation_inventory.json` covering **all** C++ and
   Rust functions it sees (not just the ones it generates specs for).
2. pretty-specs builds the ledger from `I ∪ M` joined with `P`; functions in
   `I` with no proof and no exclusion render as ⚠️.
3. `coverage.toml` exclusions are honored **and** surfaced as a footnote count.
4. Adding an unmodeled, unexcluded function to the codebase makes the ⚠️ count
   go up on the next docs build with no other change.
