use std::fs;

use super::util::{property_anchor, sanitize_slug};
use super::{ModuleSpec, SymbolTable};
use crate::ir::Item;
use crate::parser::parse;

fn load_items() -> Vec<Item> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("SDEP.cry");
    let src = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("SDEP.cry not found at {}: {e}", path.display()));
    parse(&src)
}

#[test]
fn symbol_table_has_types() {
    let items = load_items();
    let st = SymbolTable::build(&items);

    // Type aliases
    assert_eq!(
        st.symbols["FleetMode"],
        ("types.md".into(), "fleetmode".into())
    );
    assert_eq!(
        st.symbols["ProvisionResult"],
        ("types.md".into(), "provisionresult".into())
    );

    // Enum variants map to parent type anchor
    assert_eq!(
        st.symbols["FM_Disabled"],
        ("types.md".into(), "fleetmode".into())
    );
    assert_eq!(
        st.symbols["PR_Succeeded"],
        ("types.md".into(), "provisionresult".into())
    );

    // Record type
    assert_eq!(
        st.symbols["EnrollmentStatus"],
        ("types.md".into(), "enrollmentstatus".into())
    );
}

#[test]
fn symbol_table_has_functions() {
    let items = load_items();
    let st = SymbolTable::build(&items);

    assert_eq!(
        st.symbols["provisionKey"],
        ("functions/provisionKey.md".into(), "".into())
    );
    assert_eq!(
        st.symbols["enrollDevice"],
        ("functions/enrollDevice.md".into(), "".into())
    );
    assert_eq!(
        st.symbols["enforceAccess"],
        ("functions/enforceAccess.md".into(), "".into())
    );
}

#[test]
fn symbol_table_has_properties() {
    let items = load_items();
    let st = SymbolTable::build(&items);

    assert_eq!(
        st.symbols["P1"],
        (
            "properties/key-lifecycle-safety.md".into(),
            "p1--key-monotonicity".into()
        )
    );
    assert_eq!(st.property_categories["P1"], "key-lifecycle-safety");

    // P6 is in Authentication Security
    assert_eq!(st.property_categories["P6"], "authentication-security");

    // P11 is in Access Control
    assert_eq!(st.property_categories["P11"], "access-control");

    // P15 is in Protocol Liveness
    assert_eq!(st.property_categories["P15"], "protocol-liveness");

    // P19 is in Error Handling
    assert_eq!(st.property_categories["P19"], "error-handling");
}

#[test]
fn resolve_links_in_property_doc() {
    let items = load_items();
    let st = SymbolTable::build(&items);

    // P1 doc mentions enrollDevice — should become a link
    let text = "enrollDevice can never return Succeeded";
    let resolved = st.resolve_links(text, "properties/key-lifecycle-safety.md");
    assert!(
        resolved.contains("[enrollDevice](../functions/enrollDevice.md)"),
        "Expected link to enrollDevice, got: {resolved}"
    );
}

#[test]
fn back_links_for_provision_key() {
    let items = load_items();
    let st = SymbolTable::build(&items);

    let related = st
        .related_properties
        .get("provisionKey")
        .expect("provisionKey should have related properties");
    let labels: Vec<&str> = related.iter().map(|(l, _, _)| l.as_str()).collect();

    assert!(labels.contains(&"P2"), "P2 should reference provisionKey");
    assert!(labels.contains(&"P5"), "P5 should reference provisionKey");
    assert!(labels.contains(&"P15"), "P15 should reference provisionKey");
    assert!(labels.contains(&"P20"), "P20 should reference provisionKey");
}

#[test]
fn relative_path_computation() {
    assert_eq!(
        SymbolTable::relative_path("types.md", "functions/foo.md"),
        "functions/foo.md"
    );
    assert_eq!(
        SymbolTable::relative_path("functions/foo.md", "types.md"),
        "../types.md"
    );
    assert_eq!(
        SymbolTable::relative_path("functions/foo.md", "properties/bar.md"),
        "../properties/bar.md"
    );
    assert_eq!(
        SymbolTable::relative_path("properties/bar.md", "functions/foo.md"),
        "../functions/foo.md"
    );
    assert_eq!(
        SymbolTable::relative_path("properties/bar.md", "types.md"),
        "../types.md"
    );
}

#[test]
fn anchor_generation() {
    assert_eq!(
        property_anchor("P1", "KeyMonotonicity"),
        "p1--key-monotonicity"
    );
    assert_eq!(
        property_anchor("P5", "DisabledRejectsAll"),
        "p5--disabled-rejects-all"
    );
    assert_eq!(
        property_anchor("P23", "CanonicalizationInjective"),
        "p23--canonicalization-injective"
    );
}

#[test]
fn build_for_modules_resolves_cross_module_links() {
    let a_items = parse("module A where\n\nfoo : [8]\nfoo = 0\n");
    let b_items = parse("module B where\n\nbar : [8]\nbar = A::foo\n");
    let specs = vec![
        ModuleSpec {
            name: "A",
            output_prefix: "A",
            items: &a_items,
        },
        ModuleSpec {
            name: "B",
            output_prefix: "B",
            items: &b_items,
        },
    ];

    let st = SymbolTable::build_for_modules(&specs);
    let resolved = st.resolve_links("A::foo", "B/functions/bar.md");
    assert!(
        resolved.contains("[A::foo](../../A/functions/foo.md)"),
        "resolved = {resolved}"
    );
}

#[test]
fn sanitize_slug_strips_windows_invalid_chars() {
    // `*` in "void* pointer-value" must be removed.
    assert_eq!(sanitize_slug("void-pointer-value"), "void-pointer-value");
    assert_eq!(sanitize_slug("void*-pointer-value"), "void-pointer-value");

    // All Windows-invalid chars are replaced and consecutive dashes collapsed.
    assert_eq!(sanitize_slug("a*b?c<d>e:f|g\"h"), "a-b-c-d-e-f-g-h");

    // Back- and forward-slashes are also invalid.
    assert_eq!(sanitize_slug("a\\b/c"), "a-b-c");

    // Leading/trailing dashes are stripped.
    assert_eq!(sanitize_slug("*foo*"), "foo");
    assert_eq!(sanitize_slug("-foo-"), "foo");

    // Purely-invalid input falls back to "unnamed".
    assert_eq!(sanitize_slug("***"), "unnamed");
    assert_eq!(sanitize_slug(""), "unnamed");

    // Windows reserved base-names get a trailing `_`.
    assert_eq!(sanitize_slug("con"), "con_");
    assert_eq!(sanitize_slug("NUL"), "NUL_");
    assert_eq!(sanitize_slug("com1"), "com1_");

    // Normal slugs are unchanged.
    assert_eq!(
        sanitize_slug("key-lifecycle-safety"),
        "key-lifecycle-safety"
    );
}
