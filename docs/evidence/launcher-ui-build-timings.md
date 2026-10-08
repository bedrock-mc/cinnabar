# Launcher and client UI build measurements

These measurements compare an equivalent UI presentation edit before and after
restructuring step 3. Each builds the app executable, including its library and
final link. They do not measure runtime performance or clean builds.

## Results

| UI edit to executable | Before | After |
| --- | ---: | ---: |
| Cargo build time | 139.3 s | 60.4 s |
| Compiled units | 2 | 3 |
| Fresh units | 679 | 681 |

The observed rebuild is 78.9 seconds shorter (56.6%). Before extraction, the app
library takes 136.11 seconds and its executable takes 2.21 seconds. After
extraction, client-ui takes 19.06 seconds, the app library takes 53.92 seconds and
the executable takes 2.69 seconds. These durations overlap: the app library starts
at 3.77 seconds while client-ui is still compiling, then the executable starts at
57.69 seconds. They should not be added to obtain elapsed build time.

The UI edit leaves launcher, player-state, inventory, client-world and render
fresh. The app still rebuilds because it consumes client-ui; the migration reduces
the app compilation unit and permits overlapping compilation, rather than
eliminating downstream compilation or linking.

## Method

Both phases run on the same local Apple M3 Pro (12 cores, 36 GB), macOS 26.5.1,
`aarch64-apple-darwin`, Rust 1.93.1. Both use the repository's dev profile:
workspace optimization level 1, dependency optimization level 3, three Cargo jobs,
no debug information, no incremental compilation and no compiler cache.

The baseline is `75ccf7cf62035a41c00d9146ed384b874057ce99`, after the inventory
extraction landed. The measured implementation is
`3667d25adda2ae12fc8557ff047f0338f654ddfc`. The worktree is
`/Users/hashim/Coding/cinnabar-wt/render-api`. Warm the app, append a blank line and
`// Timing probe: behavior-neutral source edit.` to the presentation source, build
with the same command, save the timing report, then restore the original bytes.
The source is `app/src/ui_runtime/presentation.rs` before extraction and
`crates/client-ui/src/ui_runtime/presentation.rs` after extraction.

```sh
build_slot=/private/tmp/claude-501/-Users-hashim-Downloads-cinnabar/30f7cca3-953b-4833-843f-34f90cbe1dae/scratchpad/cslot
CARGO_BUILD_JOBS=3 "$build_slot" env RUSTC_WRAPPER= CARGO_INCREMENTAL=0 \
  CARGO_TARGET_DIR="$PWD/target" \
  cargo build --offline --locked -p bedrock-client --timings
```

The limiter sets `CARGO_PROFILE_DEV_DEBUG=0`. Clearing `RUSTC_WRAPPER` after entering
the limiter disables its default sccache wrapper. No probe is committed, and no
target directory is shared with another worktree.

The Cargo HTML reports exclude time waiting for the shared limiter. There is one
sample per case, with other agents compiling on the machine. Elapsed-time changes
are observations, not an isolated or statistically established speedup.

## Artifacts

The complete HTML reports, logs, `measure.py` runner and `summarize.py` parser remain
in `/private/tmp/cinnabar-launcher-ui-timings/`. Structured results record compiled
units, report hashes, command, environment and real exit status, with
limiter-inclusive wall time stored separately in
[the structured results](launcher-ui-build-timings.json).

- [Before UI edit report](/private/tmp/cinnabar-launcher-ui-timings/before-ui-edit.html)
- [After UI edit report](/private/tmp/cinnabar-launcher-ui-timings/after-ui-edit.html)

See the [migration, ordering and vanilla references](../architecture/launcher-ui.md).
No client or live game server was run.
