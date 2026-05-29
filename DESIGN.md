# `pretty-specs` — Cryptol-to-Markdown Renderer

## Problem

Cryptol specifications are machine-provable but hard for humans to read. Engineers who need to review, audit, or understand a protocol's security properties shouldn't have to learn Cryptol syntax to do it. We want a CLI tool that takes a `.cry` file and emits a clean, readable Markdown document.

## Goal

```
pretty-specs SDEP.cry -o output/
```

Input: any well-structured Cryptol spec file.  
Output: a directory of cross-linked Markdown documents a staff engineer or security reviewer can navigate cold — one page per function, properties grouped by category, types on their own page, and an index that ties it all together.

---

## Cryptol Constructs We Need to Handle

Based on the example spec (`SDEP.cry`), the tool must recognise and render these Cryptol constructs:

| Construct | Cryptol pattern | Markdown rendering |
|---|---|---|
| **Module declaration** | `module Foo where` | H1 title |
| **Section comments** | `// ---- Category A: ...` or `////...` blocks | H2/H3 headings |
| **Prose comments** | `// This module specifies...` | Body paragraphs |
| **Type aliases (enums)** | `type FleetMode = [1]` + named constants | Enum table with name, value, bit-width |
| **Well-formedness predicates** | `isFleetMode m = ...` | Folded into enum table as "valid values" |
| **Record types** | `type EnrollmentStatus = { ... }` | Field table |
| **Type aliases (scalars)** | `type Timestamp = [64]` | Inline definition in a types section |
| **Functions** | `provisionKey : ... -> ...` + body | Decision table or prose summary |
| **Properties** | `property P1_KeyMonotonicity ...` | Plain-English security guarantee card |
| **Bounded-model / history comments** | Multi-line `//` blocks before properties | Rendered as explanatory notes/asides |

---

## Architecture

```
┌──────────────┐     ┌───────────┐     ┌──────────┐     ┌───────────┐     ┌──────────┐
│  .cry file   │────>│  Lexer /  │────>│  IR      │────>│  Link     │────>│ Markdown │───> output/
│  (UTF-8)     │     │  Parser   │     │  (typed  │     │  Resolver │     │ Emitter  │    ├─ index.md
└──────────────┘     └───────────┘     │   AST)   │     └───────────┘     └──────────┘    ├─ types.md
                                       └──────────┘                                      ├─ functions/
                                                                                          └─ properties/
```

### Phase 1 — Lexer / Parser

We do **not** need a full Cryptol parser. Cryptol's grammar is complex (dependent types, type-level arithmetic, implicit arguments). A full parse would be a multi-month project and we'd be reinventing `cryptol-server`.

Instead we build a **lightweight structural parser** that recognises the constructs in the table above by line-level pattern matching on a token stream. This is the 80/20: the constructs that appear in real specs are a small, predictable subset of the full grammar.

Specifically:
- Lex the file into tokens (keywords, operators, identifiers, literals, comments, newlines).
- Recognise top-level declarations by keyword anchors: `module`, `type`, `property`, and bare identifiers at column 0 followed by `:` (function signatures).
- Parse function bodies only deep enough to extract `if/then/else` chains (the dominant pattern in decision-logic specs). We don't need to handle arbitrary Cryptol expressions — just enough to build decision tables.
- Collect comment blocks and associate them with the declaration they precede.

**Why not use an existing Cryptol parser?**  
There is no published Cryptol parser crate for Rust (or any language outside the Cryptol compiler itself, which is Haskell). The `cryptol-remote-api` JSON interface could give us an AST, but it requires a running Cryptol server, a Haskell toolchain, and doesn't preserve comments (which are critical for our output). A line-level structural parser is dramatically simpler, faster to build, and sufficient.

### Phase 2 — Intermediate Representation

The parser emits a flat list of `Item` nodes:

```rust
enum Item {
    Module { name: String, doc: Vec<String> },
    Section { level: u8, title: String, doc: Vec<String> },
    TypeAlias { name: String, width: u32, doc: Vec<String> },
    EnumGroup {
        type_name: String,
        width: u32,
        variants: Vec<EnumVariant>,  // (name, value)
        predicate: Option<String>,   // isFleetMode body
        doc: Vec<String>,
    },
    RecordType {
        name: String,
        fields: Vec<(String, String)>,  // (field_name, type)
        doc: Vec<String>,
    },
    Function {
        name: String,
        signature: String,
        branches: Vec<Branch>,  // condition -> result
        doc: Vec<String>,
    },
    Property {
        label: String,          // "P1"
        name: String,           // "KeyMonotonicity"
        params: Vec<String>,
        body: String,           // raw Cryptol for fallback
        doc: Vec<String>,
        proof_status: Option<ProofStatus>,  // populated by --proof-status
    },
    CommentBlock { lines: Vec<String> },  // standalone prose
}
```

/// Proof status for a property, populated from an external proof manifest.
#[derive(Debug, Clone, Serialize, Deserialize)]
enum ProofStatus {
    Proven { solver: String, time_secs: Option<f64> },
    Assumed,
    Failed { reason: String },
    NotAttempted,
}
```

The IR is intentionally flat — no nested scopes, no expression trees beyond `if` chains.

#### Proof-status integration (future, designed now)

The `proof_status` field on `Property` is `Option` — `None` by default, populated when the user supplies a **proof manifest** via `--proof-status <manifest.json>`. The manifest is a simple JSON map from property label to status:

```json
{
  "P1": { "status": "proven", "solver": "z3", "time_secs": 0.42 },
  "P5": { "status": "proven", "solver": "z3", "time_secs": 1.87 },
  "P23": { "status": "proven", "solver": "z3", "time_secs": 812.3 },
  "P8": { "status": "assumed" },
  "P22": { "status": "not_attempted" }
}
```

This manifest is the contract between `pretty-specs` and the `saw-spec-gen` ecosystem. `saw-spec-gen` (or a SAW wrapper script) produces the manifest after running proofs; `pretty-specs` consumes it. The tool doesn't need to know anything about SAW — it just reads JSON.

When proof status is available, the emitter renders a badge next to each property:
- **Proven** → `✅ Proven (z3, 0.42s)`
- **Assumed** → `⚠️ Assumed (not machine-checked)`
- **Failed** → `❌ Failed: <reason>`
- **Not attempted** → `⬚ Not yet verified`

The `index.md` TOC gains a summary row: `22/25 properties proven, 1 assumed, 2 not attempted`.

### Phase 2.5 — Link Resolution

Before emitting, a resolution pass walks the IR and builds a **symbol table** mapping every type name, function name, and property label to the output file + anchor where it will be rendered. Then, when any doc comment or rendered text mentions a known symbol (e.g. `ProvisionResult`, `enrollDevice`, `P5`), the emitter replaces it with a relative Markdown link:

- `ProvisionResult` → `[ProvisionResult](../types.md#provisionresult)`
- `enrollDevice` → `[enrollDevice](../functions/enrollDevice.md)`
- `P5` → `[P5](../properties/key-lifecycle-safety.md#p5--disabled-rejects-all)`

This makes every page navigable without needing to ctrl-F a monolith.

### Phase 3 — Markdown Emitter (multi-file)

The emitter writes to an **output directory**, not a single file. Each IR node maps to a specific output file:

| IR node | Output file | Content |
|---|---|---|
| `Module` | `index.md` | H1 title, intro paragraph, table of contents with links to all other pages |
| `Section` | *(routing key)* | Determines which property-group file subsequent properties land in |
| `EnumGroup` | `types.md` | Enum table: `\| Name \| Value \| Description \|` |
| `RecordType` | `types.md` | Field table |
| `TypeAlias` | `types.md` | Inline `**Timestamp** — 64-bit unsigned integer` |
| `Function` | `functions/{name}.md` | One page per function: signature, decision table, doc, `<details>` fold |
| `Property` | `properties/{category}.md` | Grouped by category section (e.g. `key-lifecycle-safety.md`) |
| `CommentBlock` | *(context-dependent)* | Flows into whichever page is currently being built |

#### Output directory structure

```
output/
├── index.md                        # TOC + intro
├── types.md                        # All enums, records, scalar aliases
├── functions/
│   ├── provisionKey.md
│   ├── enrollDevice.md
│   ├── authenticate.md
│   ├── isValidRequestDate.md
│   ├── isValidSignature.md
│   ├── enforceAccess.md
│   └── getStatus.md
├── properties/
│   ├── key-lifecycle-safety.md     # P1–P5
│   ├── authentication-security.md  # P6–P10
│   ├── access-control.md           # P11–P14
│   ├── protocol-liveness.md        # P15–P18
│   └── error-handling.md           # P19–P22
└── canonicalization.md             # P23–P25 (standalone deep-dive)
```

#### Page templates

**`index.md`** — Generated TOC with three sections: Types, Functions, Properties. Each entry is a link. Property entries include the label and one-line summary.

**`functions/{name}.md`** — Title, signature (with type links), decision table, prose doc, and a "Related Properties" section listing every property that references this function (auto-detected from the link resolver).

**`properties/{category}.md`** — Category heading, then one H3 per property: label, plain-English statement (from doc comment), and raw Cryptol in a `<details>` fold. Function and type references are hyperlinked.

For **properties**, the emitter uses a template-based approach to generate plain English:
1. Extract the property label (`P1`) and descriptive name (`KeyMonotonicity`) from the identifier.
2. Use the leading doc comment (which in well-written specs already contains the English statement).
3. Wrap the raw Cryptol in a collapsible `<details>` block for those who want to see the formal version.
4. Cross-link any mentioned functions or types to their respective pages.

---

## Technology Choices

### Language: Rust

Correct choice for a CLI tool: single binary, fast, good ecosystem for parsing and CLI.

### Crates

| Crate | Purpose | Why |
|---|---|---|
| [`clap`](https://crates.io/crates/clap) | CLI argument parsing | Industry standard |
| [`logos`](https://crates.io/crates/logos) | Lexer generator | Zero-copy, derive macro, very fast. Perfect for keyword/token recognition without writing a manual lexer. |
| [`pulldown-cmark`](https://crates.io/crates/pulldown-cmark) | Markdown validation (optional) | Can round-trip our output to verify it's well-formed Markdown. |
| [`serde`](https://crates.io/crates/serde) + [`serde_json`](https://crates.io/crates/serde_json) | Serialise IR to JSON | For `--emit-json` flag (lets downstream tools consume the parsed structure). |
| [`miette`](https://crates.io/crates/miette) | Error diagnostics | Pretty error messages with source spans when parsing fails. |
| [`insta`](https://crates.io/crates/insta) | Snapshot testing | Snapshot the Markdown output of test specs to catch regressions. |
| [`regex`](https://crates.io/crates/regex) | Pattern extraction | For extracting property labels, splitting CamelCase names, etc. |
| [`convert_case`](https://crates.io/crates/convert_case) | CamelCase → Title Case | `KeyMonotonicity` → `Key Monotonicity` for headings. |

### What We Are NOT Building

- **A full Cryptol type-checker or evaluator.** We parse structure, not semantics.
- **A Cryptol-to-English theorem prover.** Properties are rendered using their doc comments + the formal body in a fold. We don't try to auto-translate arbitrary Cryptol expressions to English.
- **A general-purpose Cryptol IDE.** This is a one-shot batch renderer.

---

## CLI Interface

```
pretty-specs [OPTIONS] <INPUT>

Arguments:
  <INPUT>           Path to .cry file

Options:
  -o, --output <DIR>     Output directory (default: ./output)
      --single-file      Emit a single Markdown file instead of a directory
      --emit-json        Emit the parsed IR as JSON instead of Markdown
      --no-details       Don't include <details> folds with raw Cryptol
      --proof-status <FILE>  JSON proof manifest (from saw-spec-gen / SAW)
      --title <TITLE>    Override the document title
  -h, --help             Print help
  -V, --version          Print version
```

Default mode writes a directory of cross-linked pages. `--single-file` collapses everything into one document (still useful for pasting into a wiki page or sending as an attachment).

---

## Example Output (sketch)

Given `SDEP.cry`, the tool would produce something like:

---

#### `output/index.md`

> # Secure Device Enrollment Protocol (SDEP)
>
> This module specifies the pure decision logic of SDEP and encodes the 22 security properties (P1–P22) as formal declarations suitable for proof with SAW/Z3.
>
> ## Types
>
> All type definitions: [types.md](types.md)
>
> ## Functions
>
> | Function | Description |
> |----------|-------------|
> | [provisionKey](functions/provisionKey.md) | Provision a new key into the vault |
> | [enrollDevice](functions/enrollDevice.md) | Activate a device enrollment |
> | [authenticate](functions/authenticate.md) | Validate date, signature, and claims |
> | ... | |
>
> ## Security Properties
>
> | Category | Properties |
> |----------|------------|
> | [Key Lifecycle Safety](properties/key-lifecycle-safety.md) | P1–P5 |
> | [Authentication Security](properties/authentication-security.md) | P6–P10 |
> | [Access Control](properties/access-control.md) | P11–P14 |
> | [Protocol Liveness](properties/protocol-liveness.md) | P15–P18 |
> | [Error Handling](properties/error-handling.md) | P19–P22 |
> | [Canonicalization](canonicalization.md) | P23–P25 |

#### `output/functions/provisionKey.md`

> # `provisionKey`
>
> **Signature:** `(fleetEnabled, validRequest, vaultResult, keyIsActive) →` [`ProvisionResult`](../types.md#provisionresult)
>
> | # | Condition | Result |
> |---|-----------|--------|
> | 1 | Fleet is disabled | `PR_Disabled` |
> | 2 | Request is invalid | `PR_BadRequest` |
> | 3 | Vault result ≠ OK | `PR_InternalError` |
> | 4 | Key is already active | `PR_Unauthorized` |
> | 5 | *(otherwise)* | `PR_Succeeded` |
>
> ### Related Properties
> - [P2 — Active Prevents Provisioning](../properties/key-lifecycle-safety.md#p2--active-prevents-provisioning)
> - [P5 — Disabled Rejects All](../properties/key-lifecycle-safety.md#p5--disabled-rejects-all)
> - [P15 — Can Provision When Ready](../properties/protocol-liveness.md#p15--can-provision-when-ready)
> - [P20 — Bad Request Distinct](../properties/error-handling.md#p20--badrequest-distinct-from-unauthorized)
>
> <details><summary>Formal definition (Cryptol)</summary>
>
> ```cryptol
> provisionKey fleetEnabled validRequest vaultResult keyIsActive =
>   if ~ fleetEnabled        then PR_Disabled
>    | ~ validRequest        then PR_BadRequest
>    ...
> ```
> </details>

#### `output/properties/key-lifecycle-safety.md`

> # Key Lifecycle Safety
>
> ### P1 — Key Monotonicity  ✅ Proven (z3, 0.42s)
>
> Once a key is active, any subsequent activation attempt returns AlreadyActive. For any inputs, if the system reports the key is active then [`enrollDevice`](../functions/enrollDevice.md) can never return `Succeeded`.
>
> <details><summary>Formal property (Cryptol)</summary>
>
> ```cryptol
> property P1_KeyMonotonicity ...
> ```
> </details>

---

## Project Structure

```
pretty-specs/
├── Cargo.toml
├── src/
│   ├── main.rs          # CLI entry point (clap)
│   ├── lib.rs           # Public API: parse + render
│   ├── lexer.rs         # logos-based tokeniser
│   ├── parser.rs        # Structural parser → IR
│   ├── ir.rs            # Item enum + supporting types
│   ├── linker.rs        # Symbol table + cross-reference link resolver
│   ├── render_md.rs     # IR → multi-file Markdown
│   └── render_json.rs   # IR → JSON (serde)
├── tests/
│   ├── snapshots/       # insta snapshot files
│   └── integration.rs   # End-to-end: .cry → .md round-trip
├── examples/
│   └── SDEP.cry         # The example spec
├── DESIGN.md            # This file
└── README.md
```

---

## Work Breakdown

Items to enter in beads after approval:

| # | Item | Est. |
|---|------|------|
| 1 | **Project scaffold** — `cargo init`, `Cargo.toml` with deps, `clap` CLI skeleton, CI | S |
| 2 | **Lexer** — `logos` token definitions for Cryptol keywords, operators, literals, comments, newlines | M |
| 3 | **Parser: sections + comments** — Recognise `////` section headers, `//` comment blocks, `module` declaration | S |
| 4 | **Parser: type aliases + enum groups** — `type X = [N]`, constant definitions, `is<Enum>` predicates, record types | M |
| 5 | **Parser: functions** — Signature lines, `if \| else` chains → `Branch` list | M |
| 6 | **Parser: properties** — `property` keyword, label/name extraction, parameter list, body capture | M |
| 7 | **IR types** — `Item` enum, `EnumVariant`, `Branch`, serde derives | S |
| 8 | **Link resolver** — Build symbol table from IR, map type/function/property names to output file + anchor, replace references in doc text with relative Markdown links | M |
| 9 | **Markdown emitter: multi-file scaffold** — Create output directory, write `index.md` with TOC and links to all pages | S |
| 10 | **Markdown emitter: types.md** — Enum tables, record tables, scalar aliases, cross-linked | M |
| 11 | **Markdown emitter: functions/{name}.md** — One page per function: decision table, signature with type links, "Related Properties" back-links | M |
| 12 | **Markdown emitter: properties/{category}.md** — Grouped by section, plain-English cards with `<details>` folds, cross-linked | M |
| 13 | **Single-file fallback** — `--single-file` flag collapses all pages into one document | S |
| 14 | **JSON emitter** — `--emit-json` flag, serde serialisation of IR | S |
| 15 | **Snapshot tests** — `SDEP.cry` golden-file test with `insta` (multi-file + single-file modes) | S |
| 16 | **README + polish** — Usage docs, error messages, edge cases | S |

S = small (< half day), M = medium (half day – full day)

---

## Open Questions / Future Work

1. **Multi-module specs.** Some projects split specs across files with `import`. We can add `--include-dir` later; for now, single-file is sufficient.
2. **Custom templates.** A `--template` flag with Tera/Handlebars could let teams customise the output format. Not needed for v1.
3. **Cryptol server integration.** For specs that use complex type-level computation, we could optionally call `cryptol-remote-api` to evaluate types. Deferred — line-level parsing handles the common cases.
4. **PDF output.** Markdown → PDF via `pandoc` is trivial once the Markdown is clean. Not in scope for this tool.
5. **Static site generation.** The multi-file Markdown output is already compatible with `mdbook`, MkDocs, or Docusaurus. A `--mdbook` flag that also emits a `SUMMARY.md` / `book.toml` would be trivial to add later.
6. **`saw-spec-gen` proof manifest emission.** Extend `saw-spec-gen` to emit the `manifest.json` that `pretty-specs` consumes via `--proof-status`. The manifest schema is defined above in the IR section. Integration path: `saw-spec-gen` runs SAW/Z3 proofs → writes `manifest.json` → `pretty-specs` reads it and renders badges. Could also support a `--watch` mode that re-renders when the manifest changes.
7. **Proof-status diffing.** Compare two manifests (e.g. before/after a code change) and highlight regressions — properties that were proven but now fail. Useful in CI gating.
8. **SAW log parsing.** As an alternative to a structured manifest, parse SAW's stdout directly (`Proof succeeded`, `Counterexample found`) and build the manifest automatically. Lower priority since `saw-spec-gen` can do this more cleanly.
