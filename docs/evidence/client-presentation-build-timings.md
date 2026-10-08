# Client presentation build measurements

These measurements compare an equivalent actor-presentation source edit before
and after architecture step 5. Both rebuild the app executable with its default
features, including acceptance, its library and the final link. They do not
measure runtime performance or clean builds.

## Results

| Actor edit to executable | Before | After |
| --- | ---: | ---: |
| Cargo build time | 36.3 s | 34.9 s |
| Compiled units | 2 | 4 |
| Fresh units | 669 | 670 |
| App library compilation | 34.47 s | 30.25 s |

The observed total is 1.4 seconds shorter (3.9%). This is one sample on a shared
machine with concurrent agent builds, so the small difference does not establish
a reliable speedup.

After extraction, client-presentation takes 6.36 seconds, acceptance takes
2.42 seconds, the app library takes 30.25 seconds, and the executable takes
1.29 seconds. These durations overlap: acceptance starts at 1.55 seconds and the
app library starts at 3.38 seconds while presentation is still compiling. Do not
add unit durations to calculate elapsed time.

The actor edit leaves diagnostics, assets, launcher, client-ui, player-state,
inventory, client-world, chunk-pipeline and render fresh. The app remains the
dominant compilation unit and acceptance also rebuilds because it consumes
presentation observations. This step establishes the ownership boundaries; it
does not eliminate downstream compilation or linking.

## Method

Both phases ran on the same local macOS 26.5.1 machine with 12 logical CPUs,
`aarch64-apple-darwin`, Rust 1.93.1. Both use the dev profile: workspace optimization
level 1, dependency optimization level 3, three Cargo jobs, no debug information,
no incremental compilation and no compiler cache.

The baseline is `01f54528f5b206d9dd12d6399ce48f74a4eabe03`, archived into
`/private/tmp/cinnabar-step5-timings/baseline` with its own target directory so
implementation work could proceed separately. The measured implementation is
`3733d0fbaf27e536702b313f26563ea1c8b24ce8` in
`/Users/hashim/Coding/cinnabar-wt/step5`.

For each phase, warm the executable build, append a blank line and
`// Timing probe: behavior-neutral source edit.` to the actor source, run the same
build command, save the timing report, then restore the exact original bytes.
The source is `app/src/presentation/actors.rs` before extraction and
`crates/client-presentation/src/presentation/actors.rs` after extraction.

```sh
build_slot=/private/tmp/claude-501/-Users-hashim-Downloads-cinnabar/30f7cca3-953b-4833-843f-34f90cbe1dae/scratchpad/cslot
"$build_slot" env RUSTC_WRAPPER= CARGO_INCREMENTAL=0 \
  CARGO_TARGET_DIR="$PWD/target" \
  cargo build --offline --locked -p bedrock-client --timings
```

The limiter sets three jobs and `CARGO_PROFILE_DEV_DEBUG=0`. Clearing
`RUSTC_WRAPPER` after entering it disables its default sccache wrapper. Cargo HTML
reports exclude time waiting for the limiter; the JSON records limiter-inclusive
wall time separately. No probe is committed and targets are not shared between
source trees.

## Validation

All Cargo commands ran locally through the same limiter. The implementation passed:

- Workspace test-binary compilation: `cargo test --offline --locked --workspace --no-run`.
- The architecture check with `tools/architecture/policy.toml`, including new
  dependency and source-boundary regression tests.
- Clippy with `--all-targets -- -D warnings` for bedrock-client, client-presentation,
  acceptance, diagnostics, assets and architecture.
- All 2,046 non-ignored tests in those six packages. The final run selected only
  these packages from the already-built workspace to reuse its feature set.
- The ignored held-item frame-export test with all required fixture variables
  unset, confirming that missing local inputs skip cleanly without writing frames.
- A production `--no-default-features` check; the normal dependency graph contains
  no acceptance crate. The five unused gameplay/session accessor warnings in this
  configuration are documented in the [acceptance seam](../architecture/acceptance.md).

Three existing tests needed a retry with permission to bind their temporary local
sockets: two loopback HTTP download fixtures and one Unix control fixture. Two
registry-install tests needed `GOCACHE` under `/private/tmp` because the sandbox
denied writes to the normal Go cache. All five passed on retry. A camera test's
source-location assertion was updated to inspect the relocated writer and the
app adapter, retaining its ordering and sole-writer checks.

No live game server, remote machine, installed carrier write or new parity gate
was involved. Gameplay and session implementation files remain unchanged.

## Artifacts

The HTML reports, logs, `measure.py` runner and `summarize.py` parser remain in
`/private/tmp/cinnabar-step5-timings/`. The
[structured results](client-presentation-build-timings.json) record commands,
profile settings, compiled units, report hashes and actual exit statuses.

- [Before actor-edit report](/private/tmp/cinnabar-step5-timings/before-edit.html)
- [After actor-edit report](/private/tmp/cinnabar-step5-timings/after-edit.html)

See the [migration, ordering and vanilla references](../architecture/client-presentation.md).
