# Build and verification workflow

## Concurrent builds

Bound concurrent Cargo builds to the machine's available CPU, memory, and disk.
Use the configured shared build limiter when available and never hold a build lock
while waiting for another. Set `CARGO_BUILD_JOBS` explicitly when several worktrees
are active. Prefer `cargo check -p <crate>` and avoid release builds unless the task
requires performance measurements. Local builds do not use sccache.

`.cargo/config.toml` gives each worktree its own Cargo `build-dir` under
`$CARGO_HOME/build/cinnabar`, keyed by workspace path; `target` keeps only final artifacts. Never
share a `build-dir` or `CARGO_TARGET_DIR` between worktrees: workspace crates hash the
same in every checkout and freshness is mtime-based, so Cargo would reuse another
worktree's stale crates. Remove a worktree's `target` after its work is integrated. Build test carriers into an isolated scratch directory so concurrent
runs cannot overwrite each other's inputs. Stop only processes you started, by PID.

## Checks

For PR work, run `cargo check --tests -p <touched-crate>`, push, and open the PR.
CI and code review are the merge gate and run in parallel. Do not repeat clippy,
nextest, or `verify-affected` locally on top of PR CI.

Before an authorized direct push to `dev`, run:

```sh
cargo run -p devtool --locked -- verify-affected --base origin/dev
```

Read the command's exit status; a finished background job does not mean a passing
check. Investigate network or local-fixture failures, rerun after correcting their
cause, and report any validation that remains incomplete.

## CI

`ci-ok` is the required check. Pull requests run formatting, the architecture gate, and
Go tests on Linux. Rust jobs build and lint the whole workspace, so only changed crates and
their reverse dependencies recompile, and `devtool ci-plan` limits Rust tests to those
crates. Root Cargo files, `.cargo/`, toolchain files, workflows, `tools/architecture`, and
paths it cannot classify run every test. A crate that reads files outside its own directory lists
them under `[package.metadata.cinnabar]` as `reads = ["crates/<crate>/data/*"]`, so changing
one selects the reader too; the architecture gate rejects undeclared and stale reads. Go
sources still run every test.

Crates with macOS or Windows code declare it in `Cargo.toml` as
`[package.metadata.cinnabar] platform-specific = ["macos", "windows"]`; the architecture
gate rejects missing and stale declarations. A Windows or macOS job runs when a changed
crate declares that platform, a shader or workflow changes, or every test runs. It tests
the affected crates that declare the platform, with only the client's library tests. Full
runs catch platform-only breakage reached through another crate's API. Packaging,
physics-bootstrap, and PowerShell-harness jobs run when their inputs change.

The full three-platform matrix runs on `dev` every six hours and can be requested
with `gh workflow run ci.yml --ref <branch>`. `ci-cache.yml` tests `dev` on Linux
hourly. Fix failing scheduled checks promptly.

### Build caches

CI and packaging use `.github/actions/build-cache`. Rust caches keep workspace
artifacts as well as dependencies. The action hashes tracked inputs and restores
old file and directory timestamps only for identical tracked contents and
permissions, so fresh checkouts do not force recompilation. Changed inputs get
fresh timestamps after restoration. A source change
during the build prevents saving artifacts against the wrong input snapshot.
Directories with untracked inputs keep their current timestamps.

Cache keys separate OS, architecture, compiler settings, and CI/release profiles.
Each run gets a new key with a compatible fallback. Only runs on `dev`
save caches; PR and tag runs restore them. Go workflows use `stable`, with the
resolved compiler version included in cache keys. Go uses the same restore/save
ownership, without timestamp restoration.

CI buckets live in the shared GitHub cache, which evicts least recently used entries
past 10 GB. Each bucket has a budget in `.github/actions/build-cache/budgets.json`,
the budgets must fit the limit together, and a save deletes its bucket's old entry
first, then fails if the new one is over budget. Release buckets, used once a day,
would be the first evicted there, so `package.yml` stores them as workflow artifacts
instead, kept for three days with their own budgets.

CI builds with `--workspace` and selects tests with nextest filtersets: a `-p` subset
unifies features differently and rebuilds cached dependencies.

The first run after a cache format change is cold. Check the restored-source count,
Cargo build durations, and cache sizes on subsequent runs before claiming a speedup.
Keep release optimization settings and test coverage unchanged when tuning caches.
