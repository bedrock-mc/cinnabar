//! Conservative worktree inventory and cleanup; branch refs are never deleted.
use crate::DevtoolError;
use serde::Deserialize;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Options {
    pub apply: bool,
    pub force: Option<PathBuf>,
}

#[derive(Debug)]
struct Worktree {
    path: PathBuf,
    branch: Option<String>,
    protected: bool,
    unavailable: bool,
}
#[derive(Debug, Deserialize)]
struct PullRequest {
    state: String,
    #[serde(rename = "headRefOid", default)]
    head: Option<String>,
}
#[derive(Debug, PartialEq, Eq)]
enum Processes {
    Idle,
    Busy,
    Unknown,
}
#[derive(Debug)]
struct State {
    dirty: bool,
    unpushed: bool,
    pr: Option<String>,
    pushed_age: Option<u64>,
    age: Option<u64>,
    processes: Processes,
}

/// Accepts only explicit cleanup flags; a forced path still requires --apply.
pub(crate) fn parse(args: &[String]) -> Result<Options, DevtoolError> {
    let mut options = Options::default();
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--apply" => options.apply = true,
            "--force" => {
                options.force = Some(
                    args.next()
                        .ok_or_else(|| {
                            DevtoolError::Usage("--force requires a worktree path".into())
                        })?
                        .into(),
                )
            }
            _ => {
                return Err(DevtoolError::Usage(format!(
                    "unknown wt-gc argument: {arg}"
                )));
            }
        }
    }
    Ok(options)
}

/// Inventories every worktree, then independently rechecks each candidate before removal.
pub(crate) fn run(options: &Options) -> Result<(), DevtoolError> {
    let root = PathBuf::from(git(Path::new("."), &["rev-parse", "--show-toplevel"])?.trim());
    let worktrees = parse_worktrees(&git(&root, &["worktree", "list", "--porcelain", "-z"])?);
    let forced = options
        .force
        .as_ref()
        .map(fs::canonicalize)
        .transpose()
        .map_err(|source| DevtoolError::Spawn {
            command: "resolve forced worktree".into(),
            source,
        })?;
    if forced.as_ref().is_some_and(|path| {
        !worktrees
            .iter()
            .any(|wt| fs::canonicalize(&wt.path).is_ok_and(|candidate| candidate == *path))
    }) {
        return Err(DevtoolError::Usage(
            "--force must name a registered worktree".into(),
        ));
    }
    let mut plans = Vec::new();
    for (index, wt) in worktrees.into_iter().enumerate() {
        let state = inspect(&wt)?;
        let force = forced.as_ref().is_some_and(|path| {
            fs::canonicalize(&wt.path).is_ok_and(|candidate| candidate == *path)
        });
        let protected = index == 0
            || wt.protected
            || fs::canonicalize(&wt.path).ok() == fs::canonicalize(&root).ok();
        let remove = !protected && eligible(&state, force);
        println!(
            "{} branch={} PR={} availability={} changes={} unpushed={} processes={:?} age_days={} pushed_days={} action={}",
            wt.path.display(),
            wt.branch.as_deref().unwrap_or("detached"),
            state.pr.as_deref().unwrap_or("unknown"),
            if wt.unavailable || !wt.path.is_dir() {
                "unavailable"
            } else {
                "available"
            },
            state.dirty,
            state.unpushed,
            state.processes,
            days(state.age),
            days(state.pushed_age),
            if remove { "remove" } else { "keep" }
        );
        plans.push((wt, remove, force, protected));
    }
    if !options.apply {
        println!("dry run; use --apply to clean eligible worktrees");
        return Ok(());
    }
    for (wt, remove, force, protected) in plans {
        let state = inspect(&wt)?;
        if protected {
            continue;
        }
        if remove && eligible(&state, force) {
            // The final process check is separate from both inventory and deletion.
            if processes(&wt.path) != Processes::Idle {
                continue;
            }
            let mut command = Command::new("git");
            command.current_dir(&root).args(["worktree", "remove"]);
            if force {
                command.arg("--force");
            }
            let status = command
                .arg(&wt.path)
                .status()
                .map_err(|source| DevtoolError::Spawn {
                    command: "git worktree remove".into(),
                    source,
                })?;
            if !status.success() {
                return Err(DevtoolError::Usage(format!(
                    "worktree removal failed: {}",
                    wt.path.display()
                )));
            }
        } else if state.pr.as_deref() == Some("MERGED")
            && !state.dirty
            && !state.unpushed
            && processes(&wt.path) == Processes::Idle
        {
            let target = wt.path.join("target");
            // Never follow a target symlink into another worktree or a shared cache.
            if fs::symlink_metadata(&target)
                .is_ok_and(|meta| meta.is_dir() && !meta.file_type().is_symlink())
            {
                fs::remove_dir_all(&target).map_err(|source| DevtoolError::Spawn {
                    command: "remove merged worktree target".into(),
                    source,
                })?;
                println!("removed {}", target.display());
            }
        }
    }
    Ok(())
}

/// Allows force to override changes and unpublished commits, never process or protection checks.
fn eligible(state: &State, force: bool) -> bool {
    state.processes == Processes::Idle
        && (force
            || (!state.dirty
                && !state.unpushed
                && (matches!(state.pr.as_deref(), Some("MERGED" | "CLOSED"))
                    || state.pushed_age.is_some_and(|age| age > 7 * 86400))))
}

/// Reports an unavailable timestamp distinctly from a new worktree.
fn days(age: Option<u64>) -> String {
    age.map_or_else(|| "unknown".into(), |age| (age / 86400).to_string())
}

/// Parses NUL-delimited porcelain records, including paths containing spaces or newlines.
fn parse_worktrees(text: &str) -> Vec<Worktree> {
    let mut worktrees = Vec::new();
    for record in text.split("\0\0") {
        let mut path = None;
        let mut branch = None;
        let mut protected = false;
        let mut unavailable = false;
        for field in record.split('\0') {
            if let Some(value) = field.strip_prefix("worktree ") {
                path = Some(PathBuf::from(value));
            }
            if let Some(value) = field.strip_prefix("branch refs/heads/") {
                branch = Some(value.into());
            }
            if field.starts_with("locked") || field == "bare" || field.starts_with("prunable") {
                protected = true;
            }
            if field == "bare" || field.starts_with("prunable") {
                unavailable = true;
            }
        }
        if let Some(path) = path {
            worktrees.push(Worktree {
                path,
                branch,
                protected,
                unavailable,
            });
        }
    }
    worktrees
}

/// Reads fresh Git and GitHub state; failures prevent automatic deletion.
fn inspect(wt: &Worktree) -> Result<State, DevtoolError> {
    if wt.unavailable || !wt.path.is_dir() {
        return Ok(State {
            dirty: true,
            unpushed: true,
            pr: None,
            pushed_age: None,
            age: None,
            processes: Processes::Unknown,
        });
    }
    let dirty = !git(
        &wt.path,
        &["status", "--porcelain", "--untracked-files=all"],
    )?
    .is_empty();
    let revision = wt.branch.as_deref().unwrap_or("HEAD");
    let request = pull_request(wt);
    let mut revisions = vec!["log", revision, "--not", "--remotes"];
    if let Some(head) = request
        .as_ref()
        .filter(|pr| pr.state == "MERGED")
        .and_then(|pr| pr.head.as_deref())
        .filter(|head| head.len() == 40 && head.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .filter(|head| {
            git(
                &wt.path,
                &["rev-parse", "--verify", &format!("{head}^{{commit}}")],
            )
            .is_ok()
        })
    {
        revisions.push(head);
    }
    revisions.push("--format=%H");
    let unpushed = !git(&wt.path, &revisions)?.is_empty();
    let pr = request.map(|pr| pr.state);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let age = fs::metadata(&wt.path)
        .and_then(|meta| meta.created())
        .or_else(|_| fs::metadata(wt.path.join(".git")).and_then(|meta| meta.modified()))
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|time| now.saturating_sub(time.as_secs()));
    // A remote commit date does not establish when it was pushed. Require a local push reflog.
    let pushed_age = wt.branch.as_ref().and_then(|branch| {
        let remote = git(
            &wt.path,
            &[
                "rev-parse",
                "--symbolic-full-name",
                &format!("{branch}@{{upstream}}"),
            ],
        )
        .ok()?;
        let entries = git(
            &wt.path,
            &[
                "reflog",
                "show",
                "--date=unix",
                "--format=%gD%x09%gs",
                remote.trim(),
            ],
        )
        .ok()?;
        last_push_age(&entries, now)
    });
    Ok(State {
        dirty,
        unpushed,
        pr,
        pushed_age,
        age,
        processes: processes(&wt.path),
    })
}

/// Keeps open PRs protected and reads the published head even after a squash deletes its branch.
fn pull_request(wt: &Worktree) -> Option<PullRequest> {
    let branch = wt.branch.as_ref()?;
    let output = Command::new("gh")
        .current_dir(&wt.path)
        .args([
            "pr",
            "list",
            "--state",
            "all",
            "--head",
            branch,
            "--json",
            "state,headRefOid",
            "--limit",
            "100",
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let requests: Vec<PullRequest> = serde_json::from_slice(&output.stdout).ok()?;
    let chosen = requests
        .iter()
        .position(|pr| pr.state == "OPEN")
        .unwrap_or(0);
    requests.into_iter().nth(chosen)
}

/// Uses the reflog event time, since the pushed commit itself may be years old.
fn last_push_age(entries: &str, now: u64) -> Option<u64> {
    let (selector, event) = entries.lines().next()?.split_once('\t')?;
    if !event.contains("update by push") {
        return None;
    }
    let timestamp = selector
        .rsplit_once("@{")?
        .1
        .strip_suffix('}')?
        .parse::<u64>()
        .ok()?;
    Some(now.saturating_sub(timestamp))
}

/// Bounds recursive lsof to two seconds; missing or timed-out checks never mean idle.
fn processes(path: &Path) -> Processes {
    let Ok(file) = tempfile::tempfile() else {
        return Processes::Unknown;
    };
    let Ok(stdout) = file.try_clone() else {
        return Processes::Unknown;
    };
    let Ok(errors) = tempfile::tempfile() else {
        return Processes::Unknown;
    };
    let Ok(stderr) = errors.try_clone() else {
        return Processes::Unknown;
    };
    let Ok(mut child) = Command::new("lsof")
        .args(["-t", "+D"])
        .arg(path)
        .stdout(stdout)
        .stderr(stderr)
        .spawn()
    else {
        return Processes::Unknown;
    };
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let bytes = file.metadata().map_or(0, |meta| meta.len());
                return if bytes > 0 {
                    Processes::Busy
                } else if status.code() == Some(1)
                    && errors.metadata().is_ok_and(|meta| meta.len() == 0)
                {
                    Processes::Idle
                } else {
                    Processes::Unknown
                };
            }
            Ok(None) if start.elapsed() < Duration::from_secs(2) => {
                thread::sleep(Duration::from_millis(20))
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Processes::Unknown;
            }
        }
    }
}

/// Runs read-only Git commands and preserves failures instead of interpreting them as empty state.
fn git(root: &Path, args: &[&str]) -> Result<String, DevtoolError> {
    let output = Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .map_err(|source| DevtoolError::Spawn {
            command: format!("git {}", args.join(" ")),
            source,
        })?;
    if !output.status.success() {
        return Err(DevtoolError::Command {
            command: format!("git {}", args.join(" ")),
            status: output.status.to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).into(),
        });
    }
    String::from_utf8(output.stdout).map_err(|_| DevtoolError::NonUtf8 {
        command: "git".into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn push_age_uses_the_push_event_and_does_not_infer_age_from_a_fetch() {
        assert_eq!(
            last_push_age("refs/remotes/origin/topic@{100}\tupdate by push\n", 150),
            Some(50)
        );
        assert_eq!(
            last_push_age("refs/remotes/origin/topic@{100}\tfetch origin\n", 150),
            None
        );
        assert_eq!(last_push_age("", 150), None);
    }

    #[test]
    fn cleanup_requires_all_safety_conditions() {
        let mut state = State {
            dirty: false,
            unpushed: false,
            pr: Some("MERGED".into()),
            pushed_age: None,
            age: None,
            processes: Processes::Idle,
        };
        assert!(eligible(&state, false));
        state.dirty = true;
        assert!(!eligible(&state, false));
        assert!(eligible(&state, true));
        state.unpushed = true;
        assert!(!eligible(&state, false));
        state.processes = Processes::Unknown;
        assert!(!eligible(&state, true));
        state.processes = Processes::Busy;
        assert!(!eligible(&state, true));
        state.processes = Processes::Idle;
        state.dirty = false;
        state.unpushed = false;
        state.pr = Some("OPEN".into());
        assert!(!eligible(&state, false));
        state.pushed_age = Some(8 * 86400);
        assert!(eligible(&state, false));
    }
    #[test]
    fn temp_repo_inventory_preserves_paths_and_locked_worktrees() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("repo");
        fs::create_dir(&root).unwrap();
        let run = |args: &[&str]| {
            assert!(
                Command::new("git")
                    .current_dir(&root)
                    .args(args)
                    .status()
                    .unwrap()
                    .success()
            );
        };
        run(&["init", "--quiet"]);
        run(&[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "commit",
            "--quiet",
            "--allow-empty",
            "-m",
            "fixture",
        ]);
        let other = temp.path().join("space and\nnewline");
        run(&[
            "worktree",
            "add",
            "--quiet",
            "--detach",
            other.to_str().unwrap(),
        ]);
        run(&["worktree", "lock", other.to_str().unwrap()]);
        let trees =
            parse_worktrees(&git(&root, &["worktree", "list", "--porcelain", "-z"]).unwrap());
        assert_eq!(trees.len(), 2);
        assert_eq!(trees[1].path, fs::canonicalize(other).unwrap());
        assert!(trees[1].protected);
        assert!(
            !git(
                &trees[1].path,
                &["log", "HEAD", "--not", "--remotes", "--format=%H"]
            )
            .unwrap()
            .is_empty()
        );
        fs::write(trees[1].path.join("new"), "change").unwrap();
        assert!(
            !git(&trees[1].path, &["status", "--porcelain"])
                .unwrap()
                .is_empty()
        );
    }
}
