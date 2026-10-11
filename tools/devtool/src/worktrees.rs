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
    branch: Option<String>,
    head: String,
    dirty: bool,
    unpushed: bool,
    pr: Option<String>,
    pr_unknown: bool, // The PR lookup failed, so an open PR cannot be ruled out.
    /// A merged or closed PR's head contains HEAD; a reused branch with newer commits is not retired.
    retired: bool,
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
    let nested: Vec<_> = worktrees
        .iter()
        .map(|wt| contains_worktree(&wt.path, &worktrees))
        .collect();
    let mut plans = Vec::new();
    for (index, (wt, nested)) in worktrees.into_iter().zip(nested).enumerate() {
        let state = inspect(&wt)?;
        let force = forced.as_ref().is_some_and(|path| {
            fs::canonicalize(&wt.path).is_ok_and(|candidate| candidate == *path)
        });
        let protected = index == 0
            || wt.protected
            || nested
            || fs::canonicalize(&wt.path).ok() == fs::canonicalize(&root).ok();
        let remove = !protected && eligible(&state, force);
        println!(
            "{} branch={} PR={} availability={} changes={} unpushed={} processes={:?} age_days={} pushed_days={} action={}",
            wt.path.display(),
            state.branch.as_deref().unwrap_or("detached"),
            if state.pr_unknown {
                "unknown"
            } else {
                state.pr.as_deref().unwrap_or("none")
            },
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
        if protected {
            continue;
        }
        let state = inspect(&wt)?;
        let remove = remove && eligible(&state, force);
        let clean_target = !remove
            && state.retired
            && state.pr.as_deref() == Some("MERGED")
            && !state.dirty
            && !state.unpushed;
        // The final process check is separate from inventory and deletion; skip the tree if its
        // checkout moved or another worktree was nested inside it meanwhile.
        if !(remove || clean_target)
            || processes(&wt.path) != Processes::Idle
            || checkout(&wt.path).ok() != Some((state.branch.clone(), state.head.clone()))
            || contains_worktree(
                &wt.path,
                &parse_worktrees(&git(&root, &["worktree", "list", "--porcelain", "-z"])?),
            )
        {
            continue;
        }
        if remove {
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
        } else {
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

/// Allows force to override changes, unpublished commits, and open or unknown PRs, never process
/// or protection checks.
fn eligible(state: &State, force: bool) -> bool {
    state.processes == Processes::Idle
        && (force
            || (!state.dirty
                && !state.unpushed
                && !state.pr_unknown
                && state.pr.as_deref() != Some("OPEN")
                && (state.retired || state.pushed_age.is_some_and(|age| age > 7 * 86400))))
}

/// Reports an unavailable timestamp distinctly from a new worktree.
fn days(age: Option<u64>) -> String {
    age.map_or_else(|| "unknown".into(), |age| (age / 86400).to_string())
}

/// Whether another registered worktree lives inside `path`, so removing it would delete that tree.
fn contains_worktree(path: &Path, worktrees: &[Worktree]) -> bool {
    let canonical = |path: &Path| fs::canonicalize(path).unwrap_or_else(|_| path.to_owned());
    let parent = canonical(path);
    worktrees.iter().any(|other| {
        let child = canonical(&other.path);
        child != parent && child.starts_with(&parent)
    })
}

/// Reads the checked-out branch, `None` when detached, and the HEAD commit.
fn checkout(path: &Path) -> Result<(Option<String>, String), DevtoolError> {
    let current = git(path, &["rev-parse", "--abbrev-ref", "HEAD"])?;
    let branch = (current.trim() != "HEAD").then(|| current.trim().to_owned());
    Ok((branch, git(path, &["rev-parse", "HEAD"])?.trim().to_owned()))
}

/// Parses NUL-delimited porcelain records, including paths containing spaces or newlines.
fn parse_worktrees(text: &str) -> Vec<Worktree> {
    let mut worktrees = Vec::new();
    for record in text.split("\0\0") {
        let mut path = None;
        let mut protected = false;
        let mut unavailable = false;
        for field in record.split('\0') {
            if let Some(value) = field.strip_prefix("worktree ") {
                path = Some(PathBuf::from(value));
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
            branch: None,
            head: String::new(),
            dirty: true,
            unpushed: true,
            pr: None,
            pr_unknown: true,
            retired: false,
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
    let (branch, head) = checkout(&wt.path)?;
    let lookup = pull_request(&wt.path, branch.as_deref());
    let pr_unknown = lookup.is_none();
    let request = lookup.flatten();
    let pr_head = request
        .as_ref()
        .and_then(|pr| pr.head.as_deref())
        .filter(|head| head.len() == 40 && head.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .filter(|head| {
            git(
                &wt.path,
                &["rev-parse", "--verify", &format!("{head}^{{commit}}")],
            )
            .is_ok()
        });
    let state = request.as_ref().map(|pr| pr.state.as_str());
    let mut revisions = vec!["log", head.as_str(), "--not", "--remotes"];
    if let Some(pr_head) = pr_head.filter(|_| state == Some("MERGED")) {
        revisions.push(pr_head);
    }
    revisions.push("--format=%H");
    let unpushed = !git(&wt.path, &revisions)?.is_empty();
    let retired = matches!(state, Some("MERGED" | "CLOSED"))
        && pr_head.is_some_and(|pr_head| {
            git(&wt.path, &["merge-base", "--is-ancestor", &head, pr_head]).is_ok()
        });
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
    // A remote commit date does not establish when it was pushed. Require a local push reflog
    // of this branch's own remote ref, not its upstream, that recorded HEAD.
    let pushed_age = branch.as_ref().and_then(|branch| {
        git(&wt.path, &["remote"])
            .ok()?
            .lines()
            .filter_map(|remote| {
                let entries = git(
                    &wt.path,
                    &[
                        "reflog",
                        "show",
                        "--date=unix",
                        "--format=%gD%x09%H%x09%gs",
                        &format!("refs/remotes/{remote}/{branch}"),
                    ],
                )
                .ok()?;
                last_push_age(&entries, &head, now)
            })
            .min()
    });
    Ok(State {
        branch,
        head,
        dirty,
        unpushed,
        pr,
        pr_unknown,
        retired,
        pushed_age,
        age,
        processes: processes(&wt.path),
    })
}

/// Returns the open or newest PR for `branch`, or `None` when a lookup fails; reads the published
/// head even after a squash deletes its branch.
fn pull_request(path: &Path, branch: Option<&str>) -> Option<Option<PullRequest>> {
    let Some(branch) = branch else {
        return Some(None);
    };
    // Open PRs are queried alone: a reused branch name can have more history than one page.
    if let Some(open) = pull_requests(path, branch, "open")?
        .into_iter()
        .find(|pr| pr.state == "OPEN")
    {
        return Some(Some(open));
    }
    Some(pull_requests(path, branch, "all")?.into_iter().next())
}

/// Lists PRs with head `branch` in `state`, newest first; `None` when `gh` fails.
fn pull_requests(path: &Path, branch: &str, state: &str) -> Option<Vec<PullRequest>> {
    let output = Command::new("gh")
        .current_dir(path)
        .args([
            "pr",
            "list",
            "--state",
            state,
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
    serde_json::from_slice(&output.stdout).ok()
}

/// Uses the reflog event time, since the pushed commit itself may be years old; the latest entry
/// must be a push of `head`.
fn last_push_age(entries: &str, head: &str, now: u64) -> Option<u64> {
    let mut fields = entries.lines().next()?.splitn(3, '\t');
    let (selector, pushed, event) = (fields.next()?, fields.next()?, fields.next()?);
    if pushed != head || !event.contains("update by push") {
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
        let head = "a".repeat(40);
        let entry =
            |oid: &str, event: &str| format!("refs/remotes/origin/topic@{{100}}\t{oid}\t{event}\n");
        assert_eq!(
            last_push_age(&entry(&head, "update by push"), &head, 150),
            Some(50)
        );
        assert_eq!(
            last_push_age(&entry(&head, "fetch origin"), &head, 150),
            None
        );
        assert_eq!(
            last_push_age(&entry(&"b".repeat(40), "update by push"), &head, 150),
            None
        );
        assert_eq!(last_push_age("", &head, 150), None);
    }

    #[test]
    fn cleanup_requires_all_safety_conditions() {
        let mut state = State {
            branch: None,
            head: String::new(),
            dirty: false,
            unpushed: false,
            pr: Some("MERGED".into()),
            pr_unknown: false,
            retired: true,
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
        state.retired = false;
        assert!(!eligible(&state, false));
        state.pushed_age = Some(8 * 86400);
        assert!(eligible(&state, false));
        state.pr_unknown = true;
        assert!(!eligible(&state, false));
        assert!(eligible(&state, true));
    }

    #[test]
    fn aged_open_pr_is_kept_without_force() {
        let state = State {
            branch: Some("topic".into()),
            head: "0".repeat(40),
            dirty: false,
            unpushed: false,
            pr: Some("OPEN".into()),
            pr_unknown: false,
            retired: false,
            pushed_age: Some(30 * 86400),
            age: Some(30 * 86400),
            processes: Processes::Idle,
        };
        assert!(!eligible(&state, false));
        assert!(eligible(&state, true));
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
        #[cfg(unix)]
        let other = temp.path().join("space and\nnewline");
        #[cfg(not(unix))]
        let other = temp.path().join("space and more");
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
        assert_eq!(
            fs::canonicalize(&trees[1].path).unwrap(),
            fs::canonicalize(other).unwrap()
        );
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
