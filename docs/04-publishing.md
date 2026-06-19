# One-pager: Publishing crystal-cryptal as a versioned package

**Status:** proposal · **Audience:** crystal-cryptal (pretty-specs) maintainers · **Owner:** (you)
**Companion:** consumed by `demo_protocol`'s [regen-and-publish.yml](../../.github/workflows/regen-and-publish.yml)

## Problem

`crystal-cryptal` ships the `pretty-specs` renderer and `pipeline.ps1`, but it
publishes **nothing** — no GitHub Release, no package, no crate. Every consumer
has to clone the repo and `cargo build --release` from `master` HEAD.

That has three concrete costs, all of which land on downstream automation:

1. **Slow.** `demo_protocol`'s docs-publish workflow rebuilds the whole crate
   (plus its dependency graph) on every run before it can render a single page.
   A cold `cargo build --release` dominates the job.

2. **Unreproducible.** "Build from `master`" pins to whatever HEAD happened to
   be at job time. Two runs a week apart can render with different renderer
   behavior and nobody recorded which. There is no version to cite in the
   published docs ("rendered by pretty-specs vX.Y.Z").

3. **Hard to consume.** Anyone who wants the tool — CI, a teammate, another
   repo — needs the Rust toolchain and the build incantation. There is no
   `install pretty-specs and go`.

The fix is to **publish a versioned artifact** on a tag, and let consumers
download it instead of building HEAD.

## Proposal: tag-driven release with prebuilt binaries

Adopt SemVer tags (`vX.Y.Z`) as the release trigger. On tag push, a CI matrix
builds `pretty-specs` for each target OS and uploads the binaries — **plus
`pipeline.ps1`** — as GitHub Release assets with checksums.

```
  git tag v0.1.0 && git push --tags
        │
        ▼
  release.yml (matrix: linux-x64, windows-x64[, macos])
        │   cargo build --release
        │   bundle: pretty-specs[.exe] + pipeline.ps1 + regen.ps1 + LICENSE
        │   sha256 checksums
        ▼
  GitHub Release v0.1.0  ◀── consumers download by version, not branch
```

Three rules carry the whole design:

- **The release is the unit, the tag is the source of truth.** A CI gate
  asserts the pushed tag matches `version` in `Cargo.toml`, so a release can
  never disagree with the crate metadata.

- **Ship `pipeline.ps1` *in* the release, not just the binary.** The pipeline
  script is what downstream actually invokes; a binary-only release would still
  force a clone. Bundle the binary + `pipeline.ps1` (+ `regen.ps1`) in one
  archive per platform so a consumer needs exactly one download.

- **Checksums, always.** Emit `SHA256SUMS` next to the assets so consumers can
  verify what they fetched.

## What to publish — pick the surface(s)

| Channel | Consumer command | Good for | Cost / caveat |
|---------|------------------|----------|---------------|
| **GitHub Release binaries** *(recommended primary)* | download asset for the OS, `chmod +x` | CI and any repo, zero toolchain | maintain a small matrix build |
| **crates.io** | `cargo install pretty-specs` | Rust users who already have cargo | name **`pretty-specs` may be taken**; still requires a compile on install |
| **GitHub Packages / container** | `docker run …` | hermetic, pinned toolchain | heavier; only worth it if the SAW deps are bundled too |

Recommendation: **GitHub Release binaries as the primary channel** (it's what
the `demo_protocol` workflow can consume directly), with **crates.io as an
optional convenience** once the crate-name question is settled.

### Name collision note

The repo is `crystal-cryptal`; the binary is `pretty-specs`. Before any
crates.io publish, check whether `pretty-specs` is free on crates.io. If not,
publish under a namespaced/renamed crate (e.g. `crystal-cryptal`) while keeping
the binary name via `[[bin]] name = "pretty-specs"`. GitHub Releases don't have
this constraint, which is another reason they're the primary channel.

## Versioning & changelog

- **SemVer.** `MAJOR` for output/CLI breaking changes (a re-render that moves
  pages or changes badge semantics is breaking for sites that link them);
  `MINOR` for additive flags/badges; `PATCH` for fixes.
- **Single source of truth:** `Cargo.toml` `version`. CI rejects a tag that
  doesn't match it.
- **`CHANGELOG.md`** with a section per release; the release notes are
  generated from it.
- **Stamp the version into the output.** `pretty-specs --version` already
  exists; have the renderer emit `rendered by pretty-specs vX.Y.Z` in the site
  footer / `index.md`, so every published site is traceable to a release.

## Sketch: `release.yml` (in crystal-cryptal)

```yaml
name: Release
on:
  push:
    tags: ['v*']
permissions:
  contents: write          # create the release + upload assets
jobs:
  guard:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: Tag must match Cargo.toml version
        shell: bash
        run: |
          cv=$(grep -m1 '^version' Cargo.toml | sed -E 's/.*"(.*)".*/\1/')
          [ "v$cv" = "${GITHUB_REF_NAME}" ] || { echo "tag != Cargo.toml v$cv"; exit 1; }
  build:
    needs: guard
    strategy:
      matrix:
        include:
          - { os: ubuntu-latest,  target: x86_64-unknown-linux-gnu, ext: '' }
          - { os: windows-latest, target: x86_64-pc-windows-msvc,   ext: '.exe' }
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v4
      - run: cargo build --release
      - name: Bundle
        shell: bash
        run: |
          d="pretty-specs-${GITHUB_REF_NAME}-${{ matrix.target }}"
          mkdir "$d"
          cp "target/release/pretty-specs${{ matrix.ext }}" pipeline.ps1 regen.ps1 LICENSE* "$d" 2>/dev/null || true
          tar -czf "$d.tar.gz" "$d"
          sha256sum "$d.tar.gz" > "$d.sha256"
      - uses: softprops/action-gh-release@v2
        with:
          files: |
            pretty-specs-*.tar.gz
            pretty-specs-*.sha256
```

## Consumer impact — `demo_protocol`

The docs-publish workflow's "checkout crystal-cryptal + `cargo build --release`"
step collapses to a pinned download:

```yaml
# regen job, replacing the source build
- name: Fetch pretty-specs ${{ env.PRETTY_SPECS_VERSION }}
  shell: pwsh
  run: |
    $v = $env:PRETTY_SPECS_VERSION   # e.g. v0.1.0  ← pinned, reproducible
    $a = "pretty-specs-$v-x86_64-unknown-linux-gnu.tar.gz"
    gh release download $v --repo AmeliaRose802/crystal-cryptal --pattern $a
    tar -xzf $a
    # verify SHA256SUMS, then use ./pretty-specs and ./pipeline.ps1
```

Result for the consumer: **faster** (no compile), **reproducible** (a pinned
version recorded in the workflow and stamped in the site), and **toolchain-free**
(no Rust needed in the docs job). The from-source path stays as a fallback for
testing unreleased `master`.

## Acceptance criteria

1. Pushing a `vX.Y.Z` tag produces a GitHub Release with Linux + Windows
   archives, each containing the `pretty-specs` binary **and** `pipeline.ps1`,
   plus `SHA256SUMS`.
2. CI **fails** the release if the tag doesn't match `Cargo.toml` `version`.
3. `pretty-specs --version` and the rendered site footer both report the
   released version.
4. `demo_protocol`'s docs workflow can switch its build step to a pinned
   release download and render docs with **no Rust toolchain** in that job.
5. (Optional) `cargo install pretty-specs` works, or a documented decision
   records the renamed crate if the name is taken.
