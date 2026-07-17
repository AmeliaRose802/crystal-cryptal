// Step 2 of the native pipeline: prepare saw-spec-gen configuration, expand
// implementation inputs, invoke the verifier, and classify its result files.

mod config;
mod inputs;
mod result;

use std::path::Path;
use std::process::Command;

use config::prepare_config;
use inputs::expand_impl_files;
use result::{
    ParsedResult, ResultKind, read_result, remove_stale_output, restore_result, write_error_result,
};

use super::pipeline::PipelineArgs;

#[derive(Debug, Default)]
pub(super) struct VerificationSummary {
    verified: usize,
    proof_failures: usize,
    not_attempted: usize,
    pub pipeline_errors: usize,
}

impl VerificationSummary {
    pub fn has_pipeline_errors(&self) -> bool {
        self.pipeline_errors != 0
    }
}

struct VerifyContext<'a> {
    spec: &'a Path,
    impl_files: &'a [std::path::PathBuf],
    config: &'a Path,
    args: &'a PipelineArgs,
    saw_program: &'a str,
    saw_leading_args: &'a [String],
    is_rust: bool,
}

pub(super) fn run(
    spec: &Path,
    functions: &[String],
    args: &PipelineArgs,
) -> Result<VerificationSummary, String> {
    let impl_files = expand_impl_files(&args.impl_files, &args.impl_lang)?;
    let config = prepare_config(spec, args)?;
    let (saw_program, saw_leading_args) = split_program(&args.saw_spec_gen)?;
    let context = VerifyContext {
        spec,
        impl_files: &impl_files,
        config: &config,
        args,
        saw_program: &saw_program,
        saw_leading_args: &saw_leading_args,
        is_rust: args.impl_lang == "rust",
    };

    eprintln!(
        "\n[Step 2] Running saw-spec-gen for each function ({} implementation file{})",
        impl_files.len(),
        if impl_files.len() == 1 { "" } else { "s" }
    );

    let mut summary = VerificationSummary::default();
    for name in functions {
        verify_function(name, &context, &mut summary);
    }

    eprintln!(
        "  Summary: {} verified, {} proof failure{}, {} not attempted, {} pipeline error{}",
        summary.verified,
        summary.proof_failures,
        if summary.proof_failures == 1 { "" } else { "s" },
        summary.not_attempted,
        summary.pipeline_errors,
        if summary.pipeline_errors == 1 {
            ""
        } else {
            "s"
        }
    );
    Ok(summary)
}

fn verify_function(name: &str, context: &VerifyContext<'_>, summary: &mut VerificationSummary) {
    let out_dir = context.args.verify_output.join(format!("out_{name}"));
    let mut last_not_attempted = None;
    let mut attempt_errors = Vec::new();

    for impl_file in context.impl_files {
        eprint!("  Verifying {name} with {} ...", impl_file.display());
        if let Err(e) = remove_stale_output(&out_dir) {
            eprintln!(" ERROR: {e}");
            attempt_errors.push(format!("{}: {e}", impl_file.display()));
            continue;
        }

        let argv = build_argv(context, name, impl_file, &out_dir);
        let status = Command::new(context.saw_program).args(&argv).status();
        let status_text = match &status {
            Ok(status) if status.success() => "exit 0".to_string(),
            Ok(status) => format!("exit {}", status.code().unwrap_or(-1)),
            Err(e) => format!("spawn error: {e}"),
        };

        match read_result(&out_dir) {
            Ok(ParsedResult {
                kind: ResultKind::Verified,
                ..
            }) => {
                eprintln!(" verified");
                summary.verified += 1;
                return;
            }
            Ok(ParsedResult {
                kind: ResultKind::ProofFailed,
                ..
            }) => {
                eprintln!(" proof failed");
                summary.proof_failures += 1;
                return;
            }
            Ok(ParsedResult {
                kind: ResultKind::Unknown,
                ..
            }) => {
                eprintln!(" inconclusive");
                summary.proof_failures += 1;
                return;
            }
            Ok(ParsedResult {
                kind: ResultKind::NotAttempted,
                text,
            }) => {
                eprintln!(" no matching symbol");
                last_not_attempted = Some(text);
            }
            Err(result_error) => {
                eprintln!(" ERROR ({status_text})");
                attempt_errors.push(format!(
                    "{}: {status_text}; {result_error}",
                    impl_file.display()
                ));
            }
        }
    }

    if attempt_errors.is_empty()
        && let Some(result) = last_not_attempted
    {
        if context.args.strict_on_missing {
            summary.pipeline_errors += 1;
            write_error_result(
                &out_dir,
                name,
                &context.args.impl_lang,
                "no matching implementation symbol was found (--strict-on-missing)",
            );
        } else {
            summary.not_attempted += 1;
            restore_result(&out_dir, &result);
        }
        return;
    }

    summary.pipeline_errors += 1;
    let message = if attempt_errors.is_empty() {
        "saw-spec-gen produced no usable result.json".to_string()
    } else {
        format!(
            "saw-spec-gen produced no usable result.json:\n{}",
            attempt_errors.join("\n")
        )
    };
    write_error_result(&out_dir, name, &context.args.impl_lang, &message);
}

fn build_argv(
    context: &VerifyContext<'_>,
    name: &str,
    impl_file: &Path,
    out_dir: &Path,
) -> Vec<String> {
    let mut argv = context.saw_leading_args.to_vec();
    if context.is_rust {
        argv.push("verify-rust".into());
        argv.extend(["--rust-file".into(), path_str(impl_file)]);
    } else {
        argv.push("verify-cpp".into());
        argv.extend(["--cpp-file".into(), path_str(impl_file)]);
    }
    argv.extend(["--cryptol-spec".into(), path_str(context.spec)]);
    argv.extend(["--cryptol-fn".into(), name.to_string()]);
    argv.extend(["--function".into(), name.to_string()]);
    argv.extend(["--output".into(), path_str(out_dir)]);
    argv.push(format!("--config={}", path_str(context.config)));

    if !context.is_rust {
        for dir in &context.args.cxx_include_dirs {
            argv.push(format!("--include-dir={}", path_str(dir)));
        }
        if let Some(standard) = &context.args.cxx_standard {
            argv.push(format!("--cxx-standard={standard}"));
        }
        for flag in &context.args.clang_flags {
            // Joined form is required for dash-prefixed values. With separate
            // tokens clap interprets `-fexceptions` as a new top-level option.
            argv.push(format!("--clang-flag={flag}"));
        }
    }
    argv
}

/// Split a possibly wrapped program (`cargo run --`). A directly existing path
/// is kept intact so Windows executable paths containing spaces still work.
fn split_program(value: &str) -> Result<(String, Vec<String>), String> {
    if Path::new(value).is_file() {
        return Ok((value.to_string(), Vec::new()));
    }
    let mut parts = value.split_whitespace().map(String::from);
    let program = parts
        .next()
        .filter(|program| !program.is_empty())
        .ok_or_else(|| "--saw-spec-gen cannot be empty".to_string())?;
    Ok((program, parts.collect()))
}

fn path_str(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}
