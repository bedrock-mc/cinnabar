//! Read-only worktree report: each worktree is a cleanup candidate or kept, with the reason.
//! Nothing is deleted; the user removes candidates with `git worktree remove`.
use crate::DevtoolError;
use serde::Deserialize;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

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
    #[serde(rename = "headRepositoryOwner")]
    owner: Option<Owner>,
    #[serde(rename = "headRepository")]
    repository: Option<Repository>,
}
#[derive(Debug, Deserialize)]
struct Owner {
    login: String,
}
#[derive(Debug, Deserialize)]
struct Repository {
    name: String,
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

/// Prints one line per registered worktree with its verdict; takes no arguments.
pub(crate) fn run(args: &[String]) -> Result<(), DevtoolError> {
    if let Some(arg) = args.first() {
        return Err(DevtoolError::Usage(format!(
            "unknown wt-gc argument: {arg}"
        )));
    }
    // Only Git's line ending is stripped; a path may itself end in whitespace.
    let toplevel = git(Path::new("."), &["rev-parse", "--show-toplevel"])?;
    let root = PathBuf::from(toplevel.strip_suffix('\n').unwrap_or(&toplevel));
    let worktrees = parse_worktrees(&git(&root, &["worktree", "list", "--porcelain", "-z"])?);
    for (index, wt) in worktrees.iter().enumerate() {
        let state = inspect(wt)?;
        let (candidate, reason) =
            if index == 0 || fs::canonicalize(&wt.path).ok() == fs::canonicalize(&root).ok() {
                (false, "main checkout")
            } else if wt.protected {
                (false, "locked, bare, or prunable")
            } else if contains_worktree(&wt.path, &worktrees) {
                (false, "contains another worktree")
            } else {
                verdict(&state)
            };
        println!(
            "{} branch={} PR={} availability={} changes={} unpushed={} processes={:?} age_days={} pushed_days={} verdict={} reason=\"{reason}\"",
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
            if candidate { "candidate" } else { "keep" },
        );
    }
    println!("report only; remove candidates yourself with `git worktree remove <path>`");
    Ok(())
}

/// Returns whether a worktree is a cleanup candidate and why; any doubt keeps it.
fn verdict(state: &State) -> (bool, &'static str) {
    match state {
        State {
            processes: Processes::Busy,
            ..
        } => (false, "processes are using it"),
        State {
            processes: Processes::Unknown,
            ..
        } => (false, "process check unavailable"),
        State { dirty: true, .. } => (false, "uncommitted changes"),
        State { unpushed: true, .. } => (false, "unpushed commits"),
        State {
            pr_unknown: true, ..
        } => (false, "PR status unknown"),
        _ if state.pr.as_deref() == Some("OPEN") => (false, "open PR"),
        State { retired: true, .. } => (true, "PR completed at this HEAD"),
        _ if state.pushed_age.is_some_and(|age| age > 7 * 86400) => {
            (true, "no push for over 7 days")
        }
        _ => (false, "recent work"),
    }
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
    let unknown = || State {
        branch: None,
        dirty: true,
        unpushed: true,
        pr: None,
        pr_unknown: true,
        retired: false,
        pushed_age: None,
        age: None,
        processes: Processes::Unknown,
    };
    if wt.unavailable || !wt.path.is_dir() {
        return Ok(unknown());
    }
    let dirty = !git(
        &wt.path,
        &["status", "--porcelain", "--untracked-files=all"],
    )?
    .is_empty();
    // An unborn branch has no HEAD to classify; keep it without aborting the report.
    let Ok((branch, head)) = checkout(&wt.path) else {
        return Ok(State { dirty, ..unknown() });
    };
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

/// Returns the open or newest PR for `branch`, or `None` when the lookup or the branch's GitHub
/// repository is unknown; reads the published head even after a squash deletes its branch.
fn pull_request(path: &Path, branch: Option<&str>) -> Option<Option<PullRequest>> {
    let Some(branch) = branch else {
        return Some(None);
    };
    // Forks can publish the same branch name, so only PRs from this branch's repository count.
    let (owner, name) = publishing_repository(path, branch)?;
    let ours = |pr: &PullRequest| {
        pr.owner
            .as_ref()
            .is_some_and(|head| head.login.eq_ignore_ascii_case(&owner))
            && pr
                .repository
                .as_ref()
                .is_some_and(|head| head.name.eq_ignore_ascii_case(&name))
    };
    // Open PRs are queried alone: a reused branch name can have more history than one page.
    if let Some(open) = pull_requests(path, branch, "open")?
        .into_iter()
        .find(|pr| pr.state == "OPEN" && ours(pr))
    {
        return Some(Some(open));
    }
    Some(
        pull_requests(path, branch, "all")?
            .into_iter()
            .find(|pr| ours(pr)),
    )
}

/// Returns the GitHub `(owner, name)` that `branch` is pushed to, read from its push or upstream
/// remote's configured push URL, falling back to its URL.
fn publishing_repository(path: &Path, branch: &str) -> Option<(String, String)> {
    let config = |key: String| {
        git(path, &["config", "--get", &key])
            .ok()
            .map(|value| value.trim().to_owned())
    };
    let remote = config(format!("branch.{branch}.pushRemote"))
        .or_else(|| config("remote.pushDefault".into()))
        .or_else(|| config(format!("branch.{branch}.remote")))
        .unwrap_or_else(|| "origin".into());
    github_repository(
        &config(format!("remote.{remote}.pushurl"))
            .or_else(|| config(format!("remote.{remote}.url")))?,
    )
}

/// Parses GitHub remote URLs and resolves the former Cinnabar owner to its current owner.
fn github_repository(url: &str) -> Option<(String, String)> {
    let (_, path) = url.split_once("github.com")?;
    let path = path.strip_prefix([':', '/'])?.trim_end_matches('/');
    let (owner, name) = path.strip_suffix(".git").unwrap_or(path).split_once('/')?;
    if owner.is_empty() || name.is_empty() || name.contains('/') {
        return None;
    }
    let canonical: serde_json::Value =
        serde_json::from_str(include_str!("../../../packaging/release-assets.json")).ok()?;
    let (current_owner, current_name) = canonical["repository"].as_str()?.split_once('/')?;
    let owner =
        if owner.eq_ignore_ascii_case("bedrock-mc") && name.eq_ignore_ascii_case(current_name) {
            current_owner
        } else {
            owner
        };
    Some((owner.to_owned(), name.to_owned()))
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
            "state,headRefOid,headRepositoryOwner,headRepository",
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
    fn github_remote_urls_resolve_their_repository() {
        let manifest: serde_json::Value =
            serde_json::from_str(include_str!("../../../packaging/release-assets.json")).unwrap();
        let (owner, name) = manifest["repository"]
            .as_str()
            .unwrap()
            .split_once('/')
            .unwrap();
        let expected = Some((owner.to_owned(), name.to_owned()));
        for org in [owner, "bedrock-mc"] {
            for url in [
                format!("https://github.com/{org}/{name}.git"),
                format!("https://github.com/{org}/{name}/"),
                format!("git@github.com:{org}/{name}.git"),
                format!("ssh://git@github.com/{org}/{name}"),
            ] {
                assert_eq!(github_repository(&url), expected, "{url}");
            }
        }
        for (org, repo) in [("bedrock-mc", "dragonfly"), ("contributor", name)] {
            assert_eq!(
                github_repository(&format!("https://github.com/{org}/{repo}.git")),
                Some((org.to_owned(), repo.to_owned())),
            );
        }
        assert_eq!(github_repository("/srv/git/cinnabar.git"), None);
    }

    #[test]
    fn candidates_require_every_safety_condition() {
        let mut state = State {
            branch: None,
            dirty: false,
            unpushed: false,
            pr: Some("MERGED".into()),
            pr_unknown: false,
            retired: true,
            pushed_age: None,
            age: None,
            processes: Processes::Idle,
        };
        assert!(verdict(&state).0);
        state.dirty = true;
        assert_eq!(verdict(&state), (false, "uncommitted changes"));
        state.dirty = false;
        state.unpushed = true;
        assert_eq!(verdict(&state), (false, "unpushed commits"));
        state.unpushed = false;
        state.processes = Processes::Unknown;
        assert_eq!(verdict(&state), (false, "process check unavailable"));
        state.processes = Processes::Busy;
        assert_eq!(verdict(&state), (false, "processes are using it"));
        state.processes = Processes::Idle;
        state.retired = false;
        assert_eq!(verdict(&state), (false, "recent work"));
        state.pushed_age = Some(8 * 86400);
        assert!(verdict(&state).0);
        state.pr_unknown = true;
        assert_eq!(verdict(&state), (false, "PR status unknown"));
    }

    #[test]
    fn aged_open_pr_is_kept() {
        let state = State {
            branch: Some("topic".into()),
            dirty: false,
            unpushed: false,
            pr: Some("OPEN".into()),
            pr_unknown: false,
            retired: false,
            pushed_age: Some(30 * 86400),
            age: Some(30 * 86400),
            processes: Processes::Idle,
        };
        assert_eq!(verdict(&state), (false, "open PR"));
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
