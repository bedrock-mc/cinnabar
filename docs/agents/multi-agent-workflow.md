# Build and verification workflow

## Concurrent builds

Bound concurrent Cargo builds to the machine's available CPU, memory, and disk.
Use the configured shared build limiter when available and never hold a build lock
while waiting for another. Set `CARGO_BUILD_JOBS` explicitly when several worktrees
are active. Prefer `cargo check -p <crate>` and avoid release builds unless the task
requires performance measurements. Restart sccache only when no build is using it.

Keep a separate Cargo `target` directory for each active worktree; never share
`CARGO_TARGET_DIR` across branches. Remove that directory after the work is
integrated. Build test carriers into an isolated scratch directory so concurrent
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

`ci-ok` is the required check. Pull requests run formatting, the architecture gate,
clippy, and Rust and Go tests on Linux. Windows and macOS run
`cargo check --workspace --all-targets`, client library tests, and the crates in
`PLATFORM_TEST_PACKAGES` in `.github/workflows/ci.yml`. Add a crate there when its
tests exercise platform-specific code. Packaging, physics-bootstrap, and
PowerShell-harness jobs run when their inputs change.

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
save caches; PR and tag runs restore them. After a successful save, older entries
in that platform/profile bucket are removed. Go uses the version in `core/go.mod`
and the same restore/save ownership, without timestamp restoration.

The first run after a cache format change is cold. Check the restored-source count,
Cargo build durations, and cache sizes on subsequent runs before claiming a speedup.
Keep release optimization settings and test coverage unchanged when tuning caches.
