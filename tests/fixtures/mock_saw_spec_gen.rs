use std::env;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

fn option_value(args: &[String], name: &str) -> Option<String> {
    let joined = format!("{name}=");
    for (index, arg) in args.iter().enumerate() {
        if let Some(value) = arg.strip_prefix(&joined) {
            return Some(value.to_string());
        }
        if arg == name {
            return args.get(index + 1).cloned();
        }
    }
    None
}

fn fail(message: &str) -> ! {
    eprintln!("mock saw-spec-gen: {message}");
    std::process::exit(2);
}

fn escape_json(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    if let Ok(log_path) = env::var("MOCK_SAW_SPEC_GEN_LOG") {
        let mut log = OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_path)
            .unwrap();
        writeln!(log, "{}", args.join("\u{1f}")).unwrap();
    }

    if args.first().map(String::as_str) != Some("verify-cpp") {
        fail("expected verify-cpp");
    }
    if args.iter().any(|arg| arg == "--clang-flag") {
        fail("clang flags must use --clang-flag=<value>");
    }
    if args.iter().any(|arg| arg == "--spec-only-on-missing") {
        fail("removed --spec-only-on-missing option was forwarded");
    }
    for expected in [
        "--clang-flag=-fexceptions",
        "--clang-flag=-fno-inline",
        "--cxx-standard=c++20",
    ] {
        if !args.iter().any(|arg| arg == expected) {
            fail(&format!("missing {expected}"));
        }
    }

    let config = option_value(&args, "--config").unwrap_or_else(|| fail("missing --config"));
    let config_text = fs::read_to_string(&config).unwrap_or_else(|_| fail("unreadable --config"));
    if !config_text.contains("spec_only_on_missing = true") {
        fail("generated config did not enable spec-only-on-missing");
    }
    if !config_text.contains("Opaque=4") {
        fail("generated config did not preserve the discovered base config");
    }

    if env::var_os("MOCK_SAW_SPEC_GEN_FAIL").is_some() {
        fail("forced invocation failure");
    }

    let cpp_file = PathBuf::from(
        option_value(&args, "--cpp-file").unwrap_or_else(|| fail("missing --cpp-file")),
    );
    let output = PathBuf::from(
        option_value(&args, "--output").unwrap_or_else(|| fail("missing --output")),
    );
    let function = option_value(&args, "--function").unwrap_or_else(|| fail("missing --function"));
    let cryptol_fn =
        option_value(&args, "--cryptol-fn").unwrap_or_else(|| fail("missing --cryptol-fn"));
    fs::create_dir_all(&output).unwrap();
    fs::write(
        output.join("generated-verify.saw"),
        format!(
            "// Step 1: Load bitcode\nm <- llvm_load_module \"fixture.bc\";\n\n// Step 2: Bitcode-derived extern overrides\n// override: _Mtx_lock  [declare-only]\nlet lock_spec = do {{\n    p0 <- llvm_fresh_pointer (llvm_int 8);\n    llvm_execute_func [p0];\n    llvm_return (llvm_term {{{{ 0 : [32] }}}});\n}};\nov_lock <- llvm_unsafe_assume_spec m \"_Mtx_lock\" lock_spec;\n\n// Step 3: Import Cryptol spec\n\n// Step 4: Uninterpreted primitive contracts\n// uninterpreted: fixtureEq (symbol: fixture_eq)\nlet fixtureEq_spec = do {{\n    a0 <- llvm_fresh_var \"a0\" (llvm_int 32);\n    llvm_execute_func [llvm_term a0];\n    llvm_return (llvm_term {{{{ fixtureEq a0 }}}});\n}};\nov_fixture <- llvm_unsafe_assume_spec m \"fixture_eq\" fixtureEq_spec;\n\n// Step 5: Equivalence spec — {cryptol_fn}\n"
        ),
    )
    .unwrap();

    if env::var_os("MOCK_SAW_SPEC_GEN_VERIFY_ERROR").is_some()
        && !cpp_file.to_string_lossy().contains("missing")
    {
        let result = format!(
            "{{\n  \"schema_version\": \"1\",\n  \"side\": \"cpp\",\n  \"function\": \"{}\",\n  \"cryptol_fn\": \"{}\",\n  \"status\": \"error\",\n  \"message\": \"Loading file verify.saw\",\n  \"impl_file\": \"{}\"\n}}\n",
            escape_json(&function),
            escape_json(&cryptol_fn),
            escape_json(&cpp_file.to_string_lossy()),
        );
        fs::write(output.join("result.json"), result).unwrap();
        eprintln!("Cryptol: [error] at verify.saw:8:1");
        eprintln!("Could not find definition for Unknown type alias Ident \"reference\"");
        std::process::exit(1);
    }

    let impl_name = cpp_file
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("unknown.cpp");
    let is_missing = impl_name.contains("missing");
    let is_disproved = !is_missing && env::var_os("MOCK_SAW_SPEC_GEN_DISPROVE").is_some();
    let (status, verdict, reason) = if is_missing {
        let status = if env::var_os("MOCK_SAW_SPEC_GEN_INCONCLUSIVE_MISSING").is_some() {
            ""
        } else {
            "\"status\": \"not_attempted\","
        };
        (
            status,
            "UNKNOWN",
            "\"message\": \"no matching implementation\",",
        )
    } else if is_disproved {
        ("", "DISPROVED", "\"message\": \"counterexample found\",")
    } else {
        ("", "VERIFIED", "")
    };
    let json = format!(
        "{{\n  \"schema_version\": \"1\",\n  \"side\": \"cpp\",\n  \"function\": \"{}\",\n  \"cryptol_fn\": \"{}\",\n  {}\n  \"verdict\": \"{}\",\n  {}\n  \"counterexample\": [],\n  \"solver\": \"z3\",\n  \"impl_file\": \"{}\"\n}}\n",
        escape_json(&function),
        escape_json(&cryptol_fn),
        status,
        verdict,
        reason,
        escape_json(impl_name),
    );
    fs::write(output.join(Path::new("result.json")), json).unwrap();
    if is_disproved {
        std::process::exit(1);
    }
}
