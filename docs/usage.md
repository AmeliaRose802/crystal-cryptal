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
      "overall": { "status": "proven", "solver": "z3", "time_secs": 1.2 },
      "by_language": {
        "cpp": { "status": "proven", "solver": "z3", "time_secs": 1.2, "impl_file": "sdep.cpp" },
        "rust": { "status": "not_attempted" }
      }
    }
  }
}
```

Statuses: `proven`, `assumed`, `failed`, `not_attempted`.

A flat legacy format (`{"P1": {...}}` with no `"properties"` wrapper) is also accepted for property-only manifests.

Property entries that have status `failed` or `not_attempted` but no matching property in the spec are rendered as placeholder sections so they are never silently dropped.

## End-to-end pipeline with saw-spec-gen

The built-in `--pipeline` flag orchestrates all four steps in one
cross-platform command (no PowerShell required):

```bash
# Full pipeline (from source):
pretty-specs SDEP.cry --pipeline --impl sdep.cpp --saw-spec-gen "cargo run --" -o docs/

# Docs only (no verification):
pretty-specs SDEP.cry --pipeline --skip-verify --skip-adapt -o docs/

# Adapt existing results and re-render:
pretty-specs SDEP.cry --pipeline --skip-verify -o docs/
```

The steps it runs:

| Step | Command |
|------|---------|
| 0 | `pretty-specs SDEP.cry -o docs/` — initial render |
| 1 | `pretty-specs SDEP.cry --emit-function-list -o verify_out/function_list.json` |
| 2 | `saw-spec-gen verify-cpp` / `verify-rust ...` for each function in the list |
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

### In-spec `@coverage` directives

Classification can also be declared **inline in the Cryptol source**, next to
the definition it describes, via a `@coverage` doc-comment directive. This keeps
the trust boundary in one place (the model) instead of a parallel
`coverage.toml`, and — unlike a config entry — it works on `private` helpers,
which are otherwise hidden from the matrix.

```cryptol
private

  // Specs only compare HMAC outputs for equality; the body is a placeholder.
  // @coverage trusted: the real SHA-256 in cpp/src/hmac.cpp is not proven here.
  hmacSha256 : HmacKey -> Request -> HmacTag
  hmacSha256 k r = k ^ r ^ (r <<< 1)
```

Recognised kinds (case-insensitive, note optional after `:`):

| Directive | Badge | Aliases |
|-----------|-------|---------|
| `@coverage trusted: <note>` | 🔒 Trusted assumption | `assumption`, `override` |
| `@coverage abstraction: <note>` | 🧩 ABI adapter / stand-in | `adapter`, `abi`, `stand-in` |
| `@coverage spec-only` | 📄 Spec-only | `spec_only`, `spec` |
| `@coverage exclude` | *(dropped from matrix)* | `internal` |

The note (text after `:`) is shown in the per-page banner. Directive lines are
stripped from the rendered prose, so they never leak into a function's
description. An unrecognised kind is ignored (degrades to "no directive" rather
than a wrong badge). An in-spec directive and a `coverage.toml` entry are both
authorial overrides; if both classify the same function they agree by
construction — the directive is evaluated with the same precedence as the
config sections.


