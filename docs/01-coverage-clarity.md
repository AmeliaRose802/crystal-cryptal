# One-pager: Making "which functions are actually covered" obvious in pretty-specs docs

**Status:** proposal · **Audience:** pretty-specs maintainers · **Owner:** (you)
**Companion:** [02-coverage-ledger.md](02-coverage-ledger.md) (where the data comes from)

## Problem

pretty-specs renders one page per **Cryptol model function** and stamps a `✓`
on the ones with a proof. That produces a tidy, all-green site — and it
quietly oversells, in two distinct ways:

1. **Abstractions look like verified code.** `hmacSha256` in the model is a
   placeholder algebra (`k ^ r ^ (r <<< 1)`), not SHA-256. `canonLenPrefixed`
   is a bounded, fixed-width stand-in for a real string encoder. These render
   as ordinary function pages with the same `✓` as `provisionKey`, even though
   nothing real was proven equivalent to them.

2. **Unverified production functions are invisible.** The docs iterate the
   *model*, so every real function that has **no** model — `sha256`,
   `hmac_sha256`, `constant_time_equals`, the string `canonicalizePayload`,
   `isValidClaims`, the key store, UUID parsing, the whole HTTP controller —
   simply never appears. A reader sees green checks and assumes the protocol
   is verified end-to-end. It isn't.

The fix is not to prove more (that's a separate effort) — it's to make the
docs **state the boundary** honestly and legibly.

## Proposal: a five-state coverage taxonomy + visible "not covered" set

Replace the single `✓` with an explicit badge drawn from one vocabulary,
shown in the page title, the TOC, and a per-page banner.

Only the first two badges are a verification claim. The rest exist to say, in
distinct ways, "**this is not a proof**" — a *trusted assumption*, a *mechanical
adapter*, or a *real function with no proof yet* — and none of them may render
as a `✓`.

| Badge | Verification claim? | Meaning | Example (this repo) |
|-------|:---:|---------|---------------------|
| ✅ **Proven** | yes | Machine-checked equivalence: real impl ≡ model, **all inputs at the ABI width**. | `provisionKey`, `enforce_access` |
| 🔲 **Proven (bounded)** | partial | Equivalence proven, but **only up to an explicit size bound** that is rendered on the badge. The general-`n` case is a prose structural argument, not a machine proof. | `canonicalize_lp` (≤16 B) |
| 🔒 **Trusted assumption** | no — *by design* | An **assumed spec** for a true external/primitive dependency we choose to trust rather than re-verify (e.g. HMAC-SHA256). Legitimate compositional-verification primitive (`llvm_unsafe_assume_spec`), **but** every proof that uses it inherits the assumption, and the docs must say so. | `hmacSha256`, `isValidSignature` (model) |
| 🧩 **ABI adapter / bounded stand-in** | no | A modeling-only Cryptol definition: an ABI width adapter or a fixed-width stand-in for a real encoder. Nothing real is proven equal to *it*; it exists to make the verifiable core statable. | `packOutcome`, `canonLenPrefixed` |
| ⚠️ **Implemented, unverified** | no | Real production function with **no proof**, carrying a *reason code* (below) for why. | `sha256`, `canonicalizePayload`, `handle_provision` |
| 📄 **Spec-only** | n/a | Lives in the model on purpose, has no implementation (e.g. `secure*` reference functions used to exhibit gaps). | `secureProvisionKey` |

Three rendering rules carry the whole message:

- **🔒 and 🧩 must not look like a pass.** Today they render with the same `✓`
  as `provisionKey`, which is the core oversell. A 🔒 page states the trust
  boundary in the imperative:
  > 🔒 **Trusted assumption — not proven here.** The HMAC primitive is treated
  > as a trusted external dependency with an assumed contract (right tag
  > verifies, wrong tag rejects). The real `cpp/src/hmac.cpp` SHA-256 is *not*
  > checked against this; every property that reaches it (P8, P9, …) inherits
  > the assumption that HMAC behaves correctly.

- **Every ⚠️ page names *why* it isn't proven** using a reason code, so
  "unverified" is never confused with "the property isn't a value property at
  all" or "this is blocked only by a dependency."

- **The site lists the 🔒, 🧩, and ⚠️ sets explicitly** instead of omitting
  them. A reader must see, on one page, every assumption and every real function
  that is *not* covered.

## Why a real function isn't proven — reason codes

"Unverified" is not one thing. Each ⚠️ function carries a reason code so the
docs explain the *obstacle* and what would lift it — not just the absence.

| Code | Obstacle | Why it blocks a proof — and the escape hatch | Functions |
|------|----------|----------------------------------------------|-----------|
| **R1 unbounded** | Loop trip count depends on runtime input length. | SAW bit-blasts a **fixed-size** formula — no native loop induction. **Bounded verification is available** (fix `MAX_LEN`, unroll), and is exactly what `canonicalize_lp` does; the bound and its cost are documented in [sdep-in-action](../../extra_docs/sdep-in-action.md). For ∀`n` there are two real options: SAW's **loop-invariant / fixpoint** prover (`llvm_verify_fixpoint`), or an induction prover (Dafny/F\*/Lean/Coq). So R1 *alone* is rarely the true wall — it is surmountable by bounding. | `sha256`, `hmac_sha256`, `canonicalize*`, `isValidClaims` |
| **R2 stl-heap** | Uses `std::string` / `std::map` / `std::sort` / `std::optional`. | At `-O0` these emit allocator, red-black-tree, iterator, and comparator calls SAW's symbolic memory model can't execute without a large override library (the "std types & strings" wall). **This — not R1 — is the real blocker for the string canonicalizers.** Lift it with override specs for the STL internals, or by verifying a fixed-width core (the `canonicalize_lp` strategy). | `canonicalize*`, `isValidClaims`, `KeyStore::*`, `Uuid::*` |
| **R3 stateful** | Mutates member state rather than mapping input→output. | The pure-equivalence pipeline proves `f(x) == model(x)` and has no vocabulary for "this method mutates a `std::optional<EnrollmentKey>` member." Needs a *stateful pre/post spec* over the heap. **This is a saw-spec-gen capability gap** — see [03-stateful-method-specs](03-stateful-method-specs.md). | `KeyStore::provision`, `KeyStore::activate` |
| **R4 timing** | The guarantee is about *execution* (which blocks/addresses), not the returned *value*. | Cryptol value-equivalence is the wrong layer: `constant_time_equals` and a naïve early-exit compare are the **same Cryptol function** `\a b -> a == b`. We *can* still prove its **functional correctness** (returns true iff `a == b`) today. The timing property needs a **leakage model** — see the toolbox note below. | `constant_time_equals` |
| **R6 compositional** | Top-level orchestrator that depends on still-blocked leaves. | `handle_provision` / `handle_activate` / `handle_status` route strings (R2) and call canonicalize → hmac → keystore → decision. **Exceptions are *not* the blocker** — saw-spec-gen lowers `throw`/`catch` to ordinary control flow ([exception-lowering](../../extra_docs/exception-lowering.md)). The real blocker is that they're verifiable only **compositionally**, once each leaf (R1/R2/R3) has a spec. | `FleetController::handle_*` |

> **What happened to R5?** "No reference model" is *not* a defect — for a
> primitive like HMAC it is the **right** call: treat it as a 🔒 trusted
> external dependency with an assumed contract, exactly as one assumes a
> correct AES or a correct allocator. It moves out of the "unverified" wall and
> into "trusted boundary, clearly labeled."

The codes compose and triage cleanly: the string canonicalizers are **R2** (not
really R1 — bounding handles the loop); `KeyStore::activate` is **R2+R3**;
`Uuid::from_string` is **R2** with an (already-lowered) exception path;
`handle_*` is **R6** over those leaves. None of them are blocked by exceptions,
and none are blocked by loops *per se*.

### Constant-time (R4): what the field actually does

Proving constant-time is a well-studied problem; value-level Cryptol just isn't
the tool. The standard approaches, any of which the docs can point to instead of
implying it's unprovable:

- **Enforce by construction (most common in crypto libs).** A "secret integer"
  type that forbids branching and array-indexing on secret data, so CT is a
  *type-system* guarantee: HACL\*/F\* `secret_int`, the FaCT language,
  libsodium's coding discipline.
- **Leakage-model / product-program verification.** Tools that build a
  self-composition and assert the *observation trace* (branch conditions +
  memory addresses) is independent of secret inputs: **ct-verif** (over LLVM),
  **Binsec/Rel**, **Jasmin + EasyCrypt** (down to assembly), **Vale**. This is
  expressible on SAW's Crucible backend (track a secret taint, assert the path
  condition and address stream don't depend on it) — it just isn't wired up
  here.
- **Dynamic/statistical** (not formal, but cheap): `dudect`, `ctgrind`.

So R4's honest docs line is: *functional correctness is provable today; the
timing property is out of scope for the Cryptol-equivalence pipeline and belongs
to a leakage-model tool or an enforce-by-construction type discipline.*

## New artifact: the Coverage Matrix page

A single generated `coverage.md` (linked from the site index) is the headline.
It is the **union** of the model functions and the real implementation
functions (see companion one-pager for how that union is computed), grouped by
badge, with counts up top:

```
Coverage summary
  ✅ Proven                6   (C++ decision core + Rust mirror)
  🔲 Proven (bounded)      1   (canonicalize_lp, ≤16-byte fields)
  🔒 Trusted assumption    2   (HMAC algebra, isValidSignature) — assumed contracts
  🧩 ABI adapter/stand-in  7   (canon stand-ins, ABI adapters)
  ⚠️ Implemented, unverified  21  (crypto, string canon, keystore, controller)
  📄 Spec-only             6   (secure* gap references)
```

| Function | Lang | Badge | Why / proof | Maps to model |
|----------|------|-------|-------------|---------------|
| `provisionKey` | C++ | ✅ Proven | SAW+Z3, all i8 | `SDEP_cpp::provisionKey` |
| `canonicalize_lp` | C++ | 🔲 Bounded ≤16B | SAW+Z3, MAX_LEN=16 | `canonicalize_lp_ret` |
| `hmacSha256` | model | 🔒 Trusted | **assumed contract** | — (HMAC abstraction) |
| `hmac_sha256` | C++ | ⚠️ Unverified | R2 (string blocks; loop is boundable) | trusts `hmacSha256` |
| `constant_time_equals` | C++ | ⚠️ Unverified | R4 (timing — value-correctness *is* provable) | — |
| `canonicalizePayload` | C++ | ⚠️ Unverified | R2 (STL); R1 boundable | bounded model only: `canonicalize_lp` |
| `KeyStore::activate` | C++ | ⚠️ Unverified | R2+R3 (stateful — see 03) | — |
| `handle_provision` | C++ | ⚠️ Unverified | R6 (composition; exceptions lower fine) | composes `provisionKey` |
| `is_valid_signature` | Rust | ⚠️ Unverified | R2 | trusts `isValidSignature` |
| … | | | | |

The 🔒/🧩→⚠️ link is the honesty hinge: every assumption names the real
function it stands in for, and every real function names the assumption (if
any) that models it. That makes "we *trusted* HMAC and did **not** verify the
real one" impossible to miss.

## Why this is low-cost

- The badge data already exists: `proof_manifest.json` gives ✅/🔲/none, and
  the full real-function inventory is already extracted by saw-spec-gen from
  the clang AST and mir-json (see companion). pretty-specs just has to **join
  and classify**, not analyze anything new.
- No proofs change. This is a rendering + manifest change only.
- It converts the docs from "everything we modeled is green" into "here is the
  exact boundary of what is proven," which is the claim we can actually defend.

## Acceptance criteria

1. Every function page shows exactly one badge from the taxonomy in its title.
2. **No 🔒, 🧩, or ⚠️ page renders a `✓`.** 🔒 pages carry a "trusted
   assumption — not proven here" banner naming the real function they stand in
   for; ⚠️ pages carry a reason code (R1–R4, R6) and link to where the real
   function lives.
3. Every 🔲 badge renders its explicit **bound** (e.g. `≤16B`), and every ⚠️
   function in `coverage.md` shows at least one reason code, so neither
   "bounded" nor "unverified" is ever ambiguous about its limit/obstacle.
4. A `coverage.md` matrix exists, is linked from the index, and includes the
   🔒, 🧩, and ⚠️ sets — i.e. no assumption and nothing real is omitted.
5. Counts at the top of `coverage.md` match `proof_manifest.json` + the
   implementation inventory (no hand-maintained numbers).
