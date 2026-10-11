//! Cleanup tests use local remotes and fake GitHub/process inventories.
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
        let commit = |path: &Path, message: &str| {
            assert!(
                Command::new("git")
                    .current_dir(path)
                    .args(["add", "."])
                    .status()
                    .unwrap()
                    .success()
            );
            assert!(
                Command::new("git")
                    .current_dir(path)
                    .args([
                        "-c",
                        "user.name=Fixture",
                        "-c",
                        "user.email=fixture@example.invalid",
                        "commit",
                        "--quiet",
                        "-m",
                        message
                    ])
                    .status()
                    .unwrap()
                    .success()
            );
        };
        commit(&published, "published topic");
        repo.git(&["push", "--quiet", "origin", "squashed"]);
        let head = Command::new("git")
            .current_dir(&published)
            .args(["rev-parse", "HEAD"])
            .output()
            .unwrap();
        let head = String::from_utf8(head.stdout).unwrap();
        let head = head.trim();
        repo.git(&["merge", "--squash", "squashed"]);
        commit(&repo.root, "squash merge");
        repo.git(&["push", "--quiet", "origin", "HEAD"]);
        repo.git(&["push", "--quiet", "origin", ":squashed"]);
        let followup = repo.worktree("followup");
        assert!(
            Command::new("git")
                .current_dir(&followup)
                .args(["reset", "--hard", head])
                .status()
                .unwrap()
                .success()
        );
        fs::write(followup.join("new"), "unpublished change").unwrap();
        commit(&followup, "unpublished followup");
        repo.script("gh", &merged_pr(head));
        let dry = repo.gc(&[]);
        assert!(dry.status.success());
        let lines = String::from_utf8_lossy(&dry.stdout);
        assert!(
            lines
                .lines()
                .any(|line| line.contains("branch=squashed ") && line.contains("unpushed=false")),
            "{lines}"
        );
        assert!(
            lines
                .lines()
                .any(|line| line.contains("branch=followup ") && line.contains("unpushed=true")),
            "{lines}"
        );
        let apply = repo.gc(&["--apply"]);
        assert!(
            apply.status.success(),
            "{}",
            String::from_utf8_lossy(&apply.stderr)
        );
        assert!(!published.exists());
        assert!(followup.exists());
        repo.git(&["show-ref", "--verify", "refs/heads/squashed"]);
    }

    #[test]
    fn deleted_worktrees_do_not_block_inventory_or_cleanup() {
        let repo = Repo::new();
        let deleted = repo.worktree("deleted");
        let merged = repo.worktree("remaining");
        fs::remove_dir_all(&deleted).unwrap();
        let dry = repo.gc(&[]);
        assert!(
            dry.status.success(),
            "{}",
            String::from_utf8_lossy(&dry.stderr)
        );
        assert!(String::from_utf8_lossy(&dry.stdout).contains("deleted"));
        assert!(merged.exists());
        let apply = repo.gc(&["--apply"]);
        assert!(
            apply.status.success(),
            "{}",
            String::from_utf8_lossy(&apply.stderr)
        );
        assert!(!merged.exists());
        repo.git(&["show-ref", "--verify", "refs/heads/deleted"]);
    }

    #[test]
    fn dry_run_and_apply_protect_changes_unpublished_commits_and_busy_trees() {
        let repo = Repo::new();
        let merged = repo.worktree("merged");
        let dirty = repo.worktree("dirty");
        let busy = repo.worktree("busy");
        let unpublished = repo.worktree("unpublished");
        fs::write(dirty.join("new"), "changes").unwrap();
        assert!(
            Command::new("git")
                .current_dir(&unpublished)
                .args([
                    "-c",
                    "user.name=Fixture",
                    "-c",
                    "user.email=fixture@example.invalid",
                    "commit",
                    "--allow-empty",
                    "--quiet",
                    "-m",
                    "unpublished"
                ])
                .status()
                .unwrap()
                .success()
        );
        let dry = repo.gc(&[]);
        assert!(dry.status.success());
        assert!(merged.exists());
        let apply = repo.gc(&["--apply"]);
        assert!(
            apply.status.success(),
            "{}",
            String::from_utf8_lossy(&apply.stderr)
        );
        assert!(!merged.exists());
        for path in [&repo.root, &dirty, &busy, &unpublished] {
            assert!(path.exists());
        }
        repo.git(&["show-ref", "--verify", "refs/heads/merged"]);
        let force = repo.gc(&["--apply", "--force", dirty.to_str().unwrap()]);
        assert!(force.status.success());
        assert!(!dirty.exists());
        let force_busy = repo.gc(&["--apply", "--force", busy.to_str().unwrap()]);
        assert!(force_busy.status.success());
        assert!(busy.exists());
    }
    #[test]
    fn branch_switch_after_inventory_keeps_unpublished_worktree_and_target() {
        let repo = Repo::new();
        let path = repo.worktree("switched");
        let run = |args: &[&str]| {
            let result = Command::new("git")
                .current_dir(&path)
                .args(args)
                .output()
                .unwrap();
            assert!(
                result.status.success(),
                "{}",
                String::from_utf8_lossy(&result.stderr)
            );
        };
        run(&["switch", "--quiet", "-c", "unpublished"]);
        run(&[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "commit",
            "--allow-empty",
            "--quiet",
            "-m",
            "unpublished",
        ]);
        run(&["switch", "--quiet", "switched"]);
        fs::create_dir(path.join("target")).unwrap();
        fs::write(path.join("target/keep"), "artifact").unwrap();
        let merged = repo.head(&path);
        repo.script(
            "gh",
            &format!(
                "#!/bin/sh\ncase \"$6\" in unpublished) printf '[]';; *) printf '[%s]' '{}';; esac\n",
                pr_json("MERGED", &merged, "fixture-owner")
            ),
        );
        repo.script("lsof", &format!(
            "#!/bin/sh\ncase \"$3\" in *switched) if [ ! -e '{}' ]; then git -C \"$3\" switch --quiet unpublished; touch '{}'; fi;; esac\nexit 1\n",
            repo.bin.join("switched-once").display(), repo.bin.join("switched-once").display()));
        let output = repo.gc(&["--apply"]);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            path.exists(),
            "removed a newly checked-out unpublished branch"
        );
        assert!(path.join("target/keep").exists(), "removed its artifacts");
        run(&["show-ref", "--verify", "refs/heads/unpublished"]);
    }

    #[test]
    fn process_recheck_prevents_removal_when_a_process_appears() {
        let repo = Repo::new();
        let path = repo.worktree("late-process");
        let counter = repo.bin.join("calls");
        repo.script("lsof", &format!("#!/bin/sh\ncase \"$3\" in *late-process*) n=$(cat '{}' 2>/dev/null || printf 0); n=$((n+1)); printf '%s' \"$n\" > '{}'; if [ \"$n\" -gt 1 ]; then printf '999\\n'; exit 0; fi;; esac\nexit 1\n", counter.display(), counter.display()));
        let output = repo.gc(&["--apply"]);
        assert!(output.status.success());
        assert!(path.exists());
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
        fs::write(child.join("work"), "uncommitted").unwrap();
        let output = repo.gc(&["--apply"]);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(parent.exists(), "removed a worktree containing another");
        assert!(
            child.join("work").exists(),
            "lost the nested worktree's changes"
        );
    }

    #[test]
    fn reused_branch_with_commits_after_its_merged_pr_is_kept() {
        let repo = Repo::new();
        let path = repo.worktree("reused");
        assert!(
            Command::new("git")
                .current_dir(&path)
                .args([
                    "-c",
                    "user.name=Fixture",
                    "-c",
                    "user.email=fixture@example.invalid",
                    "commit",
                    "--allow-empty",
                    "--quiet",
                    "-m",
                    "new work on a reused branch"
                ])
                .status()
                .unwrap()
                .success()
        );
        repo.git(&["push", "--quiet", "origin", "reused"]);
        let output = repo.gc(&["--apply"]);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(path.exists(), "an older merged PR retired newer commits");
    }

    #[test]
    fn checkout_change_during_the_final_process_check_keeps_the_worktree() {
        for change in [
            "-c user.name=Fixture -c user.email=fixture@example.invalid commit --allow-empty --quiet -m late",
            "switch --quiet -c late-branch",
        ] {
            let repo = Repo::new();
            let path = repo.worktree("late-checkout");
            let counter = repo.bin.join("calls");
            // Inventory, recheck, then the final activity check: change the checkout on the third.
            repo.script("lsof", &format!("#!/bin/sh\ncase \"$3\" in *late-checkout*) n=$(cat '{}' 2>/dev/null || printf 0); n=$((n+1)); printf '%s' \"$n\" > '{}'; if [ \"$n\" -eq 3 ]; then git -C \"$3\" {change}; fi;; esac\nexit 1\n", counter.display(), counter.display()));
            let output = repo.gc(&["--apply"]);
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(path.exists(), "removed after `git {change}`");
            assert_eq!(fs::read_to_string(&counter).unwrap(), "3");
        }
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
        let output = repo.gc(&["--apply"]);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            path.exists(),
            "removed without knowing whether a PR is open"
        );
        repo.script("gh", "#!/bin/sh\nprintf '[]'\n");
        assert!(repo.gc(&["--apply"]).status.success());
        assert!(!path.exists(), "kept a stale worktree with no PR");
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
        let output = repo.gc(&["--apply"]);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(path.exists(), "a fresh push inherited its upstream's age");
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
        let output = repo.gc(&["--apply"]);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(path.exists(), "removed a worktree whose PR is open");
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
        let output = repo.gc(&["--apply"]);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(path.exists(), "a fork's merged PR retired this branch");
    }

    #[test]
    fn unavailable_process_checks_keep_worktrees_and_symlink_targets() {
        let repo = Repo::new();
        let path = repo.worktree("unknown-process");
        repo.script("lsof", "#!/bin/sh\nexit 127\n");
        assert!(repo.gc(&["--apply"]).status.success());
        assert!(path.exists());
        let outside = repo.root.parent().unwrap().join("shared-target");
        fs::create_dir(&outside).unwrap();
        fs::write(outside.join("keep"), "artifact").unwrap();
        std::os::unix::fs::symlink(&outside, path.join("target")).unwrap();
        assert!(Path::new(&outside).join("keep").exists());
    }
}
