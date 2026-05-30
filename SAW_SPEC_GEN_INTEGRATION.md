# saw-spec-gen × pretty-specs Integration Analysis

## Executive Summary

These two tools are two halves of the same coin:

- **pretty-specs** turns a Cryptol `.cry` file into human-readable documentation
- **saw-spec-gen** turns a C++/Rust implementation + the same `.cry` file into a machine-verified proof

The Cryptol spec is the shared single source of truth. The integration opportunity is a closed-loop pipeline where writing a `.cry` file once gives you both living documentation *and* verified implementation proofs — with the proof results flowing back into the docs automatically.

---

## What Each Tool Does Today

### pretty-specs (this repo)

```
SDEP.cry ──→ Lexer ──→ LALRPOP Parser ──→ IR (Vec<Item>) ──→ Linker (SymbolTable)
                                                │
                                    ┌───────────┴───────────┐
                                    ▼                       ▼
                              render_md              render_json
                           (multi/single)          (serialized IR)
                                    │
                         ┌──────────┼──────────┐
                         ▼          ▼          ▼
                    index.md    types.md   functions/   properties/
```

**Key data model:** `Item` enum with `Function { name, signature, branches, body }` and `Property { label, name, params, body, proof_status }`. Already has proof manifest support — loads a JSON file mapping `"P1" → { status: "proven", solver: "z3", time_secs: 0.42 }` and attaches badges to properties.

### saw-spec-gen

```
add_one.cpp ──→ clang -ast-dump ──→ saw-spec-gen gen-verify ──→ out/verify.saw
add_one.rs  ──→ rustc --emit=llvm-bc ─┘                         out/specs_experimental/
add_one_spec.cry ─────────────────────┘                          out/result.json
```

**Key data model:** Language-independent `FunctionInfo { name, params: Vec<ParamInfo>, return_type }` → constraint derivation → `SpecConstraint` → SAW script emission. The `gen-verify` subcommand is the full pipeline; the `coverage` subcommand reports which functions have been verified.

---

## Integration Points

### 1. Close the Proof Loop: `result.json` → `proof_manifest.json`

**The gap:** pretty-specs already consumes `--proof-status manifest.json`. saw-spec-gen already produces `result.json` with per-function verdicts. But the formats don't align — saw-spec-gen results are per-*function*, pretty-specs manifests are per-*property*.

**What to build:** A small adapter (can live in either repo) that:
1. Reads saw-spec-gen `result.json` (per-function SAT/UNSAT verdicts)
2. Reads pretty-specs JSON IR (via `--emit-json`) to get the property→function mapping from the linker's `related_properties`
3. Emits a `proof_manifest.json` in pretty-specs format

This is the single highest-value integration. Once connected:
```
.cry spec ──→ pretty-specs --emit-json ──→ function list
                                             │
  impl.cpp ──→ saw-spec-gen gen-verify ──────┘──→ result.json
                                                       │
                            adapter ◄──────────────────┘
                                │
                                ▼
                       proof_manifest.json
                                │
              pretty-specs --proof-status ◄────┘
                                │
                                ▼
                    docs with ✅/❌ badges
```

**Complexity:** Low. Both formats are simple JSON. The mapping logic is: "if function F is proven correct by saw-spec-gen, and property Pn references F in its body/doc, then Pn's proof status is at least `assumed` (the function behaves correctly, but the property itself may need a separate Cryptol-level proof)."

The distinction matters: saw-spec-gen proves *implementation matches spec*; Cryptol proofs (via `:prove`) prove *spec properties hold*. Both are needed. The manifest should track both:

```json
{
  "P1": {
    "status": "proven",
    "solver": "z3",
    "time_secs": 0.42,
    "source": "cryptol"
  },
  "enrollDevice": {
    "status": "proven",
    "solver": "z3",
    "time_secs": 1.8,
    "source": "saw",
    "impl_lang": "rust",
    "impl_file": "src/protocol.rs"
  }
}
```

pretty-specs would need a small extension to attach proof badges to *functions* too, not just properties. The `Item::Function` variant already has `doc: Vec<String>` but no `proof_status` field — adding one is trivial.

### 2. Feed Function Signatures to saw-spec-gen

**The gap:** When you run `saw-spec-gen gen-verify`, you pass `--cryptol-fn add_one_spec --function add_one` — you have to know which Cryptol function corresponds to which implementation function. This mapping is manual.

**What to build:** pretty-specs' JSON IR already contains every function name and its Cryptol signature. A lookup tool (or a manifest file) that maps implementation function names to their Cryptol spec counterparts:

```json
{
  "functions": {
    "provisionKey": { "cryptol_fn": "provisionKey", "signature": "Bit -> Bit -> KeyVaultResult -> Bit -> ProvisionResult" },
    "enrollDevice": { "cryptol_fn": "enrollDevice", "signature": "..." },
    "authenticate": { "cryptol_fn": "authenticate", "signature": "Bit -> Bit -> Bit -> Bit" }
  }
}
```

saw-spec-gen could consume this to auto-discover which functions to verify and which Cryptol function to check each against. A `--spec-manifest spec.json` flag on `verify.ps1` would eliminate the manual `--cryptol-fn` / `--function` pair.

### 3. Type Consistency Checking

**The gap:** pretty-specs knows the Cryptol types (`type FleetMode = [1]`, `type KeyVaultResult = [2]`). saw-spec-gen's constraint engine knows the C++/Rust types (`TypeInfo::Enum { discriminant_bits: 8, variants: [...] }`). Nobody checks that they agree.

**What to build:** A cross-checking mode that compares:
- Cryptol enum width (`[2]` = 2 bits) vs implementation discriminant width (`i8` = 8 bits)
- Cryptol record fields vs C++ struct layout
- Cryptol function arity vs implementation parameter count

This catches a class of bugs where the spec and implementation silently disagree on representation. pretty-specs' IR already has `EnumGroup { width, variants }` and saw-spec-gen's `TypeInfo::Enum { discriminant_bits, variants }` — the comparison is structural.

### 4. Coverage Dashboard

**The gap:** saw-spec-gen has a `coverage` subcommand that reports which functions have verified specs. pretty-specs renders function lists with cross-links. Neither shows the full picture: "of the 18 functions in SDEP.cry, 14 have verified C++ implementations, 6 have verified Rust implementations, and 22 of 25 properties are proven."

**What to build:** A coverage summary section in pretty-specs' `index.md`:

```markdown
## Verification Coverage

| Layer | Status | Detail |
|-------|--------|--------|
| Cryptol properties | 22/25 proven | 3 assumed |
| C++ implementation | 14/18 verified | `getStatus`, `hmacSha256`, `lpField`, `lpHeader` pending |
| Rust implementation | 6/18 verified | Pure decision functions only |
| Cross-language equivalence | 6/6 | All verified functions match |
```

This requires a richer manifest format that tracks per-function, per-language status. The `proof_manifest.json` extension from point 1 handles this naturally.

### 5. Unified CLI Pipeline

**What it looks like today** (manual, fragile):
```powershell
# Step 1: Generate docs
cargo run -- SDEP.cry -o docs/

# Step 2: Run Cryptol proofs (separate script)
cryptol -b verify_properties.cry > cryptol_results.txt

# Step 3: Run SAW proofs (per-function, separate repo)
cd ../saw-spec-gen
./verify.ps1 -CppFile impl.cpp -CryptolSpec SDEP.cry -CryptolFn provisionKey -Function provisionKey

# Step 4: Manually assemble proof_manifest.json

# Step 5: Regenerate docs with proof badges
cargo run -- SDEP.cry -o docs/ --proof-status proof_manifest.json
```

**What it should look like:**
```powershell
pretty-specs SDEP.cry \
    --output docs/ \
    --verify-cpp impl.cpp \
    --verify-rust src/protocol.rs \
    --proof-status auto
```

This is aspirational and doesn't need to be one binary — a wrapper script (`verify_all.ps1`, which you already have for demo_protocol) is fine. The key is that pretty-specs' `--emit-json` output drives saw-spec-gen's function list, and saw-spec-gen's results feed back into pretty-specs' proof manifest.

---

## Recommended Implementation Order

### Phase 1: Proof Manifest Bridge (low effort, high value)

1. **Add `proof_status: Option<ProofStatus>` to `Item::Function`** in pretty-specs IR. The renderer already handles badges for properties; extending to functions is ~20 lines.

2. **Extend manifest format** to accept function names as keys alongside property labels. `load_proof_manifest` already does `HashMap<String, ProofStatus>` — no schema change needed, just apply matching to functions too.

3. **Write a `saw-results-to-manifest` adapter** — a standalone script (PowerShell or Rust) that reads saw-spec-gen's `result.json` and emits pretty-specs' `proof_manifest.json`. Can live in either repo.

### Phase 2: Function Discovery (medium effort, medium value)

4. **Add a `--emit-function-list` flag** to pretty-specs that outputs a JSON array of `{ name, signature, arity }` for every `Item::Function`. This becomes saw-spec-gen's input for batch verification.

5. **Add `--spec-manifest` to saw-spec-gen** that reads the function list and iterates over all functions without requiring `--cryptol-fn` / `--function` per invocation.

### Phase 3: Type Cross-Check (medium effort, high value for correctness)

6. **Add a `--check-types` mode** to one of the tools that compares Cryptol enum widths and record layouts against implementation types. This is a static check — no SAW invocation needed.

### Phase 4: Coverage Rendering (low effort, nice-to-have)

7. **Extend `render_index`** to emit a coverage summary table when proof statuses are available. The data is already there from the manifest; it's just a rendering pass.

---

## Data Format Alignment

### Current pretty-specs manifest (`proof_manifest.json`):
```json
{
  "P1": { "status": "proven", "solver": "z3", "time_secs": 0.42 }
}
```

### Proposed extended manifest:
```json
{
  "properties": {
    "P1": { "status": "proven", "solver": "z3", "time_secs": 0.42, "source": "cryptol" }
  },
  "functions": {
    "provisionKey": {
      "cpp": { "status": "proven", "solver": "z3", "time_secs": 1.2, "impl_file": "sdep.cpp" },
      "rust": { "status": "proven", "solver": "z3", "time_secs": 0.8, "impl_file": "src/lib.rs" }
    },
    "enrollDevice": {
      "cpp": { "status": "not_attempted" }
    }
  }
}
```

For backward compatibility, the flat format (`"P1": { ... }`) should continue to work. The loader detects which format is in use by checking whether top-level keys are `"properties"` / `"functions"` (new) or property labels (old).

### saw-spec-gen `result.json` (current):
Need to verify the exact format, but based on the README it's a machine-readable verdict per verification run. The adapter maps these into the extended manifest above.

---

## Architecture Diagram

```
                    ┌─────────────────────────┐
                    │      SDEP.cry            │
                    │   (Cryptol spec —        │
                    │    single source         │
                    │    of truth)             │
                    └────────┬────────────────┘
                             │
              ┌──────────────┼──────────────┐
              ▼              ▼              ▼
     ┌────────────┐  ┌─────────────┐  ┌──────────────┐
     │pretty-specs│  │  Cryptol    │  │ saw-spec-gen │
     │            │  │  :prove     │  │              │
     │ Parse .cry │  │             │  │ gen-verify   │
     │ → IR       │  │ Verify     │  │ against      │
     │ → Markdown │  │ properties │  │ impl.cpp /   │
     │ → JSON IR  │  │ (P1–P25)   │  │ impl.rs      │
     └─────┬──────┘  └─────┬──────┘  └──────┬───────┘
           │                │                │
           │ emit-json      │ results        │ result.json
           ▼                ▼                ▼
     ┌─────────────────────────────────────────────┐
     │          proof_manifest.json                │
     │  (unified: properties + functions + langs)  │
     └────────────────────┬────────────────────────┘
                          │
                          ▼
              ┌───────────────────────┐
              │    pretty-specs       │
              │  --proof-status       │
              │                       │
              │  Docs with badges:    │
              │  ✅ P1 Proven (z3)    │
              │  ✅ provisionKey      │
              │     verified (C++)    │
              │  ⬚ hmacSha256        │
              │     not yet verified  │
              └───────────────────────┘
```

---

## What NOT to Do

1. **Don't make pretty-specs invoke saw-spec-gen directly.** They're separate tools with separate dependencies (SAW, clang, z3). The integration point is the manifest file, not process orchestration.

2. **Don't parse SAW scripts.** saw-spec-gen *emits* `.saw` files; pretty-specs should never need to read them. The interchange format is JSON.

3. **Don't duplicate the Cryptol parser.** saw-spec-gen doesn't parse `.cry` — it passes the file path to SAW. pretty-specs is the only Cryptol parser. If saw-spec-gen needs spec metadata, it should consume pretty-specs' JSON IR.

4. **Don't try to verify from within the doc renderer.** The tools have very different runtime requirements. Keep them loosely coupled through file-based interchange.

---

## Quick Wins Available Today (No Code Changes)

Even before building any of the above, the current tools can be chained manually:

```powershell
# 1. Generate the JSON IR
cargo run -- SDEP.cry --emit-json -o spec_ir.json

# 2. Extract function names for saw-spec-gen
$ir = Get-Content spec_ir.json | ConvertFrom-Json
$fns = $ir | Where-Object { $_.Function } | ForEach-Object { $_.Function.name }

# 3. Run saw-spec-gen for each function
foreach ($fn in $fns) {
    saw-spec-gen gen-verify --cryptol-spec SDEP.cry --cryptol-fn $fn --function $fn ...
}

# 4. Manually build proof_manifest.json from results
# 5. Re-render with badges
cargo run -- SDEP.cry -o docs/ --proof-status proof_manifest.json
```

This is exactly what `verify_all.ps1` in demo_protocol does. The integration work formalizes and automates this loop.
