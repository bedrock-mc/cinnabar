use std::{path::PathBuf, process::Command};

/// Locates the raw Go source exporter and committed regeneration inputs.
fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn active_registry_regeneration_preserves_every_output_byte() {
    let root = root();
    for input in [
        "crates/protocol/data/retail_items_1_26_50.tsv",
        "crates/protocol/data/retail_biomes_1_26_50.txt",
        "crates/assets/data/block-registry-v1001.bin",
    ] {
        if !root.join(input).is_file() {
            eprintln!("missing fixture: {input}; skipping registry regeneration");
            return;
        }
    }
    let scratch = tempfile::tempdir().unwrap();
    let facts = scratch.path().join("facts.json");
    let export = Command::new("go")
        .args(["-C", "tools/registrygen", "run", ".", "-facts-out"])
        .arg(&facts)
        .current_dir(&root)
        .output()
        .unwrap();
    assert!(
        export.status.success(),
        "{}",
        String::from_utf8_lossy(&export.stderr)
    );
    let compile = Command::new(env!("CARGO_BIN_EXE_registryc"))
        .arg("--root")
        .arg(&root)
        .arg("--facts")
        .arg(facts)
        .args(["--kind", "all", "--check", "--out"])
        .arg(scratch.path().join("regenerated"))
        .output()
        .unwrap();
    assert!(
        compile.status.success(),
        "{}",
        String::from_utf8_lossy(&compile.stderr)
    );
}

#[test]
fn pinned_pack_vocabularies_regenerate_byte_for_byte() {
    let root = root();
    let source = assets::VanillaSource::read(&root.join("assets/vanilla-source.json")).unwrap();
    let paths = source.local_paths(&root).unwrap();
    if !paths.archive.is_file() {
        eprintln!(
            "missing fixture: {}; skipping pack vocabulary regeneration",
            paths.archive.display()
        );
        return;
    }
    let scratch = tempfile::tempdir().unwrap();
    let out = scratch.path().join("vocabulary.tsv");
    let actors = scratch.path().join("actors.txt");
    let result = Command::new(env!("CARGO_BIN_EXE_assetc"))
        .arg("vocabulary")
        .arg("--archive")
        .arg(&paths.archive)
        .arg("--source-manifest")
        .arg(root.join("assets/vanilla-source.json"))
        .arg("--out")
        .arg(&out)
        .arg("--actors-out")
        .arg(&actors)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(
        std::fs::read(out).unwrap(),
        include_bytes!("../../../assets/data/vanilla-pack-vocabulary-1.26.50.tsv")
    );
    assert_eq!(
        std::fs::read(actors).unwrap(),
        include_bytes!("../../../protocol/data/retail_actors_1_26_50.txt")
    );
    let derivative = Command::new("python3")
        .arg(root.join("tools/registrygen/generate_block_inputs.py"))
        .arg("--archive")
        .arg(&paths.archive)
        .arg("--out")
        .arg(scratch.path())
        .output()
        .unwrap();
    assert!(
        derivative.status.success(),
        "{}",
        String::from_utf8_lossy(&derivative.stderr)
    );
    for filename in ["block-properties-v2193.json", "block-components-v2193.tsv"] {
        assert_eq!(
            std::fs::read(scratch.path().join("crates/assets/data").join(filename)).unwrap(),
            std::fs::read(root.join("crates/assets/data").join(filename)).unwrap()
        );
    }
}

#[test]
fn item_only_regeneration_does_not_need_a_block_export_or_fetched_pack() {
    let root = root();
    let scratch = tempfile::tempdir().unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_registryc"))
        .arg("--root")
        .arg(&root)
        .args(["--kind", "items", "--check", "--out"])
        .arg(scratch.path().join(assets::facts::ITEM_FACTS_FILENAME))
        .arg("--manifest")
        .arg(scratch.path().join("expansion.tsv"))
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}
