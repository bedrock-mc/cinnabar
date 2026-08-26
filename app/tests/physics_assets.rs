use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::SystemTime,
};

use sha2::{Digest, Sha256};

#[test]
fn make_client_acquires_and_builds_the_required_physics_registry() {
    let makefile = read_makefile();
    for contract in [
        "PHYSICS_REGISTRY ?= .local/assets/block-physics-v2168.bin",
        "PHYSICS_REGISTRY_SOURCE ?= crates/assets/data/block-physics-v2168.bin",
        "PHYSICS_REGISTRY_SHA256 ?= crates/assets/data/block-physics-v2168.sha256",
        "physics-assets: $(PHYSICS_REGISTRY)",
        "$(PHYSICS_REGISTRY): $(PHYSICS_REGISTRY_SOURCE) $(PHYSICS_REGISTRY_SHA256) $(BEDROCK_TARGET_MANIFEST)",
        "$(PHYSICS_REGISTRY_INSTALL)",
        "$(GO) -C tools/registrygen run ./cmd/hashcheck",
        "$(PHYSICS_REGISTRY_CHECK) || ( $(PHYSICS_REGISTRY_INSTALL) && $(PHYSICS_REGISTRY_CHECK) )",
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
fn make_physics_assets_installs_the_pinned_registry_once() {
    if !make_available() {
        eprintln!("skipping executable physics Makefile test: `make` is unavailable");
        return;
    }
    let root = workspace_root();
    let temporary = temporary_directory("make-physics-recovery");
    let physics = temporary.join("block-physics.bin");
    let expected_sha = temporary.join("block-physics.sha256");
    let invocation_log = temporary.join("invocations.log");
    let source = temporary.join("checked-in-block-physics.bin");
    let manifest = temporary.join("bedrock-target.json");
    let pinned = b"protocol-2168-physics";
    fs::write(&source, pinned).unwrap();
    fs::write(&manifest, b"{}").unwrap();
    fs::write(&expected_sha, format!("{:x}\n", Sha256::digest(pinned))).unwrap();
    let install = if cfg!(windows) {
        format!(
            "echo invocation >> \"{}\" && copy /Y \"{}\" \"{}\" >NUL",
            make_path(&invocation_log),
            make_path(&source),
            make_path(&physics)
        )
    } else {
        format!(
            "echo invocation >> \"{}\" && cp \"{}\" \"{}\"",
            make_path(&invocation_log),
            make_path(&source),
            make_path(&physics)
        )
    };
    let assignments = [
        format!("PHYSICS_REGISTRY={}", make_path(&physics)),
        format!("PHYSICS_REGISTRY_SOURCE={}", make_path(&source)),
        format!("PHYSICS_REGISTRY_SHA256={}", make_path(&expected_sha)),
        format!("BEDROCK_TARGET_MANIFEST={}", make_path(&manifest)),
        format!("PHYSICS_REGISTRY_INSTALL={install}"),
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
    assert_eq!(fs::read(&physics).unwrap(), pinned);
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
