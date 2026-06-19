# Usage Guide

## Basic commands

```bash
# Single module → multi-file output
pretty-specs SDEP.cry -o output/

# Directory or multiple files (multi-module)
pretty-specs examples/ -o output/
pretty-specs ModA.cry ModB.cry -o output/

# Single Markdown file to stdout
pretty-specs SDEP.cry --single-file

# JSON IR to stdout
pretty-specs SDEP.cry --emit-json

# Attach proof results
pretty-specs SDEP.cry --proof-status manifest.json -o docs/

# Generate a Coverage Matrix page (5-badge taxonomy) by joining the model
# with an implementation inventory + optional coverage config.
pretty-specs SDEP.cry \
    --proof-status manifest.json \
    --implementation-inventory implementation_inventory.json \
    --coverage-config coverage.toml \
    -o docs/

# Emit function inventory for saw-spec-gen
pretty-specs SDEP.cry --emit-function-list -o function_list.json

# Parse a raw SAW prove_print log → proof_manifest.json
pretty-specs --adapt-saw-log saw_output.txt --manifest-output proof_manifest.json

# Collect saw-spec-gen result.json files → unified proof manifest
pretty-specs --adapt-saw-results ./verify_out --manifest-output proof_manifest.json
```

## CLI Reference

| Flag | Description |
|---|---|
| `<INPUT...>` | `.cry` files or directories |
| `-o, --output <DIR/FILE>` | Output directory for docs; output file for `--emit-json`, `--emit-function-list`, `--adapt-*` |
| `--single-file` | Single Markdown file to stdout (single-module only) |
| `--emit-json` | JSON IR instead of Markdown |
| `--emit-function-list` | JSON array of functions (name, signature, arity, doc summary) |
| `--no-details` | Omit function bodies and property explanations |
| `--proof-status <FILE>` | Proof-status JSON manifest (properties and/or functions) |
| `--implementation-inventory <FILE>` | Implementation inventory JSON (saw-spec-gen sidecar). Triggers `coverage.md` rendering. Auto-detected next to `--proof-status` if a file named `implementation_inventory.json` sits there. |
| `--coverage-config <FILE>` | TOML config declaring `[exclude]`, `[abstraction]`, `[spec_only]` for the coverage matrix. Auto-detected as `coverage.toml` in cwd. |
| `--adapt-saw-log <FILE>` | Parse raw SAW `prove_print`/`prove` log → proof manifest |
| `--adapt-saw-results <DIR>` | Scan directory for saw-spec-gen `result.json` files → proof manifest |
| `--manifest-output <FILE>` | Output path for `--adapt-saw-log` / `--adapt-saw-results` (default: `proof_manifest.json`) |
| `--title <TITLE>` | Document title (overrides module name) |

### Exit codes

| Code | Meaning |
|---|---|
| 0 | Success |
| 1 | Parse error |
| 2 | I/O error (cannot read input, cannot write output, missing args) |

## Output structure

Single-module:

```
output/
├── index.md
├── types.md
├── functions/
│   ├── index.md
│   └── {name}.md
└── properties/
    ├── index.md
    └── {category}.md
```

Multi-module nests each module under its name and adds a root `index.md` with a Mermaid dependency graph. Cross-module links are resolved automatically.

## Proof status manifest format

```json
{
  "properties": {
    "P1": { "status": "proven", "solver": "z3", "time_secs": 0.42 }
  },
  "functions": {
    "provisionKey": {
      "implementations": {
        "cpp": { "status": "proven", "solver": "z3", "time_secs": 1.2, "impl_file": "sdep.cpp" },
        "rust": { "status": "not_attempted" }
      }
    }
  }
}
```

Statuses: `proven`, `assumed`, `failed`, `not_attempted`.

A flat legacy format (`{"P1": {...}}` with no `"properties"` wrapper) is also accepted for property-only manifests.
Legacy `overall`-only function entries are also accepted.

Property entries that have status `failed` or `not_attempted` but no matching property in the spec are rendered as placeholder sections so they are never silently dropped.

## End-to-end pipeline with saw-spec-gen

The `pipeline.ps1` script at the repo root orchestrates all four steps:

```powershell
# Full pipeline (from source):
.\pipeline.ps1 -Spec SDEP.cry -Impl sdep.cpp -PrettySpecs "cargo run --" -Output docs/

# Docs only (no verification):
.\pipeline.ps1 -Spec SDEP.cry -SkipVerify -SkipAdapt -Output docs/

# Adapt existing results and re-render:
.\pipeline.ps1 -Spec SDEP.cry -SkipVerify -Output docs/

# Merge a second language manifest into the main manifest:
.\pipeline.ps1 -Spec SDEP.cry -SkipVerify -SkipAdapt `
  -ManifestOutput proof_manifest.json `
  -MergeProofStatus proof_manifest_rust.json -Output docs/
```

The steps it runs:

| Step | Command |
|------|---------|
| 0 | `pretty-specs SDEP.cry -o docs/` — initial render |
| 1 | `pretty-specs SDEP.cry --emit-function-list -o verify_out/function_list.json` |
| 2 | `saw-spec-gen gen-verify ...` for each function in the list |
| 3 | `pretty-specs --adapt-saw-results verify_out/ --manifest-output proof_manifest.json` |
| 4 | `pretty-specs SDEP.cry --proof-status proof_manifest.json -o docs/` — badged render |

## `result.json` format for `--adapt-saw-results`

Each `result.json` produced by saw-spec-gen (one per `out_{fn}/`) must contain:

```json
{
  "cryptol_fn": "provisionKey",
  "status": "verified",
  "solver": "z3",
  "time_secs": 1.2,
  "impl_lang": "cpp",
  "impl_file": "sdep.cpp",
  "message": null
}
```

`status` values: `verified` → proven; `counterexample`/`invalid`/`sat` → failed; `timeout`/`error` → failed; anything else → not_attempted.

If `cryptol_fn` is absent, the adapter falls back to the `function` field, then to the parent directory name (`out_provisionKey` → `provisionKey`).

## Coverage Matrix (5-badge taxonomy)

When `--implementation-inventory` (or `--coverage-config`) is supplied, the
renderer joins the Cryptol model with the production codebase and emits an
extra page at `<output>/coverage.md` that classifies every function under one
of five badges:

| Badge | Meaning |
|-------|---------|
| ✅ | **Proven** — model proved + implementation verified against it. |
| 🔲 | **Proven (bounded)** — proved up to a bound (e.g. `MAX_LEN ≤ 16`). |
| 🧩 | **Model abstraction** — model is a non-executable spec (e.g. `hmacSha256` is uninterpreted). Declared in `coverage.toml`. |
| ⚠️ | **Implemented, unverified** — real code with no proof. *This is the gap.* |
| 📄 | **Spec-only** — model exists, no implementation yet. Declared in `coverage.toml`. |

Per-page badges and an info banner are rendered on each function's page; the
module `index.md` also gains a "Coverage at a glance" section linking to the
matrix.

### `implementation_inventory.json`

```json
{
  "functions": [
    { "name": "provisionKey",  "lang": "cpp",  "symbol": "...", "file": "cpp/src/decisions.cpp" },
    { "name": "canonicalize_lp", "lang": "cpp", "file": "cpp/src/canonical.cpp",
      "models": "canonLenPrefixed", "models_note": "bounded model only" },
    { "name": "handle_provision", "lang": "cpp", "file": "cpp/src/controller.cpp",
      "composes": "provisionKey" }
  ]
}
```

Auto-detected as `implementation_inventory.json` next to `--proof-status` if
not passed explicitly (this is the sidecar saw-spec-gen already writes).

### `coverage.toml`

```toml
[exclude]
functions = ["to_lower", "trim"]   # internal helpers — drop from matrix

[abstraction]
hmacSha256 = "Model is uninterpreted; OpenSSL EVP_HMAC is trusted."
canonLenPrefixed = "Bounded model (MAX_LEN ≤ 16); production handles arbitrary length."

[spec_only]
functions = []                     # specs not yet implemented
```

Auto-detected as `coverage.toml` in the cwd if not passed explicitly.

If neither input is present, the renderer falls back to the legacy `✓/✗/~`
glyphs and skips the matrix page (fully backward-compatible).
