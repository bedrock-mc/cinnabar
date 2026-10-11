//! Report tests use local remotes and fake GitHub/process inventories.
#[cfg(unix)]
mod unix {
    use std::os::unix::fs::PermissionsExt;
    use std::{
        fs,
        path::{Path, PathBuf},
        process::Command,
    };

    struct Repo {
        _temp: tempfile::TempDir,
        root: PathBuf,
        bin: PathBuf,
    }
    impl Repo {
        /// Creates a published fixture commit without contacting a remote service.
        fn new() -> Self {
            let temp = tempfile::tempdir().unwrap();
            let root = temp.path().join("repo");
            fs::create_dir(&root).unwrap();
            let bin = temp.path().join("bin");
            fs::create_dir(&bin).unwrap();
            let remote = temp.path().join("remote.git");
            let repo = Self {
                _temp: temp,
                root,
                bin,
            };
            repo.git(&["init", "--quiet"]);
            fs::write(
                repo.root.join(".gitignore"),
                "target/\n/.claude/worktrees/\n",
            )
            .unwrap();
            repo.git(&["add", ".gitignore"]);
            repo.git(&[
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.invalid",
                "commit",
                "--quiet",
                "-m",
                "fixture",
            ]);
            repo.git(&["init", "--quiet", "--bare", remote.to_str().unwrap()]);
            // Pushes reach the local remote while the configured URL names the GitHub repository.
            repo.git(&["remote", "add", "origin", FIXTURE_REMOTE]);
            repo.git(&[
                "config",
                &format!("url.{}.insteadOf", remote.display()),
                FIXTURE_REMOTE,
            ]);
            repo.git(&["push", "--quiet", "origin", "HEAD"]);
            repo.script("gh", &merged_pr(&repo.head(&repo.root)));
            repo.script(
                "lsof",
                "#!/bin/sh\ncase \"$3\" in *busy*) printf '999\\n'; exit 0;; esac\nexit 1\n",
            );
            repo
        }
        /// Reads a worktree's HEAD commit.
        fn head(&self, path: &Path) -> String {
            let output = Command::new("git")
                .current_dir(path)
                .args(["rev-parse", "HEAD"])
                .output()
                .unwrap();
            String::from_utf8(output.stdout).unwrap().trim().to_owned()
        }
        /// Runs fixture Git operations with explicit identity where commits are needed.
        fn git(&self, args: &[&str]) {
            let output = Command::new("git")
                .current_dir(&self.root)
                .args(args)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        /// Adds a branch whose commit is already reachable from the local remote.
        fn worktree(&self, name: &str) -> PathBuf {
            let path = self.root.parent().unwrap().join(name);
            self.git(&[
                "worktree",
                "add",
                "--quiet",
                "-b",
                name,
                path.to_str().unwrap(),
            ]);
            path
        }
        /// Installs deterministic test commands without changing the host PATH.
        fn script(&self, name: &str, text: &str) {
            let path = self.bin.join(name);
            fs::write(&path, text).unwrap();
            fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
        }
        /// Invokes the real command with a fixture-only GitHub and lsof search path.
        fn gc(&self, args: &[&str]) -> std::process::Output {
            let path = format!("{}:{}", self.bin.display(), std::env::var("PATH").unwrap());
            Command::new(env!("CARGO_BIN_EXE_devtool"))
                .current_dir(&self.root)
                .env("PATH", path)
                .arg("wt-gc")
                .args(args)
                .output()
                .unwrap()
        }
        /// Runs the report and returns the verdict and reason printed for `branch`.
        fn verdict(&self, branch: &str) -> String {
            let output = self.gc(&[]);
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let report = String::from_utf8(output.stdout).unwrap();
            let line = report
                .lines()
                .find(|line| line.contains(&format!(" branch={branch} ")))
                .unwrap_or_else(|| panic!("no line for {branch}: {report}"));
            line[line.find("verdict=").unwrap()..].to_owned()
        }
    }
    /// Runs Git in `path`, dating commits and reflog entries `days_ago` when given.
    fn git_at(path: &Path, args: &[&str], days_ago: Option<u64>) {
        let mut command = Command::new("git");
        command.current_dir(path).args([
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
        ]);
        if let Some(days) = days_ago {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs();
            command.env(
                "GIT_COMMITTER_DATE",
                format!("{} +0000", now - days * 86400),
            );
        }
        let output = command.args(args).output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    const FIXTURE_REMOTE: &str = "https://github.com/fixture-owner/cinnabar.git";

    /// One `gh pr list` JSON entry for a PR from `owner`'s `cinnabar` fork.
    fn pr_json(state: &str, head: &str, owner: &str) -> String {
        format!(
            "{{\"state\":\"{state}\",\"headRefOid\":\"{head}\",\"headRepositoryOwner\":{{\"login\":\"{owner}\"}},\"headRepository\":{{\"name\":\"cinnabar\"}}}}"
        )
    }

    /// A fake `gh pr list` reporting one merged PR from the fixture repository whose head is `head`.
    fn merged_pr(head: &str) -> String {
        format!(
            "#!/bin/sh\nprintf '[%s]' '{}'\n",
            pr_json("MERGED", head, "fixture-owner")
        )
    }

    #[test]
    fn squash_merge_publishes_only_commits_through_the_pr_head() {
        let repo = Repo::new();
        let published = repo.worktree("squashed");
        fs::write(published.join("topic"), "published change").unwrap();
        git_at(&published, &["add", "."], None);
        git_at(
            &published,
            &["commit", "--quiet", "-m", "published topic"],
            None,
        );
        repo.git(&["push", "--quiet", "origin", "squashed"]);
        let head = repo.head(&published);
        repo.git(&["merge", "--squash", "squashed"]);
        git_at(
            &repo.root,
            &["commit", "--quiet", "-m", "squash merge"],
            None,
        );
        repo.git(&["push", "--quiet", "origin", "HEAD"]);
        repo.git(&["push", "--quiet", "origin", ":squashed"]);
        let followup = repo.worktree("followup");
        git_at(&followup, &["reset", "--quiet", "--hard", &head], None);
        git_at(
            &followup,
            &[
                "commit",
                "--allow-empty",
                "--quiet",
                "-m",
                "unpublished followup",
            ],
            None,
        );
        repo.script("gh", &merged_pr(&head));
        assert!(repo.verdict("squashed").starts_with("verdict=candidate"));
        assert_eq!(
            repo.verdict("followup"),
            "verdict=keep reason=\"unpushed commits\""
        );
    }

    #[test]
    fn report_never_removes_anything_and_rejects_cleanup_flags() {
        let repo = Repo::new();
        let merged = repo.worktree("merged");
        fs::create_dir(merged.join("target")).unwrap();
        assert!(repo.verdict("merged").starts_with("verdict=candidate"));
        assert!(merged.join("target").exists(), "the report deleted files");
        for flag in ["--apply", "--force"] {
            assert!(!repo.gc(&[flag]).status.success(), "accepted {flag}");
        }
        assert!(merged.exists());
    }

    #[test]
    fn deleted_worktrees_are_reported_without_blocking_the_inventory() {
        let repo = Repo::new();
        let deleted = repo.worktree("deleted");
        repo.worktree("remaining");
        fs::remove_dir_all(&deleted).unwrap();
        assert!(repo.verdict("remaining").starts_with("verdict=candidate"));
        let report = String::from_utf8(repo.gc(&[]).stdout).unwrap();
        let line = report
            .lines()
            .find(|line| line.contains("deleted"))
            .unwrap();
        assert!(line.contains("verdict=keep"), "{line}");
    }

    #[test]
    fn changes_unpublished_commits_and_busy_trees_are_kept() {
        let repo = Repo::new();
        let dirty = repo.worktree("dirty");
        repo.worktree("busy");
        let unpublished = repo.worktree("unpublished");
        fs::write(dirty.join("new"), "changes").unwrap();
        git_at(
            &unpublished,
            &["commit", "--allow-empty", "--quiet", "-m", "unpublished"],
            None,
        );
        assert_eq!(
            repo.verdict("dirty"),
            "verdict=keep reason=\"uncommitted changes\""
        );
        assert_eq!(
            repo.verdict("busy"),
            "verdict=keep reason=\"processes are using it\""
        );
        assert_eq!(
            repo.verdict("unpublished"),
            "verdict=keep reason=\"unpushed commits\""
        );
    }

    #[test]
    fn merged_parent_of_a_nested_registered_worktree_is_kept() {
        let repo = Repo::new();
        let parent = repo.worktree("parent");
        let child = parent.join(".claude/worktrees/child");
        repo.git(&[
            "worktree",
            "add",
            "--quiet",
            "-b",
            "child",
            child.to_str().unwrap(),
        ]);
        assert_eq!(
            repo.verdict("parent"),
            "verdict=keep reason=\"contains another worktree\""
        );
    }

    #[test]
    fn reused_branch_with_commits_after_its_merged_pr_is_kept() {
        let repo = Repo::new();
        let path = repo.worktree("reused");
        git_at(
            &path,
            &["commit", "--allow-empty", "--quiet", "-m", "new work"],
            None,
        );
        repo.git(&["push", "--quiet", "origin", "reused"]);
        assert_eq!(
            repo.verdict("reused"),
            "verdict=keep reason=\"recent work\""
        );
    }

    #[test]
    fn failed_pr_lookup_keeps_a_stale_pushed_worktree() {
        let repo = Repo::new();
        let path = repo.worktree("stale");
        git_at(
            &path,
            &["commit", "--allow-empty", "--quiet", "-m", "old"],
            Some(30),
        );
        git_at(
            &path,
            &["push", "--quiet", "-u", "origin", "stale"],
            Some(30),
        );
        repo.script("gh", "#!/bin/sh\necho 'To get started with GitHub CLI, please run:  gh auth login' >&2\nexit 4\n");
        assert_eq!(
            repo.verdict("stale"),
            "verdict=keep reason=\"PR status unknown\""
        );
        repo.script("gh", "#!/bin/sh\nprintf '[]'\n");
        assert_eq!(
            repo.verdict("stale"),
            "verdict=candidate reason=\"no push for over 7 days\""
        );
    }

    #[test]
    fn push_age_comes_from_the_branch_push_not_an_aged_upstream() {
        let repo = Repo::new();
        git_at(
            &repo.root,
            &["commit", "--allow-empty", "--quiet", "-m", "old dev"],
            Some(30),
        );
        git_at(&repo.root, &["push", "--quiet", "origin", "HEAD"], Some(30));
        let output = Command::new("git")
            .current_dir(&repo.root)
            .args(["rev-parse", "--abbrev-ref", "HEAD"])
            .output()
            .unwrap();
        let default = String::from_utf8(output.stdout).unwrap();
        let path = repo.root.parent().unwrap().join("topic");
        repo.git(&[
            "worktree",
            "add",
            "--quiet",
            "--track",
            "-b",
            "topic",
            path.to_str().unwrap(),
            &format!("origin/{}", default.trim()),
        ]);
        git_at(
            &path,
            &["commit", "--allow-empty", "--quiet", "-m", "topic"],
            None,
        );
        git_at(&path, &["push", "--quiet", "origin", "topic"], None);
        repo.script("gh", "#!/bin/sh\nprintf '[]'\n");
        assert_eq!(repo.verdict("topic"), "verdict=keep reason=\"recent work\"");
    }

    #[test]
    fn open_pr_beyond_the_history_page_keeps_the_worktree() {
        let repo = Repo::new();
        let path = repo.worktree("reopened");
        let head = repo.head(&path);
        // Only the open-state query sees the open PR; the all-state page holds older merged ones.
        repo.script(
            "gh",
            &format!(
                "#!/bin/sh\ncase \"$4\" in open) printf '[%s]' '{}';; *) printf '[%s]' '{}';; esac\n",
                pr_json("OPEN", &head, "fixture-owner"),
                pr_json("MERGED", &head, "fixture-owner")
            ),
        );
        assert_eq!(repo.verdict("reopened"), "verdict=keep reason=\"open PR\"");
    }

    #[test]
    fn another_forks_merged_pr_does_not_retire_the_branch() {
        let repo = Repo::new();
        let path = repo.worktree("shared-name");
        repo.script(
            "gh",
            &format!(
                "#!/bin/sh\nprintf '[%s]' '{}'\n",
                pr_json("MERGED", &repo.head(&path), "someone-else")
            ),
        );
        assert_eq!(
            repo.verdict("shared-name"),
            "verdict=keep reason=\"recent work\""
        );
    }

    #[test]
    fn fork_push_url_matches_the_forks_open_pr() {
        let repo = Repo::new();
        let path = repo.worktree("forked");
        repo.git(&[
            "config",
            "remote.origin.pushurl",
            "https://github.com/fork-owner/cinnabar.git",
        ]);
        repo.script(
            "gh",
            &format!(
                "#!/bin/sh\nprintf '[%s]' '{}'\n",
                pr_json("OPEN", &repo.head(&path), "fork-owner")
            ),
        );
        assert_eq!(repo.verdict("forked"), "verdict=keep reason=\"open PR\"");
    }

    #[test]
    fn unborn_worktree_is_kept_without_aborting_the_report() {
        let repo = Repo::new();
        let orphan = repo.worktree("orphan-base");
        git_at(
            &orphan,
            &["checkout", "--quiet", "--orphan", "unborn"],
            None,
        );
        repo.worktree("later");
        assert!(repo.verdict("later").starts_with("verdict="));
        let report = String::from_utf8(repo.gc(&[]).stdout).unwrap();
        let line = report
            .lines()
            .find(|line| line.contains("orphan-base "))
            .unwrap_or_else(|| panic!("unborn worktree missing: {report}"));
        assert!(line.contains("verdict=keep"), "{line}");
    }

    #[test]
    fn unavailable_process_checks_keep_worktrees() {
        let repo = Repo::new();
        repo.worktree("unknown-process");
        repo.script("lsof", "#!/bin/sh\nexit 127\n");
        assert_eq!(
            repo.verdict("unknown-process"),
            "verdict=keep reason=\"process check unavailable\""
        );
    }
}
