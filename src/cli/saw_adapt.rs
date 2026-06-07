// SAW log / result-json adapters: read upstream verifier output and emit a
// unified `proof_manifest.json` consumed by the renderer.

use std::path::{Path, PathBuf};

use pretty_specs::ir::ProofStatus;
use pretty_specs::saw_log::parse_saw_log;

pub(crate) fn run_saw_log_adapter(log_path: &Path, output: &Path) {
    let text = std::fs::read_to_string(log_path).unwrap_or_else(|e| {
        eprintln!("error: cannot read {}: {e}", log_path.display());
        std::process::exit(2);
    });

    let records = parse_saw_log(&text);

    let mut properties = serde_json::Map::new();
    for record in &records {
        let entry = proof_status_to_json(&record.status);
        properties.insert(record.name.clone(), entry);
    }

    // Preserve any existing `functions` entries so subsequent runs of
    // --adapt-saw-results don't clobber Cryptol property results and
    // vice versa.  Both adapters merge into the same manifest by name.
    let existing_functions = load_existing_section(output, "functions");

    let manifest = serde_json::json!({
        "properties": properties,
        "functions": existing_functions,
    });

    if let Some(parent) = output.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent).unwrap_or_else(|e| {
            eprintln!("error: cannot create {}: {e}", parent.display());
            std::process::exit(2);
        });
    }
    let serialized = serde_json::to_string_pretty(&manifest).unwrap_or_else(|e| {
        eprintln!("error: failed to serialize manifest: {e}");
        std::process::exit(2);
    });
    std::fs::write(output, format!("{serialized}\n")).unwrap_or_else(|e| {
        eprintln!("error: cannot write {}: {e}", output.display());
        std::process::exit(2);
    });

    eprintln!(
        "wrote {} ({} propert{})",
        output.display(),
        records.len(),
        if records.len() == 1 { "y" } else { "ies" }
    );
}

/// Read an existing manifest at `path` and return the named top-level
/// section (`properties` or `functions`) as a JSON object.  Used so the
/// two adapters can merge into the same file: each preserves the other
/// adapter's section instead of overwriting it.  Missing file or
/// unparseable manifest yields an empty object (which is also the
/// fresh-manifest case, so behavior is unchanged for first runs).
fn load_existing_section(path: &Path, key: &str) -> serde_json::Value {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(_) => return serde_json::json!({}),
    };
    let v: serde_json::Value = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(_) => return serde_json::json!({}),
    };
    v.get(key).cloned().unwrap_or_else(|| serde_json::json!({}))
}

pub(crate) fn proof_status_to_json(status: &ProofStatus) -> serde_json::Value {
    match status {
        ProofStatus::Proven {
            solver,
            time_secs,
            overrides,
            iterations,
            verify_command,
            verify_script,
        } => {
            let mut m = serde_json::Map::new();
            m.insert("status".into(), serde_json::json!("proven"));
            m.insert("solver".into(), serde_json::json!(solver));
            if let Some(t) = time_secs {
                m.insert("time_secs".into(), serde_json::json!(t));
            }
            if !overrides.is_empty() {
                m.insert("overrides".into(), serde_json::json!(overrides));
            }
            if let Some(n) = iterations {
                m.insert("iterations".into(), serde_json::json!(n));
            }
            if let Some(cmd) = verify_command {
                m.insert("verify_command".into(), serde_json::json!(cmd));
            }
            if let Some(scr) = verify_script {
                m.insert("verify_script".into(), serde_json::json!(scr));
            }
            serde_json::Value::Object(m)
        }
        ProofStatus::Failed {
            reason,
            counterexample,
            log_excerpt,
            verify_command,
            verify_script,
        } => {
            let mut m = serde_json::Map::new();
            m.insert("status".into(), serde_json::json!("failed"));
            m.insert("reason".into(), serde_json::json!(reason));
            if let Some(cx) = counterexample {
                m.insert("counterexample".into(), serde_json::json!(cx));
            }
            if let Some(log) = log_excerpt {
                m.insert("log_excerpt".into(), serde_json::json!(log));
            }
            if let Some(cmd) = verify_command {
                m.insert("verify_command".into(), serde_json::json!(cmd));
            }
            if let Some(scr) = verify_script {
                m.insert("verify_script".into(), serde_json::json!(scr));
            }
            serde_json::Value::Object(m)
        }
        ProofStatus::Assumed => serde_json::json!({ "status": "assumed" }),
        ProofStatus::NotAttempted => serde_json::json!({ "status": "not_attempted" }),
    }
}

/// Scan a directory tree for saw-spec-gen `result.json` files and emit a
/// unified `proof_manifest.json`.
pub(crate) fn run_adapt_saw_results(dir: &Path, output: &Path) {
    let mut result_files = Vec::new();
    collect_result_json_recursive(dir, &mut result_files).unwrap_or_else(|e| {
        eprintln!("error: {e}");
        std::process::exit(2);
    });

    if result_files.is_empty() {
        eprintln!(
            "warning: no result.json files found under {}",
            dir.display()
        );
    }

    let mut functions_map = serde_json::Map::new();

    for path in &result_files {
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("warning: cannot read {}: {e}", path.display());
                continue;
            }
        };
        let value: serde_json::Value = match serde_json::from_str(&text) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("warning: cannot parse {}: {e}", path.display());
                continue;
            }
        };

        let fn_name = extract_fn_name(&value, path);
        let proof_status = result_value_to_status(&value);

        let mut status_json = proof_status_to_json(&proof_status);
        if let Some(obj) = status_json.as_object_mut()
            && let Some(f) = value.get("impl_file").and_then(|v| v.as_str())
        {
            obj.insert("impl_file".into(), serde_json::json!(f));
        }

        let entry = functions_map
            .entry(fn_name)
            .or_insert_with(|| serde_json::json!({ "implementations": {} }));
        if let Some(entry_obj) = entry.as_object_mut() {
            if let Some(lang) = value.get("impl_lang").and_then(|v| v.as_str()) {
                let implementations = entry_obj
                    .entry("implementations")
                    .or_insert_with(|| serde_json::json!({}));
                if let Some(impl_obj) = implementations.as_object_mut() {
                    impl_obj.insert(lang.to_string(), status_json);
                }
            } else {
                entry_obj.insert("overall".into(), status_json);
            }
        }
    }

    let fn_count = functions_map.len();
    // Preserve any existing `properties` section so a prior
    // --adapt-saw-log run (Cryptol property verdicts) is not clobbered
    // when this adapter writes function verdicts to the same manifest.
    let existing_properties = load_existing_section(output, "properties");
    let manifest = serde_json::json!({
        "properties": existing_properties,
        "functions": functions_map,
    });

    write_manifest_file(&manifest, output);
    eprintln!(
        "wrote {} ({} function{})",
        output.display(),
        fn_count,
        if fn_count == 1 { "" } else { "s" }
    );
}

fn extract_fn_name(value: &serde_json::Value, path: &Path) -> String {
    value
        .get("cryptol_fn")
        .and_then(|v| v.as_str())
        .or_else(|| value.get("function").and_then(|v| v.as_str()))
        .unwrap_or_else(|| {
            // Fall back to the parent directory name (e.g. out_provisionKey → provisionKey)
            path.parent()
                .and_then(|p| p.file_name())
                .and_then(|n| n.to_str())
                .unwrap_or("unknown")
        })
        .to_string()
}

/// Convert a parsed `result.json` value into a `ProofStatus`. Handles both the
/// legacy lowercase `status` field and the newer uppercase `verdict` field
/// emitted by saw-spec-gen's Write-VerifyResult (schema_version 1+).
fn result_value_to_status(value: &serde_json::Value) -> ProofStatus {
    let raw_status = value
        .get("status")
        .or_else(|| value.get("verdict"))
        .and_then(|v| v.as_str())
        .unwrap_or("not_run");
    let solver = value
        .get("solver")
        .and_then(|v| v.as_str())
        .unwrap_or("saw");
    let time_secs = value.get("time_secs").and_then(|v| v.as_f64());
    let message = value
        .get("message")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let overrides: Vec<String> = value
        .get("overrides")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();
    let iterations: Option<u64> = value
        .get("iterations")
        .or_else(|| value.get("loop_bound"))
        .or_else(|| value.get("max_len"))
        .and_then(|v| v.as_u64());
    let counterexample = extract_counterexample(value);
    let log_excerpt: Option<String> = value
        .get("log_excerpt")
        .or_else(|| value.get("log"))
        .or_else(|| value.get("stderr"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let verify_command: Option<String> = value
        .get("verify_command")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let verify_script: Option<String> = value
        .get("verify_script")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    match raw_status {
        "verified" | "VERIFIED" | "Q.E.D." | "valid" | "EQUIVALENT" => ProofStatus::Proven {
            solver: solver.to_string(),
            time_secs,
            overrides,
            iterations,
            verify_command,
            verify_script,
        },
        "counterexample" | "DISPROVED" | "NOT EQUIVALENT" | "invalid" | "sat" => {
            ProofStatus::Failed {
                reason: message.unwrap_or_else(|| "counterexample found".into()),
                counterexample,
                log_excerpt,
                verify_command,
                verify_script,
            }
        }
        "timeout" => ProofStatus::Failed {
            reason: message.unwrap_or_else(|| "timeout".into()),
            counterexample,
            log_excerpt,
            verify_command,
            verify_script,
        },
        "error" | "UNKNOWN" => ProofStatus::Failed {
            reason: message.unwrap_or_else(|| "error during verification".into()),
            counterexample,
            log_excerpt,
            verify_command,
            verify_script,
        },
        _ => ProofStatus::NotAttempted,
    }
}

/// Extract a counterexample as a human-readable string. Prefers `counterexample_text`
/// (schema v2+ free-form override), then accepts either a string or a structured
/// `[{name, value, bits}, …]` array that we format inline.
fn extract_counterexample(value: &serde_json::Value) -> Option<String> {
    value
        .get("counterexample_text")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .or_else(|| {
            value
                .get("counterexample")
                .or_else(|| value.get("witness"))
                .and_then(|v| match v {
                    serde_json::Value::String(s) if !s.is_empty() => Some(s.clone()),
                    serde_json::Value::Array(arr) if !arr.is_empty() => {
                        let lines: Vec<String> = arr
                            .iter()
                            .filter_map(|entry| {
                                let obj = entry.as_object()?;
                                let name = obj.get("name").and_then(|n| n.as_str())?;
                                let val = obj
                                    .get("value")
                                    .map(|x| match x {
                                        serde_json::Value::String(s) => s.clone(),
                                        other => other.to_string(),
                                    })
                                    .unwrap_or_else(|| "<unknown>".into());
                                let bits = obj.get("bits").and_then(|b| b.as_u64());
                                Some(match bits {
                                    Some(b) => format!("{name} = {val}  ({b}-bit)"),
                                    None => format!("{name} = {val}"),
                                })
                            })
                            .collect();
                        if lines.is_empty() {
                            None
                        } else {
                            Some(lines.join("\n"))
                        }
                    }
                    _ => None,
                })
        })
}

fn collect_result_json_recursive(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    for entry in std::fs::read_dir(dir)
        .map_err(|e| format!("failed to read directory {}: {e}", dir.display()))?
    {
        let entry = entry.map_err(|e| format!("failed to read entry: {e}"))?;
        let path = entry.path();
        if path.is_dir() {
            collect_result_json_recursive(&path, out)?;
        } else if path.file_name().and_then(|n| n.to_str()) == Some("result.json") {
            out.push(path);
        }
    }
    Ok(())
}

pub(crate) fn write_manifest_file(manifest: &serde_json::Value, output: &Path) {
    if let Some(parent) = output.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent).unwrap_or_else(|e| {
            eprintln!("error: cannot create {}: {e}", parent.display());
            std::process::exit(2);
        });
    }
    let serialized = serde_json::to_string_pretty(manifest).unwrap_or_else(|e| {
        eprintln!("error: failed to serialize manifest: {e}");
        std::process::exit(2);
    });
    std::fs::write(output, format!("{serialized}\n")).unwrap_or_else(|e| {
        eprintln!("error: cannot write {}: {e}", output.display());
        std::process::exit(2);
    });
}
