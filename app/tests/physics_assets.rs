use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, SystemTime},
};

use sha2::{Digest, Sha256};

#[test]
fn make_client_acquires_and_builds_the_required_physics_registry() {
    let makefile = read_makefile();
    for contract in [
        "BLOCK_DATA_MANIFEST ?= assets/block-data-sources.json",
        "BLOCK_DATA_DIR ?= .local/assets/block-data",
        "PHYSICS_REGISTRY ?= .local/assets/block-physics-v1001.bin",
        "PHYSICS_REGISTRY_SHA256 ?= crates/assets/data/block-physics-v1001.sha256",
        "physics-assets: $(PHYSICS_REGISTRY)",
        "$(GO) -C tools/registrygen run ./cmd/datafetch",
        "-manifest \"$(abspath $(BLOCK_DATA_MANIFEST))\"",
        "-out \"$(abspath $(BLOCK_DATA_DIR))\"",
        "-light-breg \"$(abspath $(BLOCK_REGISTRY))\"",
        "-physics-out \"$(abspath $(PHYSICS_REGISTRY))\"",
        "-physics-breg \"$(abspath $(BLOCK_REGISTRY))\"",
        "$(GO) -C tools/registrygen run ./cmd/hashcheck",
        "$(PHYSICS_REGISTRY_CHECK) || ( $(PHYSICS_REGISTRY_COMPILE) && $(PHYSICS_REGISTRY_CHECK) )",
    ] {
        assert!(
            makefile.contains(contract),
            "missing physics Makefile contract: {contract}"
        );
    }
    let phony = makefile
        .lines()
        .find(|line| line.starts_with(".PHONY:"))
        .unwrap();
    assert!(
        phony
            .split_whitespace()
            .any(|word| word == "physics-assets")
    );
    assert!(
        !phony
            .split_whitespace()
            .any(|word| word == "$(PHYSICS_REGISTRY)")
    );
    let client = makefile
        .lines()
        .find(|line| line.starts_with("client:"))
        .unwrap();
    assert!(
        client
            .split_whitespace()
            .any(|word| word == "physics-assets")
    );
}

#[test]
fn make_physics_assets_repairs_a_newer_corrupt_registry_once() {
    if !make_available() {
        eprintln!("skipping executable physics Makefile test: `make` is unavailable");
        return;
    }
    let root = workspace_root();
    let temporary = temporary_directory("make-physics-recovery");
    let physics = temporary.join("block-physics.bin");
    let expected_sha = temporary.join("block-physics.sha256");
    let invocation_log = temporary.join("invocations.log");
    let prerequisites = [
        temporary.join("protocol_info.json"),
        temporary.join("block-registry.bin"),
        temporary.join("light-registry.bin"),
        temporary.join("palette.bin"),
        temporary.join("blocks.rs"),
    ];
    for prerequisite in &prerequisites {
        fs::write(prerequisite, b"fixture").unwrap();
    }
    fs::write(&physics, b"corrupt but newer\n").unwrap();
    let repaired = b"repaired\n";
    fs::write(&expected_sha, format!("{:x}\n", Sha256::digest(repaired))).unwrap();
    let old = SystemTime::now() - Duration::from_secs(120);
    for prerequisite in prerequisites.iter().chain([&expected_sha]) {
        fs::File::options()
            .write(true)
            .open(prerequisite)
            .unwrap()
            .set_modified(old)
            .unwrap();
    }
    let compile = format!(
        "echo invocation >> \"{}\" && echo repaired > \"{}\"",
        make_path(&invocation_log),
        make_path(&physics)
    );
    let assignments = [
        "REGISTRYGEN_INPUTS=".to_owned(),
        "BLOCK_DATA_FETCH_INPUTS=".to_owned(),
        format!("BLOCK_DATA_SENTINEL={}", make_path(&prerequisites[0])),
        format!("BLOCK_REGISTRY={}", make_path(&prerequisites[1])),
        format!("LIGHT_REGISTRY={}", make_path(&prerequisites[2])),
        format!("VALENTINE_PALETTE={}", make_path(&prerequisites[3])),
        format!("VALENTINE_BLOCKS={}", make_path(&prerequisites[4])),
        format!("PHYSICS_REGISTRY={}", make_path(&physics)),
        format!("PHYSICS_REGISTRY_SHA256={}", make_path(&expected_sha)),
        format!("PHYSICS_REGISTRY_COMPILE={compile}"),
    ];
    for expected_invocations in [1, 1] {
        let output = Command::new("make")
            .current_dir(root)
            .args(["-f", "Makefile", "physics-assets"])
            .args(&assignments)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "make physics-assets failed:\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            fs::read_to_string(&invocation_log).unwrap().lines().count(),
            expected_invocations
        );
    }
    fs::remove_dir_all(temporary).unwrap();
}

fn workspace_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap()
}

fn read_makefile() -> String {
    fs::read_to_string(workspace_root().join("Makefile"))
        .unwrap()
        .replace("\r\n", "\n")
}

fn make_available() -> bool {
    matches!(Command::new("make").arg("--version").output(), Ok(output) if output.status.success())
}

fn temporary_directory(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path =
        std::env::temp_dir().join(format!("rust-mcbe-{label}-{}-{nonce}", std::process::id()));
    fs::create_dir_all(&path).unwrap();
    path
}

fn make_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}
