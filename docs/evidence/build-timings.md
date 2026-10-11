# Build timings

Download the `build-timings-*` artifact from an Actions run and open
`cargo-timing.html` in a browser. Package runs upload one report for each of the
four platform lanes; CI uploads the Linux workspace test build report. These
reports measure existing builds, with no extra builds in normal runs. They are
workflow artifacts and are excluded from release downloads.

Use the total build time, the concurrency graph, and the unit table together.
Find the `bedrock-client` library and binary rows to inspect the app's compile
and final link tail. Unit durations overlap, so do not add them to estimate wall
time. The report does not isolate every LLVM or linker pass. Compare the same
target, features, toolchain, runner, and cache state; a warm CI report only shows
work that actually rebuilt. The test profile is not a release baseline.

## Release-profile experiment

Once the workflow exists on the default branch, select **Build profile
experiment → Run workflow** in Actions, or run:

```sh
gh workflow run build-profile-experiment.yml --repo bedrock-mc/cinnabar --ref dev
```

Choose the branch or commit being measured with `--ref`. GitHub requires a new
manual workflow to exist on the default branch before it can be dispatched.
The experiment runs only on manual dispatch and uploads reports without
publishing binaries. Each Linux job starts without a build cache and builds the
release client with the packaging feature set:

| Artifact suffix | Release-profile overrides |
| --- | --- |
| `release` | None; use `Cargo.toml` |
| `no-lto` | `CARGO_PROFILE_RELEASE_LTO=false` |
| `codegen-16` | `CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16` |
| `no-lto-codegen-16` | Both overrides |

Only these environment overrides differ. Shipped profiles remain in
`Cargo.toml`. Compare the app rows and total Cargo build time across the four
reports to estimate the effect of ThinLTO and codegen parallelism. Runner
variation and changes in dependency compilation also affect the totals; repeat
measurements before drawing conclusions. Job duration includes setup and upload
time, so record Cargo's total separately.

The stable toolchain's `--timings` report is HTML. Machine-readable timing output
requires unstable Cargo options, so these workflows do not request JSON.

## Baseline

Pending the first runs. Record the commit, run URL, runner, toolchain, cache
state, total Cargo build time, and app library/binary times for each profile.
Keep full release builds and incremental `make play` measurements separate;
the experiment does not measure incremental builds.
