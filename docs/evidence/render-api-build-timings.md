# Render-api build measurements

The extraction stops a client-world source edit from rebuilding render. The clean
build can schedule render before client-world, instead of waiting for its metadata.
These are library build measurements, not app edit-to-executable or runtime timings.

## Results

Measured on the same local Apple M3 Pro (12 cores, 36 GB), macOS 26.5.1,
`aarch64-apple-darwin`, Rust 1.93.1, on 2026-10-03. Both use the repository's dev
profile, three Cargo jobs, no debug information, no incremental compilation, and no
compiler cache. Dependencies keep the repository's optimization settings.

| Measurement | Before | After |
| --- | ---: | ---: |
| Clean build of render + client-world and their dependencies | 494.7 s | 430.0 s |
| Rebuild after the same client-world source edit | 52.6 s | 7.7 s |
| Dirty units after that edit | 2: client-world, render | 1: client-world |
| Clean dirty / fresh units | 474 / 0 | 475 / 0 |
| Clean maximum active Cargo units | 3 | 3 |
| Clean mean active Cargo units | 2.87 | 2.83 |
| Clean client-world compile interval | 447.11–466.26 s | 371.46–404.23 s |
| Clean render compile interval | 451.05–494.70 s | 366.12–430.04 s |
| Overlap between those two compiles | 15.21 s | 32.77 s |

Times come from Cargo's HTML reports and are relative to each Cargo invocation;
limiter queue time is excluded. The CLI gives 7.72 s for the after-edit run; the
HTML summary rounds it to 7.7 s. The new contract itself took 0.34 s in the clean run.

Cargo already pipelines metadata: before extraction, client-world's metadata
unblocked render (unit 472 -> 473). Render began 3.94 s after client-world started,
while client-world was still compiling. After extraction, render began 5.34 s
**before** client-world, and client-world no longer unblocked any render unit. Its
whole compile interval overlapped render. The edit run confirms the independent
invalidation boundary: render was fresh and absent from the after-edit compile list.

There is one sample per case, and other agents were compiling on this machine under
the same shared limiter. The raw clean-time decrease is not an isolated estimate of
this change's speedup: upstream compile timings also varied, and mean Cargo
concurrency did not increase. The dependency removal, avoided render rebuild and
changed scheduling order are directly demonstrated by these reports.

## Reproduction and artifacts

- Before: `8061e4b0111a2e3a3bd4611c20b801cb0319919d`.
- After: `f8a56aec9489729690f4918272c4aca6ab561e76`.
- Worktree: `/Users/hashim/Downloads/cinnabar-wt/render-api`.
- Limiter: the task's `scratchpad/cslot` (full path below).
- Before started without a target directory. After used `cargo clean`, through the
  limiter, to remove this worktree's target before rebuilding. Each clean report has
  zero fresh units. No target directory or compiler artifacts were shared.
- For each edit run, append a blank line and
  `// Timing probe: behavior-neutral client-world source edit.` to
  `crates/client-world/src/lib.rs`, run the identical command immediately after the
  clean build, then restore the original bytes. This tests Cargo invalidation without
  changing behavior or public signatures. The probe is not committed.

Run from the worktree with `CARGO_BUILD_JOBS=3`:

```sh
build_slot=/private/tmp/claude-501/-Users-hashim-Downloads-cinnabar/30f7cca3-953b-4833-843f-34f90cbe1dae/scratchpad/cslot
CARGO_BUILD_JOBS=3 "$build_slot" env RUSTC_WRAPPER= CARGO_INCREMENTAL=0 \
  CARGO_TARGET_DIR="$PWD/target" \
  cargo build --offline --locked -p render -p client-world --lib --timings
```

The limiter sets `CARGO_PROFILE_DEV_DEBUG=0`. Clearing `RUSTC_WRAPPER` after entering
the limiter disables its default sccache wrapper for both measurements. Preserve
`target/cargo-timings/cargo-timing.html` after each invocation before cleaning again.

[Structured results](render-api-build-timings.json) retain the relevant Cargo unit
records, report hashes, environment and exit statuses. Full reports and logs are
preserved locally in `/private/tmp/cinnabar-render-api-timings/`:

- [Before clean](/private/tmp/cinnabar-render-api-timings/before-clean.html)
- [Before edit](/private/tmp/cinnabar-render-api-timings/before-edit.html)
- [After clean](/private/tmp/cinnabar-render-api-timings/after-clean.html)
- [After edit](/private/tmp/cinnabar-render-api-timings/after-edit.html)

The same directory contains the exact `measure.py` runner and `summarize.py` report
parser. No client or live server was run. See the
[contract migration and vanilla references](../architecture/render-api.md) for what
moved and which behavior remains unchanged.
