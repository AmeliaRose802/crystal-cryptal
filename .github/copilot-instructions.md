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
