use super::*;

#[test]
fn attribute_inventory_requires_the_matching_syn_dependency_pin() {
    let value: toml::Value =
        toml::from_str(&read(&Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml")).unwrap())
            .unwrap();
    let expected = format!("={}", super::super::scopes::SYN_SCHEMA_VERSION);
    assert_eq!(
        value["dependencies"]["syn"]["version"].as_str(),
        Some(expected.as_str()),
        "Review the attribute inventory when updating syn."
    );
}

#[test]
fn test_scope_covers_statements_expressions_fields_variants_and_parameters() {
    let vocabulary = vocabulary();
    let (name, _) = identity(&vocabulary);
    let production = format!("fn production() {{ display({name:?}); }}");
    for test in [
        format!("fn f() {{ #[cfg(test)] assert_eq!(name, {name:?}); }}"),
        format!("fn f() {{ let _ = #[cfg(test)] probe!({name:?}); }}"),
        format!("fn f() {{ let _ = #[cfg(test)] {name:?}; }}"),
        format!("fn f() {{ let _ = [#[cfg(test)] {name:?}]; }}"),
        format!("fn f() {{ let _ = #[cfg(test)] (name == {name:?}); }}"),
        format!("fn f() {{ let _ = #[cfg(test)] || display({name:?}); }}"),
        format!("fn f() {{ let _ = #[cfg(test)] async {{ display({name:?}); }}; }}"),
        format!("fn f() {{ let _ = #[cfg(test)] unsafe {{ display({name:?}); }}; }}"),
        format!("fn f() {{ #[cfg(test)] while ready {{ display({name:?}); }} }}"),
        format!("fn f() {{ let _ = S {{ #[cfg(test)] value: {name:?} }}; }}"),
        format!("struct S {{ #[cfg(test)] value: [u8; key({name:?})] }}"),
        format!("enum S {{ #[cfg(test)] Value = key({name:?}) }}"),
        format!("fn f(#[cfg(test)] value: [u8; key({name:?})]) {{}}"),
        format!("struct S<#[cfg(test)] const N: usize = {{ key({name:?}) }}>;"),
        format!("struct S<#[cfg(test)] T: Bound<{{ key({name:?}) }}>>;"),
        format!("unsafe extern \"C\" {{ #[cfg(test)] fn f(value: [u8; key({name:?})]); }}"),
        format!("impl S {{ #[cfg(test)] fn f() {{ display({name:?}); }} }}"),
        format!("trait S {{ #[cfg(test)] fn f() {{ display({name:?}); }} }}"),
        format!("fn f() {{ match name {{ #[cfg(test)] {name:?} => (), _ => () }}; }}"),
        format!(
            "macro_rules! f {{ ($name:expr) => {{ #[cfg(test)] assert_eq!($name, {name:?}); }} }}"
        ),
    ] {
        assert_eq!(
            literals(&format!("{test} {production}"), &vocabulary),
            [name],
            "{test}"
        );
    }
}

#[test]
fn statement_macro_scope_honors_test_predicates_without_leaking_to_siblings() {
    let vocabulary = vocabulary();
    let (name, _) = identity(&vocabulary);
    for attribute in [
        "#[cfg(test)]",
        "#[cfg(all(test, unix))]",
        "#[cfg(any(test, feature = \"test-support\"))]",
        "#[cfg(feature = \"test-support\")]",
    ] {
        let source = format!("fn f() {{ {attribute} probe!({name:?}); display({name:?}); }}");
        assert_eq!(literals(&source, &vocabulary), [name], "{attribute}");
    }
    for attribute in ["#[cfg(any(test, unix))]", "#[cfg(not(test))]"] {
        let source = format!("fn f() {{ {attribute} probe!({name:?}); display({name:?}); }}");
        assert_eq!(literals(&source, &vocabulary), [name, name], "{attribute}");
    }
}

#[test]
fn attributes_on_patterns_and_struct_pattern_fields_are_scoped() {
    let vocabulary = vocabulary();
    let (name, _) = identity(&vocabulary);
    // Attribute literals ensure the pattern node itself receives the test scope.
    for source in [
        format!("fn f() {{ let S {{ #[cfg(test)] #[marker = {name:?}] value }} = input; }}"),
        format!("fn f(#[cfg(test)] #[marker = {name:?}] value: &str) {{}}"),
    ] {
        assert!(literals(&source, &vocabulary).is_empty(), "{source}");
    }
}
