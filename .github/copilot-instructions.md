## Issue Tracking

This project uses **bd (beads)** for issue tracking — NOT GitHub Issues.

Run `bd prime` for full workflow context, or `bd ready` to find unblocked work.

**Quick reference:**

- `bd ready` — Find unblocked work
- `bd create "Title" --type task|bug|feature --priority 2 -d "description"` — Create issue
- `bd list` — List all open issues
- `bd show <id>` — Show issue details
- `bd update <id> --claim` — Claim work
- `bd close <id>` — Complete work
- `bd close <id1> <id2> ...` — Close multiple issues at once

**Labels:** Use `-l "parser"` or `-l "renderer"` to tag component.

**Do NOT** create GitHub Issues for this repo. All tracking goes through `bd`.


## File size limits

CI enforces a hard **500-line cap** on every `src/*.rs` file (see `.github/workflows/ci.yml` — the `file-length` job fails the build for anything over 500). Do **not** suppress, raise, or work around this limit.

When a file approaches ~400 lines, split it before adding more:

- Convert `src/foo.rs` to a directory: `src/foo/mod.rs` + topical sibling files (`util.rs`, `helpers.rs`, `parse.rs`, etc.).
- Keep the public API re-exported from `mod.rs` (`pub use child::PubItem;`) so external callers don't break.
- Use `pub(super)` for items shared between sibling submodules within the same directory; use `pub(crate)` only when an item needs to cross module boundaries.
- Keep `#[cfg(test)] mod tests { ... }` blocks inside the submodule whose code they exercise — this avoids the test file itself growing past 500 lines.
- After splitting, delete the old monolithic file (`Remove-Item src\foo.rs`) and run `cargo build` then `cargo test` to confirm.

Examples of this layout already in the tree: `src/render_md/`, `src/parser/`, `src/lexer/`, `src/linker/`, `src/cli/`, `src/ir/`.
