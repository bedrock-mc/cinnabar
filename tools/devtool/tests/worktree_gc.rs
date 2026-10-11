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
            fs::write(repo.root.join(".gitignore"), "target/\n").unwrap();
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
            repo.git(&["remote", "add", "origin", remote.to_str().unwrap()]);
            repo.git(&["push", "--quiet", "origin", "HEAD"]);
            repo.script("gh", "#!/bin/sh\nprintf '[{\"state\":\"MERGED\"}]'\n");
            repo.script(
                "lsof",
                "#!/bin/sh\ncase \"$3\" in *busy*) printf '999\\n'; exit 0;; esac\nexit 1\n",
            );
            repo
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
        repo.script(
            "gh",
            &format!("#!/bin/sh\nprintf '[{{\"state\":\"MERGED\",\"headRefOid\":\"{head}\"}}]'\n"),
        );
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
