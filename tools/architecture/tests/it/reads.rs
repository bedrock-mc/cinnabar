use std::{fs, path::Path};

use architecture::check_repository;

fn write(path: &Path, contents: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

/// Returns read findings for `alpha`, whose source may read the `beta` crate's files.
fn findings(source: &str, alpha_metadata: &str) -> Vec<String> {
    findings_at(source, alpha_metadata, "crates/alpha")
}

/// Builds the read fixture with an ordinary or nested reader package.
fn findings_at(source: &str, alpha_metadata: &str, alpha_root: &str) -> Vec<String> {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    write(
        &root.join("Cargo.toml"),
        &format!("[workspace]\nmembers=['{alpha_root}','crates/beta']\n"),
    );
    write(
        &root.join(alpha_root).join("Cargo.toml"),
        &format!("[package]\nname='alpha'\nversion='0.1.0'\n{alpha_metadata}\n"),
    );
    write(&root.join(alpha_root).join("src/lib.rs"), source);
    write(
        &root.join("crates/beta/Cargo.toml"),
        "[package]\nname='beta'\nversion='0.1.0'\n",
    );
    write(&root.join("crates/beta/data/table.tsv"), "id\n");
    write(&root.join("assets/shared.json"), "{}\n");
    write(&root.join("plan.md"), "# Plan\n");
    write(
        &root.join("policy.toml"),
        &format!(
            "production_rust_max=1000\nmodule_root_max=300\npowershell_max=800\ntest_max=1200\n\
             [[crates]]\nname='alpha'\npath='{alpha_root}'\n[[crates]]\nname='beta'\npath='crates/beta'\n"
        ),
    );
    check_repository(root, &root.join("policy.toml"))
        .unwrap()
        .into_iter()
        .filter(|line| line.contains("reads `") || line.contains("declared read"))
        .collect()
}

const MISSING: &str = "alpha: crates/alpha/src/lib.rs reads `crates/beta/data/table.tsv` outside the crate; list it in `reads` under [package.metadata.cinnabar]";

#[test]
fn undeclared_reads_of_another_crate_fail() {
    for source in [
        "const TABLE: &str = include_str!(\"../../beta/data/table.tsv\");",
        "const TABLE: &[u8] = include_bytes!(\"../../beta/data/table.tsv\");",
        "const TABLE: &str = concat!(include_str!(\"../../beta/data/table.tsv\"), \"\\n\");",
        "const TABLE: &str = include_str!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/../beta/data/table.tsv\"));",
        "fn table() -> std::path::PathBuf { std::path::Path::new(env!(\"CARGO_MANIFEST_DIR\")).join(\"../beta/data/table.tsv\") }",
        "fn table() -> std::path::PathBuf { std::path::Path::new(env!(\"CARGO_MANIFEST_DIR\")).join(\"../../crates/beta/data/table.tsv\") }",
    ] {
        assert_eq!(findings(source, ""), [MISSING], "{source}");
        for declaration in [
            "crates/beta/data/table.tsv",
            "crates/beta/data/*",
            "crates/beta/data",
        ] {
            assert!(
                findings(
                    source,
                    &format!("[package.metadata.cinnabar]\nreads=['{declaration}']")
                )
                .is_empty(),
                "{source} with {declaration}"
            );
        }
    }
}

#[test]
fn directory_reads_need_a_covering_declaration() {
    let source = "fn data() -> std::path::PathBuf { std::path::Path::new(env!(\"CARGO_MANIFEST_DIR\")).join(\"../beta/data\") }";
    assert_eq!(
        findings(source, ""),
        [
            "alpha: crates/alpha/src/lib.rs reads `crates/beta/data` outside the crate; list it in `reads` under [package.metadata.cinnabar]"
        ]
    );
    assert!(
        findings(
            source,
            "[package.metadata.cinnabar]\nreads=['crates/beta/data/*']"
        )
        .is_empty()
    );
}

#[test]
fn own_files_and_repository_paths_named_as_data_need_no_declaration() {
    for source in [
        "const OWN: &str = include_str!(\"../data/own.tsv\");",
        "const FIXTURE: &str = \"crates/beta/data/table.tsv\";",
        "const OUTSIDE: &str = \"../../../escape\";",
    ] {
        assert!(findings(source, "").is_empty(), "{source}");
    }
}

#[test]
fn stale_declarations_fail_unless_a_source_still_names_the_path() {
    let declared = "[package.metadata.cinnabar]\nreads=['crates/beta/data/table.tsv']";
    assert_eq!(
        findings("fn portable() {}", declared),
        ["alpha: declared read `crates/beta/data/table.tsv` matches nothing its sources read"]
    );
    assert!(
        findings(
            "fn root() -> &'static str { \"crates/beta/data/table.tsv\" }",
            declared
        )
        .is_empty()
    );
}

#[test]
fn reads_of_existing_paths_outside_every_crate_need_declarations() {
    let source = "const SHARED: &str = include_str!(\"../../../assets/shared.json\");";
    assert_eq!(
        findings(source, ""),
        [
            "alpha: crates/alpha/src/lib.rs reads `assets/shared.json` outside the crate; list it in `reads` under [package.metadata.cinnabar]"
        ]
    );
    for declaration in ["assets/shared.json", "assets", "assets/*"] {
        assert!(
            findings(
                source,
                &format!("[package.metadata.cinnabar]\nreads=['{declaration}']")
            )
            .is_empty(),
            "{declaration}"
        );
    }
    assert!(
        findings("const GONE: &str = \"../../assets/missing.json\";", "").is_empty(),
        "paths that do not exist are not reads"
    );
    assert!(findings("const PARENT: &str = \"../\";", "").is_empty());
}

#[test]
fn declarations_outside_every_crate_go_stale_too() {
    let declared = "[package.metadata.cinnabar]\nreads=['plan.md']";
    assert_eq!(
        findings("fn portable() {}", declared),
        ["alpha: declared read `plan.md` matches nothing its sources read"]
    );
    assert!(findings("fn plan() -> &'static str { \"plan.md\" }", declared).is_empty());
}

#[test]
fn joins_from_the_manifest_directory_are_followed_through_bindings() {
    // A source tree joined relative to another manifest-relative path, as render's shader check does.
    let tree = "fn sources() { let source = std::path::PathBuf::from(env!(\"CARGO_MANIFEST_DIR\")).join(\"src\"); let other = source.join(\"../../beta/data\"); let _ = other; }";
    assert_eq!(
        findings(tree, ""),
        [
            "alpha: crates/alpha/src/lib.rs reads `crates/beta/data` outside the crate; list it in `reads` under [package.metadata.cinnabar]"
        ]
    );
    assert_eq!(
        findings(
            tree,
            "[package.metadata.cinnabar]\nreads=['crates/beta/data/table.tsv']"
        )
        .len(),
        1,
        "a file inside a read directory does not declare the directory"
    );
    assert!(
        findings(
            tree,
            "[package.metadata.cinnabar]\nreads=['crates/beta/data']"
        )
        .is_empty()
    );

    // A repository root bound once and joined later, as pack-compiler's font test does.
    let root = "fn manifest() -> Vec<u8> { let root = std::path::Path::new(env!(\"CARGO_MANIFEST_DIR\")).join(\"../..\"); std::fs::read(root.join(\"assets/shared.json\")).unwrap() }";
    assert_eq!(
        findings(root, ""),
        [
            "alpha: crates/alpha/src/lib.rs reads `assets/shared.json` outside the crate; list it in `reads` under [package.metadata.cinnabar]"
        ]
    );
    assert!(
        findings(
            root,
            "[package.metadata.cinnabar]\nreads=['assets/shared.json']"
        )
        .is_empty()
    );
}

#[test]
fn computed_joins_and_parent_steps_are_not_reads() {
    for source in [
        "fn path(name: &str) -> std::path::PathBuf { std::path::Path::new(env!(\"CARGO_MANIFEST_DIR\")).join(\"../..\").join(name) }",
        "fn parent() -> std::path::PathBuf { std::path::Path::new(env!(\"CARGO_MANIFEST_DIR\")).join(\"..\") }",
        "fn local() -> std::path::PathBuf { std::path::Path::new(env!(\"CARGO_MANIFEST_DIR\")).join(\"../../.local/cache\") }",
    ] {
        assert!(findings(source, "").is_empty(), "{source}");
    }
}

#[test]
fn nested_package_literals_keep_real_parent_reads_without_inventing_inputs() {
    let reader = "crates/beta/alpha";
    assert!(findings_at("const INVALID: &str = \"../secret\";", "", reader).is_empty());
    let real = "const INPUT: &str = \"../data/table.tsv\";";
    assert_eq!(
        findings_at(real, "", reader),
        [
            "alpha: crates/beta/alpha/src/lib.rs reads `crates/beta/data/table.tsv` outside the crate; list it in `reads` under [package.metadata.cinnabar]"
        ]
    );
    assert!(
        findings_at(
            real,
            "[package.metadata.cinnabar]\nreads=['crates/beta/data/table.tsv']",
            reader
        )
        .is_empty()
    );

    for source in [
        "const INPUT: &str = include_str!(\"../../data/missing.tsv\");",
        "fn input() -> std::path::PathBuf { std::path::Path::new(env!(\"CARGO_MANIFEST_DIR\")).join(\"../data/missing.tsv\") }",
    ] {
        assert_eq!(
            findings_at(source, "", reader),
            [
                "alpha: crates/beta/alpha/src/lib.rs reads `crates/beta/data/missing.tsv` outside the crate; list it in `reads` under [package.metadata.cinnabar]"
            ],
            "{source}"
        );
    }
}

#[test]
fn policy_selected_identifier_sources_require_architecture_read_declarations() {
    let vocabulary =
        include_str!("../../../../crates/assets/data/vanilla-pack-vocabulary-1.26.50.tsv");
    for declared in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        write(
            &root.join("Cargo.toml"),
            "[workspace]\nmembers=['tools/architecture','crates/beta']\n",
        );
        let metadata = if declared {
            "[package.metadata.cinnabar]\nreads=['crates/beta/data/table.tsv']\n"
        } else {
            ""
        };
        write(
            &root.join("tools/architecture/Cargo.toml"),
            &format!("[package]\nname='architecture'\nversion='0.1.0'\n{metadata}"),
        );
        write(
            &root.join("tools/architecture/src/lib.rs"),
            "fn gate() {}\n",
        );
        write(
            &root.join("crates/beta/Cargo.toml"),
            "[package]\nname='beta'\nversion='0.1.0'\n",
        );
        write(&root.join("crates/beta/src/lib.rs"), "");
        write(&root.join("crates/beta/data/table.tsv"), vocabulary);
        write(
            &root.join("policy.toml"),
            "production_rust_max=1000\nmodule_root_max=300\npowershell_max=800\ntest_max=1200\n[[crates]]\nname='architecture'\npath='tools/architecture'\n[[crates]]\nname='beta'\npath='crates/beta'\n[identifier_literals]\nvocabularies=[{path='crates/beta/data/table.tsv',column=1}]\n",
        );
        let diagnostics = check_repository(root, &root.join("policy.toml")).unwrap();
        let findings: Vec<_> = diagnostics
            .iter()
            .filter(|line| line.contains("reads `") || line.contains("declared read"))
            .collect();
        if declared {
            assert!(findings.is_empty(), "{findings:?}");
        } else {
            assert_eq!(findings.len(), 1);
            assert!(
                findings[0]
                    .contains("policy.identifier_literals reads `crates/beta/data/table.tsv`")
            );
        }
    }
}
