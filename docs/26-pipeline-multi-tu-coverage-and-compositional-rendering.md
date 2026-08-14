# Proposal: pretty-specs pipeline — accurate multi-TU coverage + compositional rendering

**Status:** Bucket 1 implementation; Bucket 2 unblocked by saw-spec-gen PR 94
**Audience:** pretty-specs maintainers
**Companion:** `25-compositional-contract-overrides.md` (saw-spec-gen side)
**Date:** 2026-08-12

> **Upstream update:** saw-spec-gen PR 94 merged on 2026-08-13 UTC. Cross-TU
> callee contracts are now discovered automatically, so pretty-specs does not
> need to forward compose configuration. The new single-contract multi-effect
> model also lets new contracts use the implementation symbol as their Cryptol
> name. Inventory-based name mapping remains necessary for existing contracts
> whose model and implementation names differ.

---

## 1. Problem

`pretty-specs --pipeline` drives saw-spec-gen per model function across all
`--impl` TUs, then `--adapt-saw-results` aggregates the per-function
`result.json` into `proof_manifest.json` for rendering. Two behaviours make the
rendered coverage **under-report reality**:

1. **Declaration-only attempts counted as failures.** Every function is tried
   against every TU. A leaf defined in `decision.cpp` but *called* from
   `controller.cpp`/`auth.cpp` is `declare`-only in those caller modules, so the
   attempt returns `inconclusive`. That inconclusive currently overrides the
   `verified` result from the defining TU. Observed: 6 decision functions
   reported as failures purely because callers reference them
   (`Summary: 1 verified, 6 proof failures`), despite all 6 verifying against
   `decision.cpp`.

2. **Cryptol-fn ↔ C++-symbol name mismatch.** The pipeline uses the *model*
   function name as the C++ symbol. Where they differ, the function is reported
   "no C++ implementation" and never counted:
   - `keyStoreProvisionRet` → C++ `provision`
   - `keyStoreHasKeyRet`/`…IsActiveRet`/`…ActivateRet`/`…CurrentRet` → `hasKey`/`isActive`/`activate`/`current`
   - `canonicalize_lp_ret` → `canonicalize_lp`

   All of these verify via direct `verify-cpp`, but the pipeline can't render
   them, so the site shows ~6 instead of the true **13 verified**.

## 2. Work, split by dependency

### Bucket 1 — pretty-specs only, **independent, land now**
Operates on the `result.json` saw-spec-gen already produces correctly. No
saw-spec-gen change, no waiting.

- **Best-wins aggregation** (`--adapt-saw-results`): a function `verified` in
  *any* TU is `verified`; `inconclusive`/`not-present` from other TUs must not
  downgrade it. Precedence: `verified`/`disproved` (a real solver verdict) >
  `inconclusive` > `not_attempted`. This alone erases the 6 false failures.
- **Name mapping** (`--pipeline`): consume the `models` field already present in
  `implementation_inventory.json` (e.g. `provision` *models* `keyStoreProvisionRet`)
  to pass the correct `--function`/`--cryptol-fn` to saw-spec-gen. Renders the
  KeyStore five + `canonicalize_lp`.
- **Defining-TU targeting** (optional cleanup): rather than attempting each
  function against every `--impl`, attempt it against the TU that *defines* it
  (has a body). Avoids the wasted declaration-only attempts entirely and speeds
  up the pipeline. Best-wins aggregation is still the safety net.

Outcome: a regen renders all **13 verified** immediately — unblocks doc review
with no dependency on the compositional feature.

### Bucket 2 — pretty-specs follow-up, **upstream dependency now landed**

saw-spec-gen PR 94 implemented automatic compositional-contract discovery.
No explicit `compose` forwarding is required for conventionally named contracts;
the remaining pretty-specs work is target selection and provenance rendering.

- **Surface orchestrator targets.** Allow non-leaf functions
  (`FleetController::handle_provision`, `handle_activate`, the `getStatus`
  wiring) to be declared as verification targets in the pipeline.
- **Rely on automatic composition.** Invoke orchestrator targets normally;
  saw-spec-gen discovers reachable declaration-only callees with matching
  `<name>`/`<name>_spec` contracts and substitutes those contracts for havoc.
  Explicit saw-spec-gen config remains available for exceptional mappings.
- **Render composed results.** Show the orchestrator's status plus which
  callee contracts it assumed (provenance), so a green orchestrator visibly
  rests on already-proven leaves.

This is a thin forwarding/rendering layer; it can't precede the engine.

## 3. Sequencing

1. **Now:** Bucket 1 (best-wins aggregation + name mapping). Ships as its own
   PR; immediately gives accurate docs for the current 13.
2. **Follow-up, now unblocked:** add orchestrator target selection and render
  the automatic composed-contract provenance emitted by saw-spec-gen.

Keeping Bucket 1 as a standalone PR means doc accuracy does **not** wait on the
larger compositional work.

## 4. Acceptance / test hooks

- **Bucket 1 regression:** a fixture with a leaf defined in TU X and called from
  TU Y must render `verified` (not `failed`) after aggregation; a function whose
  Cryptol name differs from its C++ symbol (via `models`) must be attempted and
  rendered.
- **Bucket 2:** once the saw-spec-gen `double_it`/`double_plus_one` E2E
  (§5 of the companion doc) exists, the pipeline should render
  `double_plus_one` as `verified` in the compose case and `disproved` in the
  havoc case, with the assumed-contract provenance shown.

## 5. Payoff

- Bucket 1: the published coverage matrix reflects the true **13 verified**
  instead of ~6, with no false failures — no engine work required.
- Bucket 2: renders the end-to-end orchestrator proofs the compose feature
  unlocks, so the site shows the full chain from leaves to `FleetController`.
