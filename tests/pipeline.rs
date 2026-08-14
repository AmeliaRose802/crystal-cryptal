use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(0);
static MOCK_SAW_SPEC_GEN: OnceLock<PathBuf> = OnceLock::new();

struct TestProject {
    root: PathBuf,
    spec: PathBuf,
    first_impl_dir: PathBuf,
    second_impl_dir: PathBuf,
    include_dir: PathBuf,
    docs: PathBuf,
    verify_output: PathBuf,
    manifest: PathBuf,
    invocation_log: PathBuf,
}

impl TestProject {
    fn new(name: &str) -> Self {
        let id = NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "pretty-specs-pipeline-{name}-{}-{id}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        let first_impl_dir = root.join("impl-a");
        let second_impl_dir = root.join("impl-b/nested");
        let include_dir = root.join("include");
        fs::create_dir_all(&first_impl_dir).unwrap();
        fs::create_dir_all(&second_impl_dir).unwrap();
        fs::create_dir_all(&include_dir).unwrap();

        let spec = root.join("pipeline.cry");
        fs::write(
            &spec,
            "module Pipeline where\n\npipelineIdentity : [32] -> [32]\npipelineIdentity x = x\n",
        )
        .unwrap();
        fs::write(spec.with_extension("toml"), "alias_size = [\"Opaque=4\"]\n").unwrap();
        fs::write(
            first_impl_dir.join("a_missing.cpp"),
            "// no matching symbol\n",
        )
        .unwrap();
        fs::write(
            second_impl_dir.join("b_match.cpp"),
            "extern \"C\" unsigned pipelineIdentity(unsigned x) { return x; }\n",
        )
        .unwrap();

        Self {
            spec,
            first_impl_dir,
            second_impl_dir,
            include_dir,
            docs: root.join("docs"),
            verify_output: root.join("verify-out"),
            manifest: root.join("proof-manifest.json"),
            invocation_log: root.join("invocations.log"),
            root,
        }
    }

    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_pretty-specs"));
        command
            .current_dir(&self.root)
            .arg(&self.spec)
            .arg("--pipeline")
            .arg("--impl")
            .arg(&self.first_impl_dir)
            .arg("--impl")
            .arg(&self.second_impl_dir)
            .arg("--impl-lang")
            .arg("cpp")
            .arg("--saw-spec-gen")
            .arg(mock_saw_spec_gen())
            .arg("--cxx-include-dir")
            .arg(&self.include_dir)
            .arg("--cxx-standard")
            .arg("c++20")
            // Deliberately pass dash-prefixed values as separate input argv;
            // the pipeline must join them for downstream clap compatibility.
            .arg("--clang-flag")
            .arg("-fexceptions")
            .arg("--clang-flag")
            .arg("-fno-inline")
            .arg("--verify-output")
            .arg(&self.verify_output)
            .arg("--manifest-output")
            .arg(&self.manifest)
            .arg("--output")
            .arg(&self.docs)
            .arg("--skip-docs")
            .env("MOCK_SAW_SPEC_GEN_LOG", &self.invocation_log);
        command
    }
}

impl Drop for TestProject {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn pipeline_forwards_current_cli_shape_and_supports_repeated_directories() {
    let project = TestProject::new("forwarding");
    let output = project.command().output().unwrap();
    assert_success(&output);

    let log = fs::read_to_string(&project.invocation_log).unwrap();
    let invocations: Vec<_> = log.lines().collect();
    assert_eq!(
        invocations.len(),
        2,
        "the missing source should soft-skip before the matching source\n{log}"
    );
    assert!(log.contains("--clang-flag=-fexceptions"));
    assert!(log.contains("--clang-flag=-fno-inline"));
    assert!(log.contains("--cxx-standard=c++20"));
    assert!(log.contains("--include-dir="));
    assert!(!log.contains("--spec-only-on-missing"));
    assert!(
        !log.split(['\u{1f}', '\n'])
            .any(|argument| argument == "--clang-flag"),
        "clang flags were forwarded as separate argv tokens:\n{log}"
    );

    let generated_config =
        fs::read_to_string(project.verify_output.join("pretty-specs-saw-spec-gen.toml")).unwrap();
    let config: toml::Value = toml::from_str(&generated_config).unwrap();
    assert_eq!(config["spec_only_on_missing"].as_bool(), Some(true));
    assert_eq!(config["alias_size"][0].as_str(), Some("Opaque=4"));

    let manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&project.manifest).unwrap()).unwrap();
    let function = &manifest["functions"]["pipelineIdentity"];
    assert_eq!(function["overall"]["status"], "proven");
    assert_eq!(function["by_language"]["cpp"]["status"], "proven");
    assert_eq!(function["by_language"]["cpp"]["impl_file"], "b_match.cpp");
}

#[test]
fn pipeline_maps_cryptol_models_to_implementation_names() {
    let project = TestProject::new("model-mapping");
    let inventory = project.root.join("implementation_inventory.json");
    fs::write(
        &inventory,
        r#"{
  "functions": [
    {
      "name": "cppPipelineIdentity",
      "lang": "cpp",
      "file": "impl-b/nested/b_match.cpp",
      "models": "pipelineIdentity"
    }
  ]
}
"#,
    )
    .unwrap();

    let output = project
        .command()
        .arg("--implementation-inventory")
        .arg(&inventory)
        .output()
        .unwrap();
    assert_success(&output);

    let log = fs::read_to_string(&project.invocation_log).unwrap();
    for invocation in log.lines() {
        let args: Vec<_> = invocation.split('\u{1f}').collect();
        assert!(
            args.windows(2)
                .any(|pair| pair == ["--cryptol-fn", "pipelineIdentity"]),
            "model name was not forwarded to --cryptol-fn:\n{invocation}"
        );
        assert!(
            args.windows(2)
                .any(|pair| pair == ["--function", "cppPipelineIdentity"]),
            "mapped implementation name was not forwarded to --function:\n{invocation}"
        );
    }

    let manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&project.manifest).unwrap()).unwrap();
    assert_eq!(
        manifest["functions"]["pipelineIdentity"]["overall"]["status"],
        "proven"
    );
}

#[test]
fn adapter_keeps_the_strongest_result_across_translation_units() {
    let project = TestProject::new("best-wins-adapter");
    let results = project.root.join("multi-tu-results");
    write_result(
        &results.join("a-defining-tu/result.json"),
        "verifiedLeaf",
        "VERIFIED",
        "decision.cpp",
    );
    write_result(
        &results.join("z-caller-tu/result.json"),
        "verifiedLeaf",
        "UNKNOWN",
        "controller.cpp",
    );
    write_result(
        &results.join("b-defining-tu/result.json"),
        "disprovedLeaf",
        "DISPROVED",
        "decision.cpp",
    );
    write_result(
        &results.join("y-caller-tu/result.json"),
        "disprovedLeaf",
        "UNKNOWN",
        "auth.cpp",
    );
    let manifest_path = project.root.join("adapted-manifest.json");

    let output = Command::new(env!("CARGO_BIN_EXE_pretty-specs"))
        .arg("--adapt-saw-results")
        .arg(&results)
        .arg("--manifest-output")
        .arg(&manifest_path)
        .output()
        .unwrap();
    assert_success(&output);

    let manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(manifest_path).unwrap()).unwrap();
    assert_eq!(
        manifest["functions"]["verifiedLeaf"]["overall"]["status"],
        "proven"
    );
    assert_eq!(
        manifest["functions"]["verifiedLeaf"]["by_language"]["cpp"]["impl_file"],
        "decision.cpp"
    );
    assert_eq!(
        manifest["functions"]["disprovedLeaf"]["overall"]["status"],
        "failed"
    );
    assert_eq!(
        manifest["functions"]["disprovedLeaf"]["by_language"]["cpp"]["impl_file"],
        "decision.cpp"
    );
}

#[test]
fn pipeline_continues_after_an_inconclusive_caller_translation_unit() {
    let project = TestProject::new("inconclusive-caller");
    let output = project
        .command()
        .env("MOCK_SAW_SPEC_GEN_INCONCLUSIVE_MISSING", "1")
        .output()
        .unwrap();
    assert_success(&output);

    let log = fs::read_to_string(&project.invocation_log).unwrap();
    assert_eq!(
        log.lines().count(),
        2,
        "the defining TU must still be tried after an inconclusive caller TU\n{log}"
    );
    let manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&project.manifest).unwrap()).unwrap();
    assert_eq!(
        manifest["functions"]["pipelineIdentity"]["overall"]["status"],
        "proven"
    );
}

#[test]
fn pipeline_fails_before_adapting_unusable_verification_results() {
    let project = TestProject::new("hard-failure");
    let output = project
        .command()
        .env("MOCK_SAW_SPEC_GEN_FAIL", "1")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1), "{}", display_output(&output));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("verification was unusable"), "{stderr}");
    assert!(stderr.contains("Steps 3–4 were skipped"), "{stderr}");
    assert!(!project.manifest.exists());
    assert!(project.docs.join("index.md").exists());
    assert!(
        project
            .verify_output
            .join("out_pipelineIdentity/result.json")
            .exists()
    );
}

#[test]
fn best_effort_preserves_the_legacy_successful_exit() {
    let project = TestProject::new("best-effort");
    let output = project
        .command()
        .arg("--best-effort")
        .env("MOCK_SAW_SPEC_GEN_FAIL", "1")
        .output()
        .unwrap();
    assert_success(&output);

    let manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&project.manifest).unwrap()).unwrap();
    assert_eq!(
        manifest["functions"]["pipelineIdentity"]["overall"]["status"],
        "failed"
    );
}

#[test]
fn disproved_result_is_preserved_as_a_proof_outcome() {
    let project = TestProject::new("disproved");
    let output = project
        .command()
        .env("MOCK_SAW_SPEC_GEN_DISPROVE", "1")
        .output()
        .unwrap();
    assert_success(&output);

    let manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&project.manifest).unwrap()).unwrap();
    let status = &manifest["functions"]["pipelineIdentity"]["overall"];
    assert_eq!(status["status"], "failed");
    assert_eq!(status["reason"], "counterexample found");
}

fn write_result(path: &Path, cryptol_fn: &str, verdict: &str, impl_file: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let result = serde_json::json!({
        "schema_version": "1",
        "side": "cpp",
        "function": cryptol_fn,
        "cryptol_fn": cryptol_fn,
        "verdict": verdict,
        "impl_file": impl_file,
    });
    fs::write(path, serde_json::to_string_pretty(&result).unwrap()).unwrap();
}

fn mock_saw_spec_gen() -> &'static Path {
    MOCK_SAW_SPEC_GEN
        .get_or_init(|| {
            let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
            let output_dir = manifest_dir.join("target/pipeline-test-tools");
            fs::create_dir_all(&output_dir).unwrap();
            let executable =
                output_dir.join(format!("mock-saw-spec-gen{}", std::env::consts::EXE_SUFFIX));
            let rustc = std::env::var_os("RUSTC").unwrap_or_else(|| OsString::from("rustc"));
            let status = Command::new(rustc)
                .arg("--edition=2021")
                .arg(manifest_dir.join("tests/fixtures/mock_saw_spec_gen.rs"))
                .arg("-o")
                .arg(&executable)
                .status()
                .unwrap();
            assert!(status.success(), "failed to compile mock saw-spec-gen");
            executable
        })
        .as_path()
}

fn assert_success(output: &Output) {
    assert!(output.status.success(), "{}", display_output(output));
}

fn display_output(output: &Output) -> String {
    format!(
        "status: {}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}
