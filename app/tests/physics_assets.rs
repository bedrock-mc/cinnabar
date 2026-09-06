use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::{Command, ExitStatus, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant, SystemTime},
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

/// Upper bound for one witness or install invocation of Make.
///
/// A candidate shell that hangs (an interactive launcher waiting for input,
/// for example) must fail the witness instead of stalling the suite.
const MAKE_INVOCATION_TIMEOUT: Duration = Duration::from_secs(120);

/// Upper bound for collecting Make's remaining output after it exited or was
/// killed. A recipe descendant that inherited the pipes can keep them open
/// after Make is gone; the collector must not wait on it.
const OUTPUT_DRAIN_TIMEOUT: Duration = Duration::from_secs(3);

/// Output line the witness recipe prints through the shell Make selected.
const WITNESS_SHELL_LINE_PREFIX: &str = "shell=";

/// Line the witness recipe prints only when a POSIX shell executed its
/// builtin-only conditional; `cmd.exe` cannot parse the construct at all.
const WITNESS_BUILTIN_LINE: &str = "posix-builtin-ok";

/// What a Make-level witness proved about the shell one `SHELL=` selection
/// actually executes recipes with.
#[derive(Debug, Clone, PartialEq, Eq)]
enum WitnessVerdict {
    /// A POSIX shell ran the recipe: `$0` expanded to the shell's own name
    /// and the builtin-only conditional executed.
    Posix { shell_name: String },
    /// The recipe ran with `cmd.exe` semantics (`$0` stayed literal) or the
    /// builtin-only conditional could not execute.
    NotPosix,
    /// Make could not complete the witness recipe at all.
    Failed { detail: String },
}

/// One recipe shell selection this test exercises against the Makefile.
#[derive(Debug, Clone, PartialEq, Eq)]
struct RecipeShell {
    label: &'static str,
    /// `SHELL=` override passed to Make; `None` keeps Make's own selection.
    executable: Option<PathBuf>,
    verdict: WitnessVerdict,
}

#[test]
fn make_physics_assets_installs_the_pinned_registry_once() {
    if !make_available() {
        eprintln!("skipping executable physics Makefile test: `make` is unavailable");
        return;
    }
    let root = workspace_root();
    let shells = exercised_recipe_shells(root);
    for shell in &shells {
        eprintln!(
            "exercising physics-assets install/hash/reuse under recipe shell `{}`{} ({:?})",
            shell.label,
            shell
                .executable
                .as_deref()
                .map(|path| format!(" ({})", path.display()))
                .unwrap_or_default(),
            shell.verdict,
        );
        install_pinned_registry_once(root, shell, "make-physics-recovery");
    }
}

/// PowerShell single-quoted literals do not escape an embedded apostrophe,
/// so both the production installer and the test recipe must double it. A
/// temporary directory with an apostrophe in its name exercises the exact
/// quoting under every reachable recipe shell.
#[test]
fn make_physics_assets_install_recipes_survive_apostrophes_in_paths() {
    if !make_available() {
        eprintln!("skipping executable physics Makefile quoting test: `make` is unavailable");
        return;
    }
    let root = workspace_root();
    for shell in &exercised_recipe_shells(root) {
        install_pinned_registry_once(root, shell, "make-physics-it's-quoted");
        if cfg!(windows) {
            production_installer_survives_apostrophes(root, shell);
        }
    }
}

/// The wall-clock bound on a Make invocation must hold even when a recipe
/// leaves a descendant behind that inherited Make's stdout/stderr pipes:
/// killing Make alone does not close those pipe ends, so an unbounded read
/// would block until the descendant exits.
#[test]
fn make_timeout_stays_bounded_when_recipe_descendants_hold_the_pipes() {
    if !make_available() {
        eprintln!("skipping bounded Make timeout test: `make` is unavailable");
        return;
    }
    let root = workspace_root();
    let shells = exercised_recipe_shells(root);
    let Some(posix) = shells
        .iter()
        .find(|shell| matches!(shell.verdict, WitnessVerdict::Posix { .. }))
    else {
        eprintln!("skipping bounded Make timeout test: no POSIX recipe shell is available");
        return;
    };
    let sleep = posix_sleep_command(posix);
    let temporary = temporary_directory("make-timeout-descendants");
    let makefile = temporary.join("hang.mk");
    // The background sleep inherits the pipes and outlives Make; the
    // foreground sleep keeps the recipe (and Make) alive past the timeout.
    // One shell-requiring line (`;` and `&`): Make executes metacharacter-free
    // lines directly, which needs an `echo` executable on `PATH` that a bare
    // Windows `PATH` does not provide.
    fs::write(
        &makefile,
        "hang:\n\t@echo hang-started; '$(SLEEP)' 8 & '$(SLEEP)' 8\n",
    )
    .unwrap();
    let mut assignments = vec![format!("SLEEP={sleep}")];
    assignments.extend(shell_assignment(posix));
    let started = Instant::now();
    let output = run_make_with_timeout(
        root,
        &["-f", &make_path(&makefile), "hang"],
        &assignments,
        Duration::from_secs(1),
    );
    let elapsed = started.elapsed();
    assert!(
        output.timed_out,
        "the hanging recipe must time out (exit {}):\nstdout:\n{}\nstderr:\n{}",
        output.status, output.stdout, output.stderr
    );
    assert!(
        elapsed < Duration::from_secs(7),
        "the invocation must return well before the 8-second descendant exits (took {elapsed:?})"
    );
    assert!(
        output.stdout.contains("hang-started"),
        "output produced before the timeout is still collected: {:?}",
        output.stdout
    );
    assert!(
        output.drain_timed_out,
        "the surviving descendants hold the pipes open, so the drain itself must have been bounded"
    );
    // Descendants may still hold files under the directory; cleanup is best
    // effort and never part of the bound being tested.
    let _ = fs::remove_dir_all(temporary);
}

/// The `sleep` executable reachable from one POSIX recipe shell.
///
/// Git for Windows ships `sleep.exe` beside its `bash.exe` but does not put
/// that directory on a non-login shell's `PATH`, so the absolute sibling path
/// is used there; elsewhere `sleep` is a standard utility.
fn posix_sleep_command(shell: &RecipeShell) -> String {
    let sibling = shell
        .executable
        .as_deref()
        .and_then(Path::parent)
        .map(|directory| directory.join("sleep.exe"))
        .filter(|candidate| candidate.is_file());
    sibling.map_or_else(|| "sleep".to_owned(), |path| make_path(&path))
}

/// The recipe shells this run exercises, after the witness has classified
/// each one, with the coverage rule applied: on Windows the POSIX leg is
/// mandatory whenever Git for Windows is installed, and the `cmd.exe` leg
/// must really carry `cmd.exe` semantics.
fn exercised_recipe_shells(root: &Path) -> Vec<RecipeShell> {
    let witness_dir = temporary_directory("make-shell-witness");
    let default_shell = RecipeShell {
        label: "environment-default",
        executable: None,
        verdict: witness_recipe_shell(root, &witness_dir, None),
    };
    let mut shells = vec![default_shell];
    if cfg!(windows) {
        let cmd = PathBuf::from("cmd.exe");
        let cmd_verdict = witness_recipe_shell(root, &witness_dir, Some(&cmd));
        assert_eq!(
            cmd_verdict,
            WitnessVerdict::NotPosix,
            "the cmd.exe leg must carry cmd.exe recipe semantics"
        );
        shells.push(RecipeShell {
            label: "cmd",
            executable: Some(cmd),
            verdict: cmd_verdict,
        });
        let mut rejected = Vec::new();
        let posix = posix_shell_candidates().into_iter().find_map(|candidate| {
            let verdict = witness_recipe_shell(root, &witness_dir, Some(&candidate));
            match verdict {
                WitnessVerdict::Posix { .. } => Some(RecipeShell {
                    label: "posix",
                    executable: Some(candidate),
                    verdict,
                }),
                other => {
                    rejected.push(format!("{}: {other:?}", candidate.display()));
                    None
                }
            }
        });
        let posix_covered =
            posix.is_some() || matches!(shells[0].verdict, WitnessVerdict::Posix { .. });
        match posix {
            Some(shell) => shells.push(shell),
            None if posix_covered => {}
            None => assert!(
                !git_for_windows_installed(),
                "Git for Windows is installed but no candidate shell passed the Make-level \
                 POSIX witness; the POSIX recipe leg must not silently degrade to a skip. \
                 Rejected candidates: {rejected:?}"
            ),
        }
        if !posix_covered {
            eprintln!(
                "no POSIX recipe shell is installed on this Windows machine; only the \
                 environment-default and cmd.exe legs are exercised (rejected: {rejected:?})"
            );
        }
    } else {
        assert!(
            matches!(shells[0].verdict, WitnessVerdict::Posix { .. }),
            "Make must run recipes through a POSIX shell here: {:?}",
            shells[0].verdict
        );
    }
    fs::remove_dir_all(witness_dir).unwrap();
    shells
}

/// Runs `make physics-assets` twice against a fresh pinned fixture and proves
/// that the first run installs exactly once, the hash check accepts the
/// installed bytes, and the second run reuses the installed registry.
fn install_pinned_registry_once(root: &Path, shell: &RecipeShell, label: &str) {
    let temporary = temporary_directory(&format!("{label}-{}", shell.label));
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
    assignments.extend(shell_assignment(shell));
    for expected_invocations in [1, 1] {
        let output = run_make(root, &["-f", "Makefile", "physics-assets"], &assignments);
        assert!(
            output.status.success() && !output.timed_out && !output.drain_timed_out,
            "make physics-assets failed under recipe shell `{}`:\nstdout:\n{}\nstderr:\n{}",
            shell.label,
            output.stdout,
            output.stderr
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

/// Runs the production Windows PowerShell installer (no recipe override)
/// against an apostrophe-bearing install path and proves it copies the
/// pinned bytes.
fn production_installer_survives_apostrophes(root: &Path, shell: &RecipeShell) {
    let temporary = temporary_directory(&format!("make-physics-it's-production-{}", shell.label));
    let physics = temporary.join("nested-it's").join("block-physics.bin");
    let expected_sha = temporary.join("block-physics.sha256");
    let source = temporary.join("checked-in-block-physics.bin");
    let manifest = temporary.join("bedrock-target.json");
    let pinned = b"protocol-2168-physics-production";
    fs::write(&source, pinned).unwrap();
    fs::write(&manifest, b"{}").unwrap();
    fs::write(&expected_sha, format!("{:x}\n", Sha256::digest(pinned))).unwrap();
    let mut assignments = vec![
        format!("PHYSICS_REGISTRY={}", make_path(&physics)),
        format!("PHYSICS_REGISTRY_SOURCE={}", make_path(&source)),
        format!("PHYSICS_REGISTRY_SHA256={}", make_path(&expected_sha)),
        format!("BEDROCK_TARGET_MANIFEST={}", make_path(&manifest)),
    ];
    assignments.extend(shell_assignment(shell));
    let output = run_make(root, &["-f", "Makefile", "physics-assets"], &assignments);
    assert!(
        output.status.success() && !output.timed_out && !output.drain_timed_out,
        "production physics installer failed on an apostrophe path under recipe shell `{}`:\nstdout:\n{}\nstderr:\n{}",
        shell.label,
        output.stdout,
        output.stderr
    );
    assert_eq!(fs::read(&physics).unwrap(), pinned);
    fs::remove_dir_all(temporary).unwrap();
}

fn shell_assignment(shell: &RecipeShell) -> Option<String> {
    shell
        .executable
        .as_deref()
        .map(|executable| format!("SHELL={}", make_path(executable)))
}

/// Builds the overriding install recipe for this test.
///
/// On Windows the recipe must not depend on the recipe shell: `copy` exists
/// only inside `cmd.exe` and `cp` only inside a POSIX shell, but the hosted and
/// local Make installations select different shells. Windows PowerShell is an
/// ordinary executable reachable from both, so the recipe uses it the same way
/// the production `PHYSICS_REGISTRY_INSTALL` does. Forward-slash paths in
/// PowerShell single-quoted literals stay literal under `cmd.exe`, `sh.exe`,
/// and PowerShell alike once embedded apostrophes are doubled.
fn install_recipe(invocation_log: &Path, source: &Path, physics: &Path) -> String {
    if cfg!(windows) {
        format!(
            "powershell -NoProfile -Command \"Add-Content -Path {} -Value 'invocation'; Copy-Item -Force {} {}\"",
            powershell_single_quoted(&make_path(invocation_log)),
            powershell_single_quoted(&make_path(source)),
            powershell_single_quoted(&make_path(physics))
        )
    } else {
        format!(
            "echo invocation >> {} && cp {} {}",
            posix_single_quoted(&make_path(invocation_log)),
            posix_single_quoted(&make_path(source)),
            posix_single_quoted(&make_path(physics))
        )
    }
}

/// A PowerShell single-quoted literal: the only escape is a doubled apostrophe.
fn powershell_single_quoted(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

/// A POSIX single-quoted literal: an apostrophe closes the literal, is
/// escaped, and reopens it.
fn posix_single_quoted(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

/// Classifies the shell Make actually executes recipes with for one `SHELL=`
/// selection by running a witness Makefile through that selection.
///
/// The witness prints `shell=$0 bash=$BASH_VERSION` and then runs a
/// builtin-only POSIX conditional. GNU Make on Windows may silently fall back
/// to a different shell than the one named (or to `cmd.exe` semantics) when
/// the named executable is not a usable POSIX shell, so only the observed
/// output decides; the existence or exit status of the candidate executable
/// proves nothing.
fn witness_recipe_shell(root: &Path, witness_dir: &Path, shell: Option<&Path>) -> WitnessVerdict {
    let makefile = witness_dir.join("shell-witness.mk");
    // The conditional line is error-tolerant (`-`) so a shell that cannot
    // parse it, like cmd.exe, still leaves the first line's evidence and a
    // clean exit for classification instead of an aborted recipe.
    fs::write(
        &makefile,
        "witness:\n\t@echo shell=$$0 bash=$$BASH_VERSION\n\t-@if [ -n \"$$0\" ]; then echo posix-builtin-ok; fi\n",
    )
    .unwrap();
    let mut assignments = Vec::new();
    if let Some(shell) = shell {
        assignments.push(format!("SHELL={}", make_path(shell)));
    }
    let output = run_make(
        root,
        &["-f", &make_path(&makefile), "witness"],
        &assignments,
    );
    classify_witness(&output)
}

/// Classifies a complete witness result: the bounded timeout, the exit
/// status, and the printed evidence together.
fn classify_witness(output: &MakeOutput) -> WitnessVerdict {
    if output.timed_out {
        return WitnessVerdict::Failed {
            detail: "witness recipe exceeded its bounded timeout".to_owned(),
        };
    }
    match classify_witness_output(&output.stdout) {
        // A literal `$0` is positive evidence of cmd.exe semantics even when
        // the tolerant conditional line still made Make report an error.
        WitnessVerdict::NotPosix => WitnessVerdict::NotPosix,
        WitnessVerdict::Posix { .. } if !output.status.success() => WitnessVerdict::Failed {
            detail: format!(
                "witness recipe expanded like a POSIX shell but exited with {}:\nstdout:\n{}\nstderr:\n{}",
                output.status, output.stdout, output.stderr
            ),
        },
        WitnessVerdict::Failed { .. } => WitnessVerdict::Failed {
            detail: format!(
                "witness recipe printed no shell evidence (exit {}):\nstdout:\n{}\nstderr:\n{}",
                output.status, output.stdout, output.stderr
            ),
        },
        verdict => verdict,
    }
}

/// Pure classification of the witness recipe's stdout.
///
/// A missing `shell=` line means the recipe never reached the shell, which is
/// neither POSIX nor cmd.exe evidence; a literal `$0` is cmd.exe semantics; an
/// expanded name counts as POSIX only when the builtin conditional also ran.
fn classify_witness_output(stdout: &str) -> WitnessVerdict {
    let Some(shell_name) = stdout
        .lines()
        .map(str::trim)
        .find_map(|line| line.strip_prefix(WITNESS_SHELL_LINE_PREFIX))
        .and_then(|rest| rest.split(" bash=").next())
        .map(str::trim)
    else {
        return WitnessVerdict::Failed {
            detail: "no shell witness line".to_owned(),
        };
    };
    let builtin_ran = stdout
        .lines()
        .any(|line| line.trim() == WITNESS_BUILTIN_LINE);
    if shell_name.is_empty() || shell_name == "$0" || !builtin_ran {
        return WitnessVerdict::NotPosix;
    }
    WitnessVerdict::Posix {
        shell_name: shell_name.to_owned(),
    }
}

/// Candidate POSIX shells on Windows, most trustworthy first: the Git for
/// Windows install locations, then whatever `PATH` resolves. Every candidate
/// still has to pass the Make-level witness; the WSL launcher that ships as
/// `System32\bash.exe`, for example, is discovered on `PATH` yet cannot run
/// Make recipes.
fn posix_shell_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    for variable in ["ProgramFiles", "ProgramW6432", "ProgramFiles(x86)"] {
        let Some(program_files) = std::env::var_os(variable) else {
            continue;
        };
        for relative in ["Git/usr/bin/bash.exe", "Git/bin/bash.exe"] {
            candidates.push(Path::new(&program_files).join(relative));
        }
    }
    if let Some(path) = std::env::var_os("PATH") {
        for entry in std::env::split_paths(&path) {
            for name in ["bash.exe", "sh.exe"] {
                candidates.push(entry.join(name));
            }
        }
    }
    let mut seen = Vec::new();
    candidates
        .into_iter()
        .filter(|candidate| candidate.is_file())
        .filter(|candidate| {
            let key = candidate.to_string_lossy().to_ascii_lowercase();
            if seen.contains(&key) {
                false
            } else {
                seen.push(key);
                true
            }
        })
        .collect()
}

fn git_for_windows_installed() -> bool {
    ["ProgramFiles", "ProgramW6432", "ProgramFiles(x86)"]
        .into_iter()
        .filter_map(std::env::var_os)
        .any(|program_files| {
            Path::new(&program_files)
                .join("Git/usr/bin/bash.exe")
                .is_file()
        })
}

/// Captured result of one bounded Make invocation.
struct MakeOutput {
    status: ExitStatus,
    stdout: String,
    stderr: String,
    /// Make itself exceeded the wall-clock bound and was killed.
    timed_out: bool,
    /// The pipes stayed open past the drain bound after Make ended, so the
    /// captured output may be incomplete.
    drain_timed_out: bool,
}

fn run_make(root: &Path, arguments: &[&str], assignments: &[String]) -> MakeOutput {
    run_make_with_timeout(root, arguments, assignments, MAKE_INVOCATION_TIMEOUT)
}

/// Runs Make with a bounded wall-clock timeout, killing it (and reporting the
/// timeout) instead of letting an unusable recipe shell stall the suite.
///
/// Output is collected on detached reader threads that hand their buffers
/// back through channels: the caller waits at most [`OUTPUT_DRAIN_TIMEOUT`]
/// for them, so a recipe descendant that inherited the pipes and outlives
/// Make cannot turn the bounded invocation into an unbounded join.
fn run_make_with_timeout(
    root: &Path,
    arguments: &[&str],
    assignments: &[String],
    timeout: Duration,
) -> MakeOutput {
    let mut child = Command::new("make")
        .current_dir(root)
        .args(arguments)
        .args(assignments)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let stdout_pipe = child.stdout.take().unwrap();
    let stderr_pipe = child.stderr.take().unwrap();
    let stdout_rx = collect_detached(stdout_pipe);
    let stderr_rx = collect_detached(stderr_pipe);
    let started = Instant::now();
    let mut timed_out = false;
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if started.elapsed() > timeout {
            timed_out = true;
            let _ = child.kill();
            break child.wait().unwrap();
        }
        thread::sleep(Duration::from_millis(25));
    };
    let drain_deadline = Instant::now() + OUTPUT_DRAIN_TIMEOUT;
    let (stdout, stdout_drained) = drain_until(&stdout_rx, drain_deadline);
    let (stderr, stderr_drained) = drain_until(&stderr_rx, drain_deadline);
    MakeOutput {
        status,
        stdout,
        stderr,
        timed_out,
        drain_timed_out: !(stdout_drained && stderr_drained),
    }
}

/// Reads one pipe to its end on a detached thread, streaming chunks so that
/// output produced before a timeout is available even when the pipe never
/// reaches end-of-file.
fn collect_detached(mut pipe: impl Read + Send + 'static) -> mpsc::Receiver<Vec<u8>> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut chunk = [0_u8; 4096];
        loop {
            match pipe.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(read) => {
                    if tx.send(chunk[..read].to_vec()).is_err() {
                        break;
                    }
                }
            }
        }
    });
    rx
}

/// Collects streamed chunks until the reader finishes or the deadline passes.
/// Returns the text and whether the pipe reached end-of-file in time.
fn drain_until(rx: &mpsc::Receiver<Vec<u8>>, deadline: Instant) -> (String, bool) {
    let mut bytes = Vec::new();
    let drained = loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        match rx.recv_timeout(remaining) {
            Ok(chunk) => bytes.extend_from_slice(&chunk),
            Err(mpsc::RecvTimeoutError::Disconnected) => break true,
            Err(mpsc::RecvTimeoutError::Timeout) => break false,
        }
    };
    (String::from_utf8_lossy(&bytes).into_owned(), drained)
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

/// A synthetic exit status carrying one process exit code.
fn exit_status(code: i32) -> ExitStatus {
    #[cfg(windows)]
    {
        use std::os::windows::process::ExitStatusExt;
        ExitStatus::from_raw(code as u32)
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        ExitStatus::from_raw(code << 8)
    }
}

fn synthetic_output(code: i32, stdout: &str, stderr: &str, timed_out: bool) -> MakeOutput {
    MakeOutput {
        status: exit_status(code),
        stdout: stdout.to_owned(),
        stderr: stderr.to_owned(),
        timed_out,
        drain_timed_out: false,
    }
}

#[test]
fn witness_output_classification_separates_posix_from_cmd_semantics() {
    assert_eq!(
        classify_witness_output("shell=$0 bash=$BASH_VERSION\n"),
        WitnessVerdict::NotPosix,
        "cmd.exe leaves both expansions literal"
    );
    assert_eq!(
        classify_witness_output("shell=/usr/bin/bash bash=5.2.37(1)-release\nposix-builtin-ok\n"),
        WitnessVerdict::Posix {
            shell_name: "/usr/bin/bash".to_owned()
        }
    );
    assert_eq!(
        classify_witness_output("shell=/bin/sh bash=\nposix-builtin-ok\n"),
        WitnessVerdict::Posix {
            shell_name: "/bin/sh".to_owned()
        },
        "a non-bash POSIX shell has no BASH_VERSION and still counts"
    );
    assert_eq!(
        classify_witness_output("shell=/usr/bin/sh bash=5.2.37(1)-release\n"),
        WitnessVerdict::NotPosix,
        "an expanded name without the builtin conditional is not proven POSIX"
    );
    assert!(matches!(
        classify_witness_output(""),
        WitnessVerdict::Failed { .. }
    ));
}

/// The full-result classification: the observed cmd.exe signature counts as
/// cmd.exe even when the POSIX-only line made Make exit non-zero, while an
/// exit failure, a timeout, or missing evidence never masquerades as either
/// shell family.
#[test]
fn witness_classification_uses_status_and_timeout_with_the_printed_evidence() {
    assert_eq!(
        classify_witness(&synthetic_output(
            2,
            "shell=$0 bash=$BASH_VERSION\r\n",
            "-n was unexpected at this time.\r\n",
            false
        )),
        WitnessVerdict::NotPosix,
        "the recorded cmd.exe failure of the POSIX-only line is cmd.exe semantics"
    );
    assert_eq!(
        classify_witness(&synthetic_output(
            0,
            "shell=/usr/bin/bash bash=5.2.37(1)-release\nposix-builtin-ok\n",
            "",
            false
        )),
        WitnessVerdict::Posix {
            shell_name: "/usr/bin/bash".to_owned()
        }
    );
    assert!(matches!(
        classify_witness(&synthetic_output(
            1,
            "shell=/usr/bin/bash bash=5.2.37(1)-release\nposix-builtin-ok\n",
            "",
            false
        )),
        WitnessVerdict::Failed { .. }
    ));
    assert!(matches!(
        classify_witness(&synthetic_output(0, "", "", false)),
        WitnessVerdict::Failed { .. }
    ));
    assert!(matches!(
        classify_witness(&synthetic_output(
            0,
            "shell=/usr/bin/bash bash=5.2.37(1)-release\nposix-builtin-ok\n",
            "",
            true
        )),
        WitnessVerdict::Failed { .. }
    ));
}

#[test]
fn quoted_literals_double_or_escape_embedded_apostrophes() {
    assert_eq!(
        powershell_single_quoted("C:/it's-here/x"),
        "'C:/it''s-here/x'"
    );
    assert_eq!(
        posix_single_quoted("/tmp/it's-here/x"),
        "'/tmp/it'\\''s-here/x'"
    );
    assert_eq!(powershell_single_quoted("plain"), "'plain'");
}
