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
        "PHYSICS_REGISTRY_INSTALL = $(POWERSHELL)",
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

/// One recipe shell that GNU Make may use to run the physics-assets recipe.
///
/// GNU Make for Windows selects `sh.exe` when one is reachable through `PATH`
/// (hosted runners with Git for Windows, local Git Bash sessions) and otherwise
/// falls back to `cmd.exe` (local PowerShell sessions). The install recipe must
/// therefore work unchanged under both shells, and this test runs the complete
/// install/hash/reuse sequence under every shell it can reach instead of only
/// the one the current environment happens to pick.
#[derive(Debug, Clone, PartialEq, Eq)]
struct RecipeShell {
    label: &'static str,
    /// `SHELL=` override passed to Make; `None` keeps Make's own selection.
    executable: Option<PathBuf>,
}

#[test]
fn make_physics_assets_installs_the_pinned_registry_once() {
    if !make_available() {
        eprintln!("skipping executable physics Makefile test: `make` is unavailable");
        return;
    }
    let root = workspace_root();
    let shells = recipe_shells();
    assert!(
        !shells.is_empty(),
        "at least the environment-selected shell must be exercised"
    );
    for shell in &shells {
        eprintln!(
            "exercising physics-assets install/hash/reuse under recipe shell `{}`{}",
            shell.label,
            shell
                .executable
                .as_deref()
                .map(|path| format!(" ({})", path.display()))
                .unwrap_or_default()
        );
        install_pinned_registry_once(root, shell);
    }
}

/// Runs `make physics-assets` twice against a fresh pinned fixture and proves
/// that the first run installs exactly once, the hash check accepts the
/// installed bytes, and the second run reuses the installed registry.
fn install_pinned_registry_once(root: &Path, shell: &RecipeShell) {
    let temporary = temporary_directory(&format!("make-physics-recovery-{}", shell.label));
    let physics = temporary.join("block-physics.bin");
    let expected_sha = temporary.join("block-physics.sha256");
    let invocation_log = temporary.join("invocations.log");
    let source = temporary.join("checked-in-block-physics.bin");
    let manifest = temporary.join("bedrock-target.json");
    let pinned = b"protocol-2168-physics";
    fs::write(&source, pinned).unwrap();
    fs::write(&manifest, b"{}").unwrap();
    fs::write(&expected_sha, format!("{:x}\n", Sha256::digest(pinned))).unwrap();
    let install = install_recipe(&invocation_log, &source, &physics);
    let mut assignments = vec![
        format!("PHYSICS_REGISTRY={}", make_path(&physics)),
        format!("PHYSICS_REGISTRY_SOURCE={}", make_path(&source)),
        format!("PHYSICS_REGISTRY_SHA256={}", make_path(&expected_sha)),
        format!("BEDROCK_TARGET_MANIFEST={}", make_path(&manifest)),
        format!("PHYSICS_REGISTRY_INSTALL={install}"),
    ];
    if let Some(executable) = shell.executable.as_deref() {
        assignments.push(format!("SHELL={}", make_path(executable)));
    }
    for expected_invocations in [1, 1] {
        let output = Command::new("make")
            .current_dir(root)
            .args(["-f", "Makefile", "physics-assets"])
            .args(&assignments)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "make physics-assets failed under recipe shell `{}`:\nstdout:\n{}\nstderr:\n{}",
            shell.label,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            fs::read_to_string(&invocation_log).unwrap().lines().count(),
            expected_invocations,
            "install invocation count under recipe shell `{}`",
            shell.label
        );
    }
    assert_eq!(fs::read(&physics).unwrap(), pinned);
    fs::remove_dir_all(temporary).unwrap();
}

/// Builds the overriding install recipe for this test.
///
/// On Windows the recipe must not depend on the recipe shell: `copy` exists
/// only inside `cmd.exe` and `cp` only inside a POSIX shell, but the hosted and
/// local Make installations select different shells. Windows PowerShell is an
/// ordinary executable reachable from both, so the recipe uses it the same way
/// the production `PHYSICS_REGISTRY_INSTALL` does. Forward-slash paths in single
/// quotes stay literal under `cmd.exe`, `sh.exe`, and PowerShell alike.
fn install_recipe(invocation_log: &Path, source: &Path, physics: &Path) -> String {
    if cfg!(windows) {
        format!(
            "powershell -NoProfile -Command \"Add-Content -Path '{}' -Value 'invocation'; Copy-Item -Force '{}' '{}'\"",
            make_path(invocation_log),
            make_path(source),
            make_path(physics)
        )
    } else {
        format!(
            "echo invocation >> \"{}\" && cp \"{}\" \"{}\"",
            make_path(invocation_log),
            make_path(source),
            make_path(physics)
        )
    }
}

/// Enumerates the recipe shells this machine can exercise.
///
/// The environment-selected shell always runs. On Windows the test additionally
/// forces `cmd.exe` and, when Git for Windows or another POSIX shell is
/// reachable, that POSIX shell, so both hosted and local Make selections are
/// covered regardless of which one the current `PATH` yields. Make requires a
/// Windows-style absolute path for an explicit POSIX `SHELL`; a bare
/// `/usr/bin/bash` silently falls back to whatever `sh.exe` Make finds.
fn recipe_shells() -> Vec<RecipeShell> {
    let mut shells = vec![RecipeShell {
        label: "environment-default",
        executable: None,
    }];
    if cfg!(windows) {
        shells.push(RecipeShell {
            label: "cmd",
            executable: Some(PathBuf::from("cmd.exe")),
        });
        match posix_shell_on_windows() {
            Some(executable) => shells.push(RecipeShell {
                label: "posix",
                executable: Some(executable),
            }),
            None => eprintln!(
                "no POSIX shell found on this Windows machine; only the default and cmd.exe recipe shells are exercised"
            ),
        }
    }
    shells
}

fn posix_shell_on_windows() -> Option<PathBuf> {
    let path_entries = std::env::var_os("PATH")
        .map(|path| std::env::split_paths(&path).collect::<Vec<_>>())
        .unwrap_or_default();
    for entry in &path_entries {
        for name in ["bash.exe", "sh.exe"] {
            let candidate = entry.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    for variable in ["ProgramFiles", "ProgramW6432", "ProgramFiles(x86)"] {
        let Some(program_files) = std::env::var_os(variable) else {
            continue;
        };
        for relative in ["Git/usr/bin/bash.exe", "Git/bin/bash.exe"] {
            let candidate = Path::new(&program_files).join(relative);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
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
