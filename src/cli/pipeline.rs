// `--pipeline` orchestrator: the native, cross-platform replacement for the
// old `pipeline.ps1` script.
//
// Runs the full pretty-specs / saw-spec-gen flow for a single Cryptol spec:
//
//   0. Render docs (without proof badges).
//   1. Emit a function inventory (`--emit-function-list`).
//   2. Invoke `saw-spec-gen verify-cpp` / `verify-rust` for each function.
//   3. Collect the per-function `result.json` files (`--adapt-saw-results`).
//   4. Re-render the docs with proof-status badges.
//
// Steps 0, 1, 3, and 4 are pretty-specs operations and are run by
// re-invoking this same binary (`std::env::current_exe()`), so the pipeline
// always uses the matching renderer. Step 2 shells out to the `saw-spec-gen`
// binary's native subcommands — no PowerShell shim required.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;

/// Pipeline-mode flags, flattened into the top-level `Cli`.
#[derive(clap::Args, Debug)]
pub(crate) struct PipelineArgs {
    /// Run the full end-to-end pipeline: render docs, run saw-spec-gen per
    /// function, adapt the results, then re-render with proof badges.
    ///
    /// The first positional input is treated as the `.cry` spec. Reuses the
    /// shared `-o/--output`, `--manifest-output`, `--docfx`, `--logo`,
    /// `--favicon`, and `--extra-docs` flags for the render steps.
    #[arg(long)]
    pub pipeline: bool,

    /// Implementation file (C++ or Rust) passed to saw-spec-gen. When
    /// omitted, the verification steps (1–2) are skipped (docs only).
    #[arg(long = "impl", value_name = "FILE")]
    pub impl_file: Option<PathBuf>,

    /// Implementation language for `--impl`: `cpp` or `rust`.
    #[arg(long = "impl-lang", value_name = "cpp|rust", default_value = "cpp")]
    pub impl_lang: String,

    /// Path (or name on PATH) of the saw-spec-gen binary. A wrapper invocation
    /// such as `"cargo run --"` is also accepted (split on whitespace).
    #[arg(
        long = "saw-spec-gen",
        value_name = "PATH",
        default_value = "saw-spec-gen"
    )]
    pub saw_spec_gen: String,

    /// Directory where saw-spec-gen writes `out_*/result.json` files.
    #[arg(
        long = "verify-output",
        value_name = "DIR",
        default_value = "verify_out"
    )]
    pub verify_output: PathBuf,

    /// Extra `-I` include directory passed to clang (C++ only). Repeatable.
    #[arg(long = "cxx-include-dir", value_name = "DIR")]
    pub cxx_include_dirs: Vec<PathBuf>,

    /// C++ standard passed to clang as `-std=<value>` (C++ only).
    #[arg(long = "cxx-standard", value_name = "STD")]
    pub cxx_standard: Option<String>,

    /// Extra raw clang flag forwarded verbatim (C++ only). Repeatable.
    #[arg(long = "clang-flag", value_name = "FLAG")]
    pub clang_flags: Vec<String>,

    /// Skip saw-spec-gen verification (Steps 1–2).
    #[arg(long = "skip-verify")]
    pub skip_verify: bool,

    /// Skip `--adapt-saw-results` (Step 3); reuse an existing manifest.
    #[arg(long = "skip-adapt")]
    pub skip_adapt: bool,

    /// Skip the final doc render (Step 4).
    #[arg(long = "skip-docs")]
    pub skip_docs: bool,

    /// Treat a Cryptol helper with no matching implementation symbol as a
    /// hard error instead of soft-skipping it. (Soft-skip is the default,
    /// since most spec modules contain private helpers with no impl analog.)
    #[arg(long = "strict-on-missing")]
    pub strict_on_missing: bool,
}

/// Shared doc-rendering options pulled from the top-level `Cli`.
pub(crate) struct DocOpts<'a> {
    pub output: &'a Path,
    pub manifest_output: &'a Path,
    pub docfx: bool,
    pub logo: Option<&'a Path>,
    pub favicon: Option<&'a Path>,
    pub extra_docs: &'a [String],
}

#[derive(Debug, Deserialize)]
struct FnEntry {
    name: String,
}

/// Entry point for `--pipeline`. Never returns; exits with the final status.
#[allow(clippy::too_many_arguments)]
pub(crate) fn run_pipeline(
    inputs: &[PathBuf],
    output: &Path,
    manifest_output: &Path,
    docfx: bool,
    logo: Option<&Path>,
    favicon: Option<&Path>,
    extra_docs: &[String],
    args: &PipelineArgs,
) -> ! {
    let doc = DocOpts {
        output,
        manifest_output,
        docfx,
        logo,
        favicon,
        extra_docs,
    };
    if inputs.is_empty() {
        eprintln!("error: --pipeline requires a .cry spec as the first input");
        std::process::exit(2);
    }
    let spec = inputs[0].clone();
    let self_exe = std::env::current_exe().unwrap_or_else(|e| {
        eprintln!("error: cannot resolve pretty-specs binary: {e}");
        std::process::exit(2);
    });

    // ── Step 0: initial doc render (no proof badges) ─────────────────────────
    eprintln!("\n[Step 0] Initial doc render");
    let mut step0 = vec![osstr(&spec), os("-o"), osstr(doc.output)];
    push_doc_flags(&mut step0, &doc);
    run_or_die(&self_exe, &step0, "pretty-specs render");

    // ── Steps 1–2: verification ──────────────────────────────────────────────
    if !args.skip_verify {
        match &args.impl_file {
            Some(impl_file) => run_verification(&self_exe, &spec, impl_file, &doc, args),
            None => eprintln!(
                "\n[Step 1+2] Skipped (no --impl provided — set --impl to enable SAW verification)"
            ),
        }
    }

    // ── Step 3: adapt saw-spec-gen results ───────────────────────────────────
    if !args.skip_adapt {
        if args.verify_output.exists() {
            eprintln!(
                "\n[Step 3] Adapting saw-spec-gen results -> {}",
                doc.manifest_output.display()
            );
            run_or_die(
                &self_exe,
                &[
                    os("--adapt-saw-results"),
                    osstr(&args.verify_output),
                    os("--manifest-output"),
                    osstr(doc.manifest_output),
                ],
                "pretty-specs --adapt-saw-results",
            );
        } else {
            eprintln!(
                "\n[Step 3] Skipped ({} not found)",
                args.verify_output.display()
            );
        }
    } else {
        eprintln!("\n[Step 3] Skipped (--skip-adapt)");
    }

    // ── Step 4: final render with proof badges ───────────────────────────────
    if !args.skip_docs {
        if doc.manifest_output.exists() {
            eprintln!(
                "\n[Step 4] Rendering docs with proof badges -> {}",
                doc.output.display()
            );
            let mut step4 = vec![
                osstr(&spec),
                os("--proof-status"),
                osstr(doc.manifest_output),
                os("-o"),
                osstr(doc.output),
            ];
            push_doc_flags(&mut step4, &doc);
            run_or_die(&self_exe, &step4, "pretty-specs render (with badges)");
        } else {
            eprintln!(
                "\n[Step 4] No proof manifest at {} — docs already rendered in Step 0",
                doc.manifest_output.display()
            );
        }
    } else {
        eprintln!("\n[Step 4] Skipped (--skip-docs)");
    }

    eprintln!("\nDone. Docs -> {}", doc.output.display());
    std::process::exit(0);
}

/// Steps 1–2: emit the function list, then run saw-spec-gen per function.
fn run_verification(
    self_exe: &Path,
    spec: &Path,
    impl_file: &Path,
    doc: &DocOpts<'_>,
    args: &PipelineArgs,
) {
    let function_list = args.verify_output.join("function_list.json");
    eprintln!(
        "\n[Step 1] Emitting function list -> {}",
        function_list.display()
    );
    if let Err(e) = std::fs::create_dir_all(&args.verify_output) {
        eprintln!("error: cannot create {}: {e}", args.verify_output.display());
        std::process::exit(2);
    }
    run_or_die(
        self_exe,
        &[
            osstr(spec),
            os("--emit-function-list"),
            os("-o"),
            osstr(&function_list),
        ],
        "pretty-specs --emit-function-list",
    );

    eprintln!("\n[Step 2] Running saw-spec-gen for each function");
    let functions = load_function_names(&function_list);
    let (saw_prog, saw_lead) = split_program(&args.saw_spec_gen);
    let is_rust = args.impl_lang.eq_ignore_ascii_case("rust");

    let total = functions.len();
    let mut passed = 0usize;
    let mut failed = 0usize;

    for name in &functions {
        let out_dir = args.verify_output.join(format!("out_{name}"));
        eprint!("  Verifying {name} ...");

        let mut argv: Vec<String> = saw_lead.clone();
        if is_rust {
            argv.push("verify-rust".into());
            argv.extend(["--rust-file".into(), path_str(impl_file)]);
        } else {
            argv.push("verify-cpp".into());
            argv.extend(["--cpp-file".into(), path_str(impl_file)]);
        }
        argv.extend(["--cryptol-spec".into(), path_str(spec)]);
        argv.extend(["--cryptol-fn".into(), name.clone()]);
        argv.extend(["--function".into(), name.clone()]);
        argv.extend(["--output".into(), path_str(&out_dir)]);
        if !is_rust {
            for d in &args.cxx_include_dirs {
                argv.extend(["--include-dir".into(), path_str(d)]);
            }
            if let Some(std) = &args.cxx_standard {
                argv.extend(["--cxx-standard".into(), std.clone()]);
            }
            for f in &args.clang_flags {
                argv.extend(["--clang-flag".into(), f.clone()]);
            }
        }
        if !args.strict_on_missing {
            argv.push("--spec-only-on-missing".into());
        }

        let status = Command::new(&saw_prog).args(&argv).status();
        match status {
            Ok(s) if s.success() => {
                eprintln!(" ok");
                passed += 1;
            }
            Ok(s) => {
                let code = s.code().unwrap_or(-1);
                eprintln!(" FAILED (exit {code})");
                failed += 1;
                write_error_result(
                    &out_dir,
                    name,
                    &format!("saw-spec-gen verify exited with code {code}"),
                );
            }
            Err(e) => {
                eprintln!(" ERROR: {e}");
                failed += 1;
                write_error_result(
                    &out_dir,
                    name,
                    &format!("failed to spawn saw-spec-gen: {e}"),
                );
            }
        }
    }

    eprintln!("  {passed}/{total} passed, {failed} failed");
    let _ = doc; // doc opts unused here; verification writes to verify_output
}

/// Append the shared doc-render flags (docfx / logo / favicon / extra-docs).
fn push_doc_flags(argv: &mut Vec<std::ffi::OsString>, doc: &DocOpts<'_>) {
    if doc.docfx {
        argv.push(os("--docfx"));
    }
    if let Some(logo) = doc.logo {
        argv.push(os("--logo"));
        argv.push(osstr(logo));
    }
    if let Some(favicon) = doc.favicon {
        argv.push(os("--favicon"));
        argv.push(osstr(favicon));
    }
    for d in doc.extra_docs {
        argv.push(os("--extra-docs"));
        argv.push(std::ffi::OsString::from(d));
    }
}

fn load_function_names(path: &Path) -> Vec<String> {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| {
        eprintln!("error: cannot read {}: {e}", path.display());
        std::process::exit(2);
    });
    let entries: Vec<FnEntry> = serde_json::from_str(&text).unwrap_or_else(|e| {
        eprintln!("error: cannot parse {}: {e}", path.display());
        std::process::exit(2);
    });
    entries.into_iter().map(|e| e.name).collect()
}

/// Write a fallback `result.json` so `--adapt-saw-results` records the failure.
fn write_error_result(out_dir: &Path, name: &str, message: &str) {
    if let Err(e) = std::fs::create_dir_all(out_dir) {
        eprintln!("warning: cannot create {}: {e}", out_dir.display());
        return;
    }
    let json = serde_json::json!({
        "cryptol_fn": name,
        "status": "error",
        "message": message,
    });
    let dest = out_dir.join("result.json");
    if let Err(e) = std::fs::write(
        &dest,
        serde_json::to_string_pretty(&json).unwrap_or_default(),
    ) {
        eprintln!("warning: cannot write {}: {e}", dest.display());
    }
}

/// Run a pretty-specs (self) subcommand; abort the pipeline on failure.
fn run_or_die(self_exe: &Path, argv: &[std::ffi::OsString], label: &str) {
    eprintln!("  > {} {}", self_exe.display(), render_argv(argv));
    let status = Command::new(self_exe)
        .args(argv)
        .status()
        .unwrap_or_else(|e| {
            eprintln!("error: failed to run {label}: {e}");
            std::process::exit(2);
        });
    if !status.success() {
        eprintln!("error: {label} exited with {}", status.code().unwrap_or(-1));
        std::process::exit(status.code().unwrap_or(1));
    }
}

/// Split a possibly-wrapped program string (e.g. `"cargo run --"`) into the
/// program and its leading arguments.
fn split_program(s: &str) -> (String, Vec<String>) {
    let mut parts = s.split_whitespace().map(String::from);
    let prog = parts.next().unwrap_or_default();
    (prog, parts.collect())
}

fn os(s: &str) -> std::ffi::OsString {
    std::ffi::OsString::from(s)
}

fn osstr(p: &Path) -> std::ffi::OsString {
    p.as_os_str().to_os_string()
}

fn path_str(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

fn render_argv(argv: &[std::ffi::OsString]) -> String {
    argv.iter()
        .map(|a| a.to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join(" ")
}
