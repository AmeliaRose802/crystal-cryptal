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

| Badge | Meaning | Example (this repo) |
|-------|---------|---------------------|
| ✅ **Proven** | Machine-checked equivalence: real impl ≡ model, **all inputs at the ABI width**. | `provisionKey`, `enforce_access` |
| 🔲 **Proven (bounded)** | Equivalence proven only up to a size bound; the general-`n` case is a **prose** structural argument, not a machine proof. | `canonicalize_lp` |
| 🧩 **Model abstraction** | A Cryptol definition with **no real implementation counterpart**: a placeholder, an uninterpreted function, or an ABI adapter. Must carry a "stands in for…" note. | `hmacSha256`, `canonLenPrefixed`, `packOutcome` |
| ⚠️ **Implemented, unverified** | Real production function that exists in the codebase with **no proof**. | `sha256`, `canonicalizePayload`, `handle_provision` |
| 📄 **Spec-only** | Lives in the model on purpose, has no implementation (e.g. `secure*` reference functions used to exhibit gaps). | `secureProvisionKey` |

Two rendering rules carry the whole message:

- **Every page declares its badge in the title and a one-line banner.** A 🧩
  page must say what it abstracts and what that costs, e.g.:
  > 🧩 **Model abstraction.** This is *not* SHA-256. It is an algebraic
  > placeholder used only for the equality reasoning in P8/P9. No production
  > function is proven equivalent to it. Real HMAC lives in `cpp/src/hmac.cpp`
  > (⚠️ unverified).

- **The site lists the ⚠️ set explicitly** instead of omitting it. A reader
  must be able to see, on one page, every real function that is *not* covered.

## New artifact: the Coverage Matrix page

A single generated `coverage.md` (linked from the site index) is the headline.
It is the **union** of the model functions and the real implementation
functions (see companion one-pager for how that union is computed), grouped by
badge, with counts up top:

```
Coverage summary
  ✅ Proven                6   (C++ decision core + Rust mirror)
  🔲 Proven (bounded)      1   (canonicalize_lp, ≤16-byte fields)
  🧩 Model abstraction     9   (HMAC algebra, canon stand-ins, ABI adapters)
  ⚠️ Implemented, unverified  21  (crypto, string canon, keystore, controller)
  📄 Spec-only             6   (secure* gap references)
```

| Function | Lang | Badge | Proof | Maps to model |
|----------|------|-------|-------|---------------|
| `provisionKey` | C++ | ✅ Proven | SAW+Z3, all i8 | `SDEP_cpp::provisionKey` |
| `canonicalize_lp` | C++ | 🔲 Bounded | SAW+Z3, ≤16B | `canonicalize_lp_ret` |
| `hmac_sha256` | C++ | ⚠️ Unverified | — | (abstracted as `hmacSha256`) |
| `canonicalizePayload` | C++ | ⚠️ Unverified | — | (bounded model only: `canonicalize_lp`) |
| `handle_provision` | C++ | ⚠️ Unverified | — | composes `provisionKey` |
| `is_valid_signature` | Rust | ⚠️ Unverified | — | (abstracted as `isValidSignature`) |
| … | | | | |

The 🧩→⚠️ link is the honesty hinge: every abstraction names the real
function it stands in for, and every real function names the abstraction (if
any) that models it. That makes "we modeled HMAC as an opaque function and did
**not** verify the real one" impossible to miss.

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
2. 🧩 pages carry a "stands in for / not the real X" banner; ⚠️ where the real
   one lives.
3. A `coverage.md` matrix exists, is linked from the index, and includes the
   ⚠️ set (real functions with no proof) — i.e. nothing real is omitted.
4. Counts at the top of `coverage.md` match `proof_manifest.json` + the
   implementation inventory (no hand-maintained numbers).
