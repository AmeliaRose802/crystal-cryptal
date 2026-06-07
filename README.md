# pretty-specs

Renders Cryptol `.cry` specification files into clean, cross-linked Markdown
documentation. Optionally consumes proof results from
[SAW](https://saw.galois.com/) / Cryptol so verified properties and
implementation-vs-spec equivalence proofs are surfaced inline.

See [docs/usage.md](docs/usage.md) for the full CLI reference and the
[saw-spec-gen](https://github.com/AmeliaRose802/saw-spec-gen) pipeline
walkthrough.

## Installation

```bash
# Install globally
cargo install --path .

# Or build a release binary
cargo build --release
# Binary at target/release/pretty-specs
```

## Quick start

```bash
# Multi-file output (one page per function / property)
pretty-specs SDEP.cry -o docs/

# Multiple files or a directory of .cry modules (multi-module mode)
pretty-specs examples/ -o docs/
pretty-specs ModA.cry ModB.cry -o docs/

# Single Markdown file to stdout
pretty-specs SDEP.cry --single-file

# JSON IR to stdout (or to a file with -o)
pretty-specs SDEP.cry --emit-json

# Attach proof results from a manifest
pretty-specs SDEP.cry --proof-status manifest.json -o docs/

# Custom title, omit function bodies
pretty-specs SDEP.cry --title "My Protocol" --no-details -o docs/
```

### saw-spec-gen interop

```bash
# Emit a function inventory for saw-spec-gen
pretty-specs SDEP.cry --emit-function-list -o function_list.json

# Convert a raw SAW prove_print log into a proof manifest
pretty-specs --adapt-saw-log saw_output.txt --manifest-output proof_manifest.json

# Collect per-function saw-spec-gen result.json files into one manifest
pretty-specs --adapt-saw-results ./verify_out --manifest-output proof_manifest.json
```

The bundled [pipeline.ps1](pipeline.ps1) chains all of these steps end-to-end
(initial render → emit function list → run saw-spec-gen per function →
adapt results → re-render with badges). Run `Get-Help .\pipeline.ps1 -Full`
for parameters. To combine separate language runs into one manifest, pass one
or more `-MergeProofStatus` files; entries are merged by function name into
`functions.<name>.implementations`.

## CLI reference

| Flag | Description |
|---|---|
| `<INPUT...>` | One or more `.cry` files, or directories containing `.cry` files |
| `-o, --output <PATH>` | Output directory for docs; output file for `--emit-json`, `--emit-function-list`, `--adapt-*` |
| `--single-file` | Single Markdown file to stdout (single-module input only) |
| `--emit-json` | JSON IR instead of Markdown |
| `--emit-function-list` | JSON array of functions (name, signature, arity, doc summary) for saw-spec-gen |
| `--include-private` | With `--emit-function-list`, also include `private` declarations |
| `--no-details` | Omit function bodies and property explanations |
| `--proof-status <FILE>` | Proof-status JSON manifest (properties and/or functions) |
| `--adapt-saw-log <FILE>` | Parse a raw SAW `prove_print` / `prove` log → proof manifest |
| `--adapt-saw-results <DIR>` | Scan a directory for saw-spec-gen `result.json` files → proof manifest |
| `--manifest-output <FILE>` | Output path for `--adapt-saw-log` / `--adapt-saw-results` (default: `proof_manifest.json`) |
| `--title <TITLE>` | Document title (overrides the module name) |
| `--docfx` | Emit DocFX-compatible front-matter and `toc.yml` files |
| `--logo <PATH>` | Copy a logo image into `<output>/images/` (prints `_appLogoPath` snippet under `--docfx`) |
| `--favicon <PATH>` | Copy a favicon into `<output>/images/` (prints `_appFaviconPath` snippet under `--docfx`) |
| `--extra-docs <DIR[:NAME]>` | Include an extra directory of Markdown verbatim (see [Extra Docs](#extra-docs)). Repeatable. |

### Exit codes

| Code | Meaning |
|---|---|
| 0 | Success |
| 1 | Parse error |
| 2 | I/O error (cannot read input, cannot write output, missing args) |

## Output structure

Multi-file mode (`-o docs/`) generates, per module:

```
docs/
├── index.md              # Overview with table of contents
├── types.md              # Type aliases, enums, records
├── functions/
│   ├── index.md
│   ├── provisionKey.md   # One file per function
│   ├── authenticate.md
│   └── ...
└── properties/
    ├── index.md
    ├── key-lifecycle-safety.md   # One file per property group
    └── ...
```

Multi-module mode nests each module under its own subdirectory and adds a
top-level `index.md` with a Mermaid dependency graph; cross-module links are
resolved automatically.

Single-file mode (`--single-file`) writes one combined document to stdout.
JSON mode (`--emit-json`) writes the intermediate representation to stdout
or to the `-o` path if specified.

## Proof status integration

`--proof-status <FILE>` annotates properties and functions with verification
results. The manifest accepts both a structured form and a legacy flat form
(property-only):

```json
{
  "properties": {
    "P1": { "status": "proven", "solver": "z3", "time_secs": 0.42 },
    "P8": { "status": "assumed" },
    "P25": { "status": "not_attempted" },
    "P99": { "status": "failed", "reason": "counterexample found" }
  },
  "functions": {
    "provisionKey": {
      "implementations": {
        "cpp":  { "status": "proven", "solver": "z3", "impl_file": "sdep.cpp" },
        "rust": { "status": "not_attempted" }
      }
    }
  }
}
```

Legacy `overall`-only function entries are still accepted for backwards
compatibility.

Supported statuses: `proven`, `assumed`, `failed`, `not_attempted`.

Manifest entries with status `failed` or `not_attempted` that don't match any
property in the spec are still rendered as placeholder sections, so gaps are
never silently dropped.

## Extra Docs

Use `--extra-docs <DIR>` to ship additional hand-written Markdown pages
alongside the auto-generated spec docs. Each directory is copied verbatim
to `<output>/<basename>/`, preserving subdirectory structure. The flag is
repeatable.

```bash
# Drop docs/extra_guides/*.md into <output>/extra_guides/ and link from toc
pretty-specs SDEP.cry -o docs/ --docfx --extra-docs docs_src/extra_guides

# Multiple dirs + a custom toc label
pretty-specs SDEP.cry -o docs/ --docfx \
  --extra-docs docs_src/guides \
  --extra-docs "docs_src/tutorials:Tutorials"
```

Under `--docfx`, an entry is appended to the top-level `toc.yml`:

- If the directory contains a `toc.yml`, that file is used as the toc target.
- Otherwise an `index.md` at the root is used.
- If neither exists the files are still copied (so DocFX's content glob picks
  them up) but no navbar entry is added.

The display name comes from the optional `:NAME` suffix; otherwise the
basename is title-cased (`extra_guides` → `Extra Guides`). Hidden entries
(names starting with `.`) and symlinks are skipped.

## Architecture

```
.cry files ─▶ Lexer ─▶ Parser ─▶ IR ─▶ Linker ─▶ Renderer ─▶ output/
              (logos)  (lalrpop  (Vec<  (symbol   (Markdown
                       grammar)  Item>) table)    or JSON)
```

1. **Lexer** ([`src/lexer/`](src/lexer/mod.rs)) — tokenizes `.cry` source using
   [logos](https://crates.io/crates/logos), with a layout pass that injects
   virtual `{`/`;`/`}` tokens for layout-sensitive blocks.
2. **Parser** ([`src/parser/`](src/parser/mod.rs)) — a lalrpop grammar
   classifies declarations by leading keyword and returns byte-offset spans;
   Rust code then extracts names, signatures, and bodies into typed items.
3. **IR** ([`src/ir/`](src/ir/mod.rs)) — flat `Vec<Item>` (modules, type
   aliases, enums, records, functions, properties, sections, doc blocks).
4. **Linker** ([`src/linker/`](src/linker/mod.rs)) — builds a `SymbolTable`
   so functions, types, and properties cross-link to one another (single or
   multi-module).
5. **Renderer** ([`src/render_md/`](src/render_md/mod.rs),
   [`src/render_json.rs`](src/render_json.rs)) — emits Markdown (multi-file
   or single-file) or JSON. Cross-links symbols, renders decision tables for
   `if/then/else` functions, and attaches proof-status badges.

## Development

```bash
# Build, test, lint
cargo build --all-targets
cargo test --all
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --all

# Update snapshots after intentional output changes
cargo insta review

# Run a single snapshot test
cargo test snapshot_single_file
```

Snapshot tests live in [tests/snapshots.rs](tests/snapshots.rs) and use
[insta](https://crates.io/crates/insta) for approval-based testing. The
canonical fixture spec is [tests/fixtures/SDEP.cry](tests/fixtures/SDEP.cry).

### Pre-commit hook

```bash
git config core.hooksPath .githooks
```

This gates every commit on:

- `cargo build --all-targets` (zero warnings)
- `cargo clippy --all-targets` (zero clippy warnings)
- `cargo test` (all tests pass)
- Max 500 non-empty lines per `.rs` file

On Windows without Git Bash:

```powershell
powershell -File .githooks/pre-commit.ps1
```

CI ([`.github/workflows/ci.yml`](.github/workflows/ci.yml)) runs the same
checks on both Linux and Windows.

## License

See [Cargo.toml](Cargo.toml) for package metadata.
