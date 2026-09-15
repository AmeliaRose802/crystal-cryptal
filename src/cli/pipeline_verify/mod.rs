// Step 2 of the native pipeline: prepare saw-spec-gen configuration, expand
// implementation inputs, invoke the verifier, and classify its result files.

mod config;
mod inputs;
mod result;

use std::collections::HashMap;
use std::path::Path;
use std::process::Command;

use config::prepare_config;
use inputs::expand_impl_files;
use result::{
    ParsedResult, ResultKind, enrich_result, read_result, remove_stale_output, restore_result,
    subprocess_diagnostic, write_error_result,
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
    model_mappings: &'a HashMap<String, String>,
    is_rust: bool,
}

pub(super) fn run(
    spec: &Path,
    functions: &[String],
    model_mappings: &HashMap<String, String>,
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
        model_mappings,
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
    let implementation_name = context
        .model_mappings
        .get(name)
        .map(String::as_str)
        .unwrap_or(name);
    let mut best_result = None;
    let mut attempt_errors = Vec::new();

    for impl_file in context.impl_files {
        eprint!("  Verifying {name} with {} ...", impl_file.display());
        if let Err(e) = remove_stale_output(&out_dir) {
            eprintln!(" ERROR: {e}");
            attempt_errors.push(format!("{}: {e}", impl_file.display()));
            continue;
        }

        let argv = build_argv(context, name, implementation_name, impl_file, &out_dir);
        let output = Command::new(context.saw_program).args(&argv).output();
        let status_text = match &output {
            Ok(output) if output.status.success() => "exit 0".to_string(),
            Ok(output) => format!("exit {}", output.status.code().unwrap_or(-1)),
            Err(e) => format!("spawn error: {e}"),
        };
        if let Ok(output) = &output {
            enrich_result(&out_dir, output, context.saw_program, &argv);
        }

        match read_result(&out_dir) {
            Ok(result) => match result.kind {
                ResultKind::Verified => {
                    eprintln!(" verified");
                    summary.verified += 1;
                    return;
                }
                ResultKind::ProofFailed => {
                    eprintln!(" proof failed");
                    retain_preferred_result(&mut best_result, result);
                }
                ResultKind::Unknown => {
                    eprintln!(" inconclusive");
                    retain_preferred_result(&mut best_result, result);
                }
                ResultKind::NotAttempted => {
                    eprintln!(" no matching symbol");
                    retain_preferred_result(&mut best_result, result);
                }
            },
            Err(result_error) => {
                eprintln!(" ERROR ({status_text})");
                let diagnostic = output
                    .as_ref()
                    .map(subprocess_diagnostic)
                    .unwrap_or_default();
                attempt_errors.push(format!(
                    "{}: {status_text}; {result_error}{}{}",
                    impl_file.display(),
                    if diagnostic.is_empty() { "" } else { "\n" },
                    diagnostic,
                ));
            }
        }
    }

    if let Some(result) = best_result {
        match result.kind {
            ResultKind::ProofFailed | ResultKind::Unknown => {
                summary.proof_failures += 1;
                restore_result(&out_dir, &result.text);
                return;
            }
            ResultKind::NotAttempted if attempt_errors.is_empty() => {
                if context.args.strict_on_missing {
                    summary.pipeline_errors += 1;
                    write_error_result(
                        &out_dir,
                        implementation_name,
                        name,
                        &context.args.impl_lang,
                        "no matching implementation symbol was found (--strict-on-missing)",
                    );
                } else {
                    summary.not_attempted += 1;
                    restore_result(&out_dir, &result.text);
                }
                return;
            }
            ResultKind::NotAttempted => {}
            ResultKind::Verified => unreachable!("verified results return immediately"),
        }
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
    write_error_result(
        &out_dir,
        implementation_name,
        name,
        &context.args.impl_lang,
        &message,
    );
}

fn retain_preferred_result(best: &mut Option<ParsedResult>, candidate: ParsedResult) {
    let replace = best
        .as_ref()
        .is_none_or(|current| result_precedence(candidate.kind) > result_precedence(current.kind));
    if replace {
        *best = Some(candidate);
    }
}

fn result_precedence(kind: ResultKind) -> u8 {
    match kind {
        ResultKind::Verified => 4,
        ResultKind::ProofFailed => 3,
        ResultKind::Unknown => 2,
        ResultKind::NotAttempted => 1,
    }
}

fn build_argv(
    context: &VerifyContext<'_>,
    cryptol_fn: &str,
    implementation_name: &str,
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
    argv.extend(["--cryptol-fn".into(), cryptol_fn.to_string()]);
    argv.extend(["--function".into(), implementation_name.to_string()]);
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
