# pretty-specs

Renders Cryptol `.cry` specification files into clean, cross-linked Markdown documentation.

## Installation

```bash
# Install globally
cargo install --path .

# Or build a release binary
cargo build --release
# Binary at target/release/pretty-specs
```

## Usage

```bash
# Multi-file output (one page per function, property, etc.)
pretty-specs SDEP.cry -o output/

# Single Markdown file to stdout
pretty-specs SDEP.cry --single-file

# JSON IR to stdout
pretty-specs SDEP.cry --emit-json

# Attach proof results from a SAW/Cryptol manifest
pretty-specs SDEP.cry --proof-status manifest.json -o docs/

# Custom title, omit function bodies
pretty-specs SDEP.cry --title "My Protocol" --no-details -o docs/
```

## CLI Reference

| Flag | Description |
|---|---|
| `<INPUT>` | Path to the `.cry` input file (required) |
| `-o, --output <DIR>` | Output directory (default: `./output`) |
| `--single-file` | Emit a single Markdown file instead of a directory |
| `--emit-json` | Emit JSON IR instead of Markdown |
| `--no-details` | Omit function bodies and property explanations |
| `--proof-status <FILE>` | Path to a proof-status JSON manifest |
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
| 2 | I/O error (cannot read input, cannot write output) |

## Output Structure

Multi-file mode (`-o docs/`) generates:

```
docs/
├── index.md              # Overview with table of contents
├── types.md              # Type aliases, enums, records
├── functions/
│   ├── provisionKey.md   # One file per function
│   ├── authenticate.md
│   └── ...
└── properties/
    ├── key-lifecycle-safety.md   # One file per property group
    ├── authentication-security.md
    └── ...
```

Single-file mode (`--single-file`) writes one combined document to stdout.

JSON mode (`--emit-json`) writes the intermediate representation as JSON to stdout (or to the `-o` path if specified).

## Proof Status Integration

Use `--proof-status <FILE>` to annotate properties with verification results. The manifest is a JSON object mapping property labels to status entries:

```json
{
  "P1": { "status": "proven", "solver": "z3", "time_secs": 0.42 },
  "P2": { "status": "proven", "solver": "z3", "time_secs": 0.15 },
  "P8": { "status": "assumed" },
  "P25": { "status": "not_attempted" },
  "P99": { "status": "failed", "reason": "counterexample found" }
}
```

Supported statuses: `proven`, `assumed`, `failed`, `not_attempted`. Properties not listed in the manifest are rendered without a status badge.

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

## Development Setup

Enable the pre-commit hook (one-time):

```bash
git config core.hooksPath .githooks
```

This gates every commit on:

- `cargo build --all-targets` (zero warnings)
- `cargo clippy --all-targets` (zero clippy warnings)
- `cargo test` (all tests pass)
- Max 500 non-empty lines per `.rs` file

To run the checks manually on Windows without Git Bash:

```powershell
powershell -File .githooks/pre-commit.ps1
```

## Architecture

```
.cry file ──> Lexer ──> Parser ──> IR ──> Linker ──> Renderer ──> output/
              (logos)   (line-     (typed  (symbol    (Markdown
               tokens)   level)    AST)    table)     or JSON)
```

1. **Lexer** — tokenizes `.cry` source using [logos](https://crates.io/crates/logos).
2. **Parser** — line-level structural parser that recognizes modules, types, enums, records, functions, properties, sections, and doc comments. Not a full Cryptol parser.
3. **IR** — flat list of typed `Item` nodes (see `src/ir.rs`).
4. **Linker** — builds a `SymbolTable` for cross-references between types, functions, and properties.
5. **Renderer** — emits Markdown (multi-file or single-file) or JSON. Cross-links symbols, renders decision tables for `if/then/else` functions, and formats proof status badges.

## Development

```bash
# Run tests
cargo test

# Run clippy
cargo clippy

# Update snapshots after intentional output changes
cargo insta review

# Run a single snapshot test
cargo test snapshot_single_file
```

Snapshot tests live in `tests/snapshots.rs` and use [insta](https://crates.io/crates/insta) for approval-based testing. The fixture spec is at `tests/fixtures/SDEP.cry`.

## License

See [Cargo.toml](Cargo.toml) for package metadata.
