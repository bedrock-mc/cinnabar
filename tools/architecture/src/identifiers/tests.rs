use super::*;
use crate::policy::{IdentifierException, LineBaseline};

mod patterns;
mod scopes;
mod targets;

/// Reads the real policy so fixtures use the same pinned identifiers and owner exclusions.
fn policy() -> Policy {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    toml::from_str(&read(&root.join("tools/architecture/policy.toml")).unwrap()).unwrap()
}

/// Loads the committed vocabulary, with no installed game assets or invented registry names.
fn vocabulary() -> Vocabulary {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut diagnostics = Vec::new();
    let vocabulary =
        Vocabulary::load(&root, &policy().identifier_literals, &mut diagnostics).unwrap();
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    vocabulary
}

/// Returns an actual namespaced block/item name and its bare spelling from the pinned inputs.
fn identity(vocabulary: &Vocabulary) -> (&str, &str) {
    let name = vocabulary
        .full
        .iter()
        .find(|name| name.starts_with("minecraft:"))
        .unwrap();
    (name, name.split_once(':').unwrap().1)
}

/// Scans a syn fixture and returns only its production literal occurrences.
fn literals(source: &str, vocabulary: &Vocabulary) -> Vec<String> {
    let file = syn::parse_file(source).unwrap();
    let features = BTreeSet::from(["test-support".into()]);
    let mut scan = RustScan::new(Path::new("src/lib.rs"), false, &features, vocabulary);
    scan.visit_file(&file);
    scan.violations
        .into_values()
        .filter(|entry| !entry.test)
        .map(|entry| entry.literal)
        .collect()
}

/// Runs the rule on small source files, copying its real pinned inputs into the temporary root.
fn gate(sources: &[(&str, &str)], configure: impl FnOnce(&mut Policy)) -> Vec<String> {
    let temp = tempfile::tempdir().unwrap();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut policy = policy();
    policy.identifier_literals.baselines.clear();
    policy.identifier_literal_exceptions.clear();
    policy.crate_rules = vec![
        toml::from_str("name = 'fixture'\npath = '.'\ntest_support_features = ['test-support']")
            .unwrap(),
    ];
    fs::write(
        temp.path().join("Cargo.toml"),
        "[package]\nname = 'fixture'\nversion = '0.1.0'\nedition = '2024'\n",
    )
    .unwrap();
    fs::create_dir_all(temp.path().join("src")).unwrap();
    fs::write(temp.path().join("src/lib.rs"), "").unwrap();
    for path in policy
        .identifier_literals
        .vocabularies
        .iter()
        .map(|source| &source.path)
        .chain(&policy.identifier_literals.block_registries)
    {
        let target = temp.path().join(path);
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::copy(root.join(path), target).unwrap();
    }
    let files = sources
        .iter()
        .map(|(path, source)| {
            let path = temp.path().join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, source).unwrap();
            path
        })
        .collect::<Vec<_>>();
    configure(&mut policy);
    let mut diagnostics = Vec::new();
    check_identifiers(temp.path(), &policy, &files, &mut diagnostics).unwrap();
    diagnostics
}

#[test]
fn rejects_each_bare_identifier_selector_and_table_shape() {
    let vocabulary = vocabulary();
    let (_, bare) = identity(&vocabulary);
    for expression in [
        format!("match name {{ {bare:?} => true, _ => false }}"),
        format!("matches!(name, {bare:?})"),
        format!("matches!(name, | {bare:?} if ready, )"),
        format!("name == {bare:?}"),
        format!("{bare:?} != name"),
        format!("[{bare:?}]"),
        format!("[{bare:?}; 2]"),
        format!("const {{ {bare:?} }}"),
        format!("name.contains({bare:?})"),
        format!("std::primitive::str::contains(name, {bare:?})"),
        format!("vec![{bare:?}]"),
        format!("wrapper!(name == {bare:?})"),
    ] {
        assert_eq!(
            literals(
                &format!("fn check() {{ let _ = {expression}; }}"),
                &vocabulary
            ),
            [bare],
            "{expression}"
        );
    }
    for source in [
        format!("const NAME: &str = {bare:?};"),
        format!("static NAME: &str = {bare:?};"),
        format!("impl A {{ const NAME: &str = {bare:?}; }}"),
        format!("trait A {{ const NAME: &str = {bare:?}; }}"),
    ] {
        assert_eq!(literals(&source, &vocabulary), [bare], "{source}");
    }
}

#[test]
fn repeated_vector_and_array_elements_keep_the_identity_table_context() {
    let vocabulary = vocabulary();
    let (_, bare) = identity(&vocabulary);
    for expression in [
        format!("vec![{bare:?}; 4]"),
        format!("[{bare:?}; 4]"),
        format!("vec![&({bare:?}); length]"),
        format!("[{bare:?}; length]"),
    ] {
        let source = format!("fn f() {{ let _ = {expression}; }}");
        assert_eq!(literals(&source, &vocabulary), [bare], "{expression}");
        assert!(
            gate(&[("src/lib.rs", &source)], |_| {})
                .iter()
                .any(|line| line.contains("count 1 exceeds budget 0")),
            "{expression}"
        );
    }
    let template =
        format!("macro_rules! table {{ ($length:expr) => {{ vec![{bare:?}; $length] }} }}");
    assert_eq!(literals(&template, &vocabulary), [bare]);
}

#[test]
fn qualified_identifiers_are_rejected_outside_selector_contexts() {
    let vocabulary = vocabulary();
    let (name, _) = identity(&vocabulary);
    for source in [
        format!("fn f() {{ display({name:?}); }}"),
        format!("fn f() {{ log!({name:?}); }}"),
        format!("#[serde(rename = {name:?})] struct A;"),
        format!("#[serde(rename = {name:?})] const NAME: &str = \"\";"),
        format!("fn f() {{ value.get::<{{ key({name:?}) }}>(); }}"),
        format!("fn f() {{ display(r##\"{name}\"##); }}"),
        format!("macro_rules! f {{ () => {{ display({name:?}); }} }}"),
    ] {
        assert_eq!(literals(&source, &vocabulary), [name], "{source}");
    }
}

#[test]
fn ordinary_bare_strings_scrutinees_and_guards_are_allowed() {
    let vocabulary = vocabulary();
    let (_, bare) = identity(&vocabulary);
    for source in [
        format!("fn f() {{ display({bare:?}); }}"),
        format!("fn f() {{ let name = {bare:?}; }}"),
        format!("fn f() {{ let _ = matches!({bare:?}, _ if display({bare:?})); }}"),
        format!("fn f() {{ match name {{ _ if display({bare:?}) => display({bare:?}) }}; }}"),
        "fn f() { name.strip_prefix(\"minecraft:\"); }".to_owned(),
    ] {
        assert!(literals(&source, &vocabulary).is_empty(), "{source}");
    }
}

#[test]
fn all_affix_methods_require_two_distinct_proper_matches() {
    let vocabulary = vocabulary();
    // Real names supply one shared suffix and one full, unshared spelling.
    let affix = "_helmet";
    assert!(vocabulary.affixes.contains(affix));
    let unique = vocabulary
        .bare
        .iter()
        .find(|name| !vocabulary.affixes.contains(*name))
        .unwrap();
    for method in [
        "ends_with",
        "starts_with",
        "strip_suffix",
        "strip_prefix",
        "contains",
        "trim_matches",
        "trim_start_matches",
        "trim_end_matches",
        "trim_left_matches",
        "trim_right_matches",
    ] {
        let source = format!("fn f() {{ name.{method}(({affix:?})); }}");
        assert_eq!(literals(&source, &vocabulary), [affix], "{method}");
        if method != "contains" {
            assert!(
                literals(
                    &format!("fn f() {{ name.{method}({unique:?}); }}"),
                    &vocabulary
                )
                .is_empty()
            );
        }
    }
    let full = vocabulary
        .full
        .iter()
        .filter(|name| name.ends_with(affix))
        .take(2)
        .cloned()
        .collect();
    let two = Vocabulary::new(full);
    assert!(two.affixes.contains(affix));
    let one = Vocabulary::new(BTreeSet::from([two.full.first().unwrap().clone()]));
    assert!(!one.affixes.contains(affix));
    let empty = "";
    assert!(!two.affixes.contains(empty));
}

#[test]
fn namespace_aliases_and_domains_do_not_double_count_affix_members() {
    let vocabulary = vocabulary();
    let (name, bare) = identity(&vocabulary);
    let single = Vocabulary::new(BTreeSet::from([name.to_owned(), bare.to_owned()]));
    assert!(single.affixes.is_empty());
}

#[test]
fn overlapping_shapes_count_each_literal_token_once() {
    let vocabulary = vocabulary();
    let (name, _) = identity(&vocabulary);
    let source = format!(
        "const IDS: &[&str] = &[{name:?}, {name:?}]; fn f() {{ name.contains(\"_helmet\"); }}"
    );
    assert_eq!(literals(&source, &vocabulary), [name, name, "_helmet"]);
}

#[test]
fn macros_preserve_patterns_and_nested_selector_calls() {
    let vocabulary = vocabulary();
    let (_, bare) = identity(&vocabulary);
    for source in [
        format!("fn f() {{ assert!(matches!(name, {bare:?})); }}"),
        format!("macro_rules! f {{ ($name:expr) => {{ matches!($name, {bare:?}) }} }}"),
        format!("macro_rules! f {{ ($name:expr) => {{ $name == {bare:?} }} }}"),
        "macro_rules! f { ($name:expr) => { $name.ends_with(\"_helmet\") } }".to_owned(),
    ] {
        assert_eq!(literals(&source, &vocabulary).len(), 1, "{source}");
    }
}

#[test]
fn skips_test_items_impl_members_trait_defaults_and_registered_features() {
    let vocabulary = vocabulary();
    let (name, _) = identity(&vocabulary);
    for source in [
        format!("#[cfg(test)] mod helpers {{ const NAME: &str = {name:?}; }}"),
        format!("#[test] fn f() {{ display({name:?}); }}"),
        format!("#[tokio::test] async fn f() {{ display({name:?}); }}"),
        format!("#[cfg(feature = \"test-support\")] fn f() {{ display({name:?}); }}"),
        format!("#[cfg(all(unix, test))] fn f() {{ display({name:?}); }}"),
        format!("#[cfg(any(test, feature = \"test-support\"))] fn f() {{ display({name:?}); }}"),
        format!("impl A {{ #[cfg(test)] fn f() {{ display({name:?}); }} }}"),
        format!("trait A {{ #[cfg(test)] const NAME: &str = {name:?}; }}"),
        format!("#![cfg(test)] const NAME: &str = {name:?};"),
        format!("fn f() {{ #[cfg(test)] let name = {name:?}; }}"),
        format!("fn f() {{ #[cfg(test)] {{ display({name:?}); }} }}"),
        format!("fn f() {{ #[cfg(test)] display({name:?}); }}"),
        format!("fn f() {{ match name {{ #[cfg(test)] {name:?} => (), _ => () }}; }}"),
    ] {
        assert!(literals(&source, &vocabulary).is_empty(), "{source}");
    }
    for attribute in ["#[cfg(not(test))]", "#[cfg(any(test, unix))]"] {
        assert_eq!(
            literals(
                &format!("{attribute} fn f() {{ display({name:?}); }}"),
                &vocabulary
            ),
            [name]
        );
    }
    let source = format!(
        "#[cfg(test)] fn test() {{ display({name:?}); }} fn production() {{ display({name:?}); }}"
    );
    assert_eq!(literals(&source, &vocabulary), [name]);
}

#[test]
fn skips_each_policy_owner_and_test_file_location() {
    let vocabulary = vocabulary();
    let (name, _) = identity(&vocabulary);
    let source = format!("const NAME: &str = {name:?};");
    for owner in &policy().identifier_literals.owners {
        let path = if owner.ends_with('/') {
            format!("{owner}nested/owner.rs")
        } else {
            owner.clone()
        };
        assert!(gate(&[(&path, &source)], |_| {}).is_empty(), "{path}");
    }
    for path in [
        "src/tests.rs",
        "src/tests/helper.rs",
        "src/fixture_tests.rs",
        "src/fixture_tests/helper.rs",
    ] {
        assert!(gate(&[(path, &source)], |_| {}).is_empty(), "{path}");
    }
    assert!(!gate(&[("crates/assets/src/facts_extra.rs", &source)], |_| {}).is_empty());
    assert!(!gate(&[("crates/json-ui/src/vanilla_extra.rs", &source)], |_| {}).is_empty());
}

#[test]
fn test_scope_propagates_through_out_of_line_and_explicit_path_modules() {
    let vocabulary = vocabulary();
    let (name, _) = identity(&vocabulary);
    let source = format!("const NAME: &str = {name:?};");
    let files = [
        (
            "src/lib.rs",
            "#[cfg(test)] #[path = \"helpers.rs\"] mod helpers; #[cfg(test)] mod nested;",
        ),
        ("src/helpers.rs", "mod child;"),
        ("src/helpers/child.rs", source.as_str()),
        ("src/nested/mod.rs", "mod child;"),
        ("src/nested/child.rs", source.as_str()),
    ];
    assert!(gate(&files, |_| {}).is_empty());
}

#[test]
fn production_reachability_wins_when_tests_import_the_same_module_tree() {
    let vocabulary = vocabulary();
    let (name, _) = identity(&vocabulary);
    let shared = format!(
        "mod child; fn production() {{ display({name:?}); }} #[cfg(test)] const TEST: &str = {name:?};"
    );
    let child = format!("fn production() {{ display({name:?}); }}");
    for entrypoint in ["src/lib.rs", "src/main.rs"] {
        let files = [
            (entrypoint, "mod shared;"),
            ("src/shared.rs", shared.as_str()),
            ("src/shared/child.rs", child.as_str()),
            (
                "tests/shared_import.rs",
                "#[path = \"../src/shared.rs\"] mod shared;",
            ),
        ];
        let diagnostics = gate(&files, |_| {});
        let counts: Vec<_> = diagnostics
            .iter()
            .filter(|line| line.contains("count 1 exceeds budget 0"))
            .collect();
        assert_eq!(counts.len(), 2, "{diagnostics:?}");
        assert!(counts.iter().any(|line| line.starts_with("src/shared.rs:")));
        assert!(
            counts
                .iter()
                .any(|line| line.starts_with("src/shared/child.rs:"))
        );
    }
}

#[test]
fn production_path_imports_override_test_directory_names() {
    let vocabulary = vocabulary();
    let (name, _) = identity(&vocabulary);
    let shared = format!("mod child; fn production() {{ display({name:?}); }}");
    let child = format!("fn production() {{ display({name:?}); }}");
    let files = [
        ("src/lib.rs", "#[path = \"../tests/shared.rs\"] mod shared;"),
        ("tests/shared.rs", shared.as_str()),
        ("tests/child.rs", child.as_str()),
        ("tests/import.rs", "#[path = \"shared.rs\"] mod shared;"),
    ];
    let diagnostics = gate(&files, |_| {});
    assert_eq!(
        diagnostics
            .iter()
            .filter(|line| line.contains("count 1 exceeds budget 0"))
            .count(),
        2,
        "{diagnostics:?}"
    );
}

#[test]
fn exceptions_are_exact_and_must_be_used_and_reasoned() {
    let vocabulary = vocabulary();
    let (name, bare) = identity(&vocabulary);
    let source = format!("fn f() {{ display({name:?}); }}");
    let exception = || IdentifierException {
        path: "src/lib.rs".into(),
        literal: name.into(),
        reason: "Retained identifier during migration.".into(),
    };
    assert!(
        gate(&[("src/lib.rs", &source)], |policy| policy
            .identifier_literal_exceptions
            .push(exception()))
        .is_empty()
    );
    let unused = gate(&[("src/lib.rs", "fn f() {}")], |policy| {
        policy.identifier_literal_exceptions.push(exception())
    });
    assert!(
        unused
            .iter()
            .any(|line| line.contains("unused identifier literal exception"))
    );
    let wrong_file = gate(&[("src/other.rs", &source)], |policy| {
        policy.identifier_literal_exceptions.push(exception())
    });
    assert!(
        wrong_file
            .iter()
            .any(|line| line.contains("count 1 exceeds budget 0"))
    );
    let wrong_literal = gate(&[("src/lib.rs", &source)], |policy| {
        let mut entry = exception();
        entry.literal = bare.into();
        policy.identifier_literal_exceptions.push(entry);
    });
    assert!(
        wrong_literal
            .iter()
            .any(|line| line.contains("count 1 exceeds budget 0"))
    );
    let missing_reason = gate(&[("src/lib.rs", &source)], |policy| {
        let mut entry = exception();
        entry.reason.clear();
        policy.identifier_literal_exceptions.push(entry);
    });
    assert!(
        missing_reason
            .iter()
            .any(|line| line.contains("needs a reason"))
    );
}

#[test]
fn ratchet_allows_equal_and_falling_counts_but_rejects_rises_and_new_files() {
    let vocabulary = vocabulary();
    let (name, _) = identity(&vocabulary);
    let one = format!("fn f() {{ display({name:?}); }}");
    let two = format!("fn f() {{ display({name:?}); display({name:?}); }}");
    let zero = "fn f() {}".to_owned();
    for (source, max) in [(&one, 1), (&one, 2), (&two, 2), (&zero, 1)] {
        assert!(
            gate(&[("src/lib.rs", source)], |policy| policy
                .identifier_literals
                .baselines
                .push(LineBaseline {
                    path: "src/lib.rs".into(),
                    max
                }))
            .is_empty()
        );
    }
    let rise = gate(&[("src/lib.rs", &two)], |policy| {
        policy.identifier_literals.baselines.push(LineBaseline {
            path: "src/lib.rs".into(),
            max: 1,
        })
    });
    assert!(
        rise.iter()
            .any(|line| line.contains("count 2 exceeds budget 1") && line.contains("max = 2"))
    );
    assert!(!gate(&[("src/new.rs", &one)], |_| {}).is_empty());
}

#[test]
fn malformed_rust_and_vocabulary_rows_fail_the_gate() {
    let malformed = gate(&[("src/lib.rs", "fn {")], |_| {});
    assert!(
        malformed
            .iter()
            .any(|line| line.contains("cannot parse Rust for identifier_literals"))
    );
    let malformed = gate(&[], |policy| {
        policy.identifier_literals.vocabularies[0].column = usize::MAX
    });
    assert!(
        malformed
            .iter()
            .any(|line| line.contains("missing identifier vocabulary column"))
    );
}

#[test]
fn duplicate_exceptions_and_unused_budgets_are_rejected() {
    let vocabulary = vocabulary();
    let (name, _) = identity(&vocabulary);
    let source = format!("fn f() {{ display({name:?}); }}");
    let duplicates = gate(&[("src/lib.rs", &source)], |policy| {
        for _ in 0..2 {
            policy
                .identifier_literal_exceptions
                .push(IdentifierException {
                    path: "src/lib.rs".into(),
                    literal: name.into(),
                    reason: "Retained during migration.".into(),
                });
        }
    });
    assert!(
        duplicates
            .iter()
            .any(|line| line.contains("duplicate identifier literal exception"))
    );
    let unused = gate(&[], |policy| {
        policy.identifier_literals.baselines.push(LineBaseline {
            path: "src/gone.rs".into(),
            max: 1,
        })
    });
    assert!(
        unused
            .iter()
            .any(|line| line.contains("unused identifier_literals budget"))
    );
    let duplicates = gate(&[("src/lib.rs", "fn f() {}")], |policy| {
        for _ in 0..2 {
            policy.identifier_literals.baselines.push(LineBaseline {
                path: "src/lib.rs".into(),
                max: 1,
            });
        }
    });
    assert!(
        duplicates
            .iter()
            .any(|line| line.contains("duplicate identifier_literals budget"))
    );
}
