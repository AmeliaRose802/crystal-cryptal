// JSON renderer: emits JSON from linked IR.

use crate::ir::Item;
use std::io::Write;
use std::path::Path;

/// Serialize items as pretty-printed JSON.
/// If `output_path` is `None`, write to stdout; otherwise write to the given file.
pub fn render_json(items: &[Item], output_path: Option<&Path>) -> std::io::Result<()> {
    let json = serde_json::to_string_pretty(items)
        .map_err(std::io::Error::other)?;
    match output_path {
        Some(path) => {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(path, json)?;
            eprintln!("wrote {}", path.display());
        }
        None => {
            let stdout = std::io::stdout();
            let mut out = stdout.lock();
            out.write_all(json.as_bytes())?;
            out.write_all(b"\n")?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::ir::Item;

    #[test]
    fn round_trip() {
        let items = vec![
            Item::TypeAlias {
                name: "Key".into(),
                width: "[256]".into(),
                doc: vec!["A 256-bit key.".into()],
            },
            Item::Function {
                name: "id".into(),
                signature: "{a} a -> a".into(),
                branches: vec![],
                body: "\\x -> x".into(),
                doc: vec![],
                proof_status: None,
                is_private: false,
            },
        ];

        let json = serde_json::to_string_pretty(&items).unwrap();
        let recovered: Vec<Item> = serde_json::from_str(&json).unwrap();
        assert_eq!(json, serde_json::to_string_pretty(&recovered).unwrap());
    }
}
