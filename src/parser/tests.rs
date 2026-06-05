use super::parse;
use crate::ir::Item;

#[test]
fn parses_import_with_alias_and_hiding() {
    let items = parse("module A where\n\nimport Crypto::Hash as H hiding (internalA, internalB)\n");

    let import = items.into_iter().find_map(|i| match i {
        Item::Import {
            module_path,
            qualifier,
            hiding,
        } => Some((module_path, qualifier, hiding)),
        _ => None,
    });

    let (module_path, qualifier, hiding) = import.expect("import item");
    assert_eq!(module_path, "Crypto::Hash");
    assert_eq!(qualifier.as_deref(), Some("H"));
    assert_eq!(
        hiding,
        vec!["internalA".to_string(), "internalB".to_string()]
    );
}
