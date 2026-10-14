use super::*;

/// Reports the observed one-literal counts for files newly admitted through Cargo targets.
fn checked_files(sources: &[(&str, &str)]) -> Vec<String> {
    gate(sources, |_| {})
        .into_iter()
        .filter(|line| line.contains("count 1 exceeds budget 0"))
        .map(|line| line.split(':').next().unwrap().to_owned())
        .collect()
}

#[test]
fn explicit_libraries_binaries_and_build_scripts_reach_shared_modules() {
    let vocabulary = vocabulary();
    let (name, _) = identity(&vocabulary);
    let shared = format!("fn production() {{ display({name:?}); }}");
    for (target, manifest, import) in [
        (
            "lib/client.rs",
            "[lib]\npath = 'lib/client.rs'\n",
            r#"#[path = "../shared/owner.rs"] mod owner;"#,
        ),
        (
            "commands/tool.entry",
            "[[bin]]\nname = 'tool'\npath = 'commands/tool.entry'\n",
            r#"#[path = "../shared/owner.rs"] mod owner;"#,
        ),
        ("build.rs", "", r#"#[path = "shared/owner.rs"] mod owner;"#),
        (
            "scripts/build.entry",
            "",
            r#"#[path = "../shared/owner.rs"] mod owner;"#,
        ),
    ] {
        let build = if target == "scripts/build.entry" {
            "build = 'scripts/build.entry'\n"
        } else {
            ""
        };
        let manifest = format!(
            "[package]\nname = 'fixture'\nversion = '0.1.0'\nedition = '2024'\n{build}{manifest}"
        );
        let files = [
            ("Cargo.toml", manifest.as_str()),
            (target, import),
            ("shared/owner.rs", shared.as_str()),
            (
                "tests/import.rs",
                r#"#[path = "../shared/owner.rs"] mod owner;"#,
            ),
        ];
        assert_eq!(checked_files(&files), ["shared/owner.rs"], "{target}");
    }
}

#[test]
fn automatic_binary_targets_use_crate_root_module_directories() {
    let vocabulary = vocabulary();
    let (name, _) = identity(&vocabulary);
    let shared = format!("fn production() {{ display({name:?}); }}");
    for (target, helper, test_import) in [
        (
            "src/bin/tool.rs",
            "src/bin/support/mod.rs",
            r#"#[path = "../src/bin/support/mod.rs"] mod support;"#,
        ),
        (
            "src/bin/tool/main.rs",
            "src/bin/tool/support.rs",
            r#"#[path = "../src/bin/tool/support.rs"] mod support;"#,
        ),
        (
            "build.rs",
            "support.rs",
            r#"#[path = "../support.rs"] mod support;"#,
        ),
    ] {
        let files = [
            (target, "mod support;"),
            (helper, shared.as_str()),
            ("tests/import.rs", test_import),
        ];
        assert_eq!(checked_files(&files), [helper], "{target}");
    }
}

#[test]
fn explicit_binary_paths_override_inferred_targets_with_the_same_name() {
    let vocabulary = vocabulary();
    let (name, _) = identity(&vocabulary);
    let source = format!("fn f() {{ display({name:?}); }}");
    let manifest = "[package]\nname='fixture'\nversion='0.1.0'\nedition='2024'\n[[bin]]\nname='tool'\npath='commands/custom.rs'\n";
    let files = [
        ("Cargo.toml", manifest),
        ("commands/custom.rs", source.as_str()),
        ("src/bin/tool.rs", source.as_str()),
        (
            "tests/import.rs",
            r#"#[path="../commands/custom.rs"] mod custom; #[path="../src/bin/tool.rs"] mod inferred;"#,
        ),
    ];
    assert_eq!(checked_files(&files), ["commands/custom.rs"]);
}

#[test]
fn disabled_auto_discovery_and_build_scripts_do_not_override_test_imports() {
    let vocabulary = vocabulary();
    let (name, _) = identity(&vocabulary);
    let source = format!("fn f() {{ display({name:?}); }}");
    let manifest = "[package]\nname='fixture'\nversion='0.1.0'\nedition='2024'\nautobins=false\n autolib=false\nbuild=false\n";
    let files = [
        ("Cargo.toml", manifest),
        ("src/lib.rs", source.as_str()),
        ("src/main.rs", source.as_str()),
        ("src/bin/tool.rs", source.as_str()),
        ("build.rs", source.as_str()),
        (
            "tests/import.rs",
            r#"#[path="../src/lib.rs"] mod library; #[path="../src/main.rs"] mod main; #[path="../src/bin/tool.rs"] mod tool; #[path="../build.rs"] mod build;"#,
        ),
    ];
    assert!(checked_files(&files).is_empty());
}

#[test]
fn examples_benches_and_custom_test_targets_are_development_only() {
    let vocabulary = vocabulary();
    let (name, _) = identity(&vocabulary);
    let source = format!("fn f() {{ display({name:?}); }}");
    let manifest = "[package]\nname='fixture'\nversion='0.1.0'\nedition='2024'\n[[test]]\nname='custom_test'\npath='checks/custom.rs'\n[[example]]\nname='custom_example'\npath='demos/custom.rs'\n[[bench]]\nname='custom_bench'\npath='benchmarks/custom.rs'\n";
    let files = [
        ("Cargo.toml", manifest),
        (
            "examples/demo.rs",
            r#"#[path="../shared/owner.rs"] mod owner;"#,
        ),
        (
            "benches/speed/main.rs",
            r#"#[path="../../../shared/owner.rs"] mod owner;"#,
        ),
        (
            "checks/custom.rs",
            r#"#[path="../shared/owner.rs"] mod owner;"#,
        ),
        (
            "demos/custom.rs",
            r#"#[path="../shared/owner.rs"] mod owner;"#,
        ),
        (
            "benchmarks/custom.rs",
            r#"#[path="../shared/owner.rs"] mod owner;"#,
        ),
        ("shared/owner.rs", source.as_str()),
    ];
    assert!(gate(&files, |_| {}).is_empty());
}

#[test]
fn workspace_editions_and_named_binary_inference_are_respected() {
    let vocabulary = vocabulary();
    let (name, _) = identity(&vocabulary);
    let source = format!("fn f() {{ display({name:?}); }}");
    let manifest = "[package]\nname='fixture'\nversion='0.1.0'\nedition.workspace=true\n[workspace.package]\nedition='2024'\n[[bin]]\nname='named'\n";
    let files = [
        ("Cargo.toml", manifest),
        ("src/bin/named.rs", source.as_str()),
        ("src/bin/extra/main.rs", source.as_str()),
        (
            "tests/import.rs",
            r#"#[path="../src/bin/named.rs"] mod named; #[path="../src/bin/extra/main.rs"] mod extra;"#,
        ),
    ];
    let mut observed = checked_files(&files);
    observed.sort();
    assert_eq!(observed, ["src/bin/extra/main.rs", "src/bin/named.rs"]);
}

#[test]
fn legacy_target_defaults_apply_per_kind_and_resolve_historical_paths() {
    let vocabulary = vocabulary();
    let (name, _) = identity(&vocabulary);
    let source = format!("fn f() {{ display({name:?}); }}");
    let manifest =
        "[package]\nname='fixture'\nversion='0.1.0'\nedition='2015'\n[[bin]]\nname='named'\n";
    let files = [
        ("Cargo.toml", manifest),
        ("src/lib.rs", source.as_str()),
        ("src/main.rs", source.as_str()),
        ("src/bin/extra.rs", source.as_str()),
        (
            "tests/import.rs",
            r#"#[path="../src/lib.rs"] mod library; #[path="../src/main.rs"] mod main; #[path="../src/bin/extra.rs"] mod extra;"#,
        ),
    ];
    let mut observed = checked_files(&files);
    observed.sort();
    assert_eq!(observed, ["src/lib.rs", "src/main.rs"]);
}

#[test]
fn test_support_binary_aliases_do_not_make_the_same_path_a_production_root() {
    let vocabulary = vocabulary();
    let (name, _) = identity(&vocabulary);
    let source = format!("fn f() {{ display({name:?}); }}");
    let manifest = "[package]\nname='fixture'\nversion='0.1.0'\nedition='2024'\n[[bin]]\nname='probe'\npath='src/bin/shared.rs'\nrequired-features=['test-support']\n";
    let files = [
        ("Cargo.toml", manifest),
        ("src/bin/shared.rs", source.as_str()),
    ];
    assert!(gate(&files, |_| {}).is_empty());
}

#[test]
fn hidden_auto_targets_stay_exempt_when_only_tests_import_them() {
    let vocabulary = vocabulary();
    let (name, _) = identity(&vocabulary);
    let source = format!("fn f() {{ display({name:?}); }}");
    let files = [
        ("src/bin/.hidden.rs", source.as_str()),
        (
            "tests/import.rs",
            r#"#[path="../src/bin/.hidden.rs"] mod hidden;"#,
        ),
    ];
    assert!(gate(&files, |_| {}).is_empty());
}
