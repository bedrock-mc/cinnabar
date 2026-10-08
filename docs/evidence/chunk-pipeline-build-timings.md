# Chunk-pipeline build measurements

These measurements compare stream and renderer edits before and after architecture
step 6. Each command builds the app library and executable, including the final
link. They measure rebuilds after comment-only source edits, not runtime speed or
clean builds.

## Results

The same local Apple M3 Pro (12 cores, 36 GB), macOS 26.5.1 and Rust 1.93.1 were used
for both phases. Builds use three Cargo jobs, workspace optimization level 1,
dependency optimization level 3, no debug information, no incremental compilation
and no compiler cache.

| Edited component and executable rebuild | Before | After |
| --- | ---: | ---: |
| Stream module, moved from client-world to chunk-pipeline | 174.4 s | 52.8 s |
| Render chunk module | 87.2 s | 102.4 s |

Before extraction the stream edit recompiles `client-world` and both app targets.
After extraction its moved counterpart recompiles `chunk-pipeline` and both app
targets; `client-world`, `pack-compiler` and `render` remain fresh. Render edits
recompile `render` and both app targets in both phases, leaving world authority,
chunk scheduling and reusable compilation fresh after extraction.

An additional after-only edit to the lower world-ordering owner took 97.4 s.
It recompiles `client-world`, `chunk-pipeline` and both app targets. This demonstrates
the intended dependency direction: scheduling edits leave authority fresh, while
authority edits rebuild their scheduling consumer. The app remains a downstream
consumer in every case.

These are single samples on a shared machine, where other agents may compile at
the same time. Elapsed differences are observations, not an isolated or established
speedup. The compiled-unit lists provide the evidence for the new build boundaries.

## Reproduction and artifacts

- Before commit: `7251e86ed4b52f9b2921b91ed12e13521309043d`.
- After implementation commit: `37bfc75776eb978670b3b50240a0827f3cba9dc4`.
- Worktree: `/Users/hashim/Coding/cinnabar-wt/chunk-pipeline`.
- Stream source before: `crates/client-world/src/stream.rs`.
- Stream source after: `crates/chunk-pipeline/src/stream.rs`.
- Render source in both phases: `crates/render/src/chunk/mod.rs`.
- Additional lower world source after: `crates/client-world/src/ingestion/ordered.rs`.

Warm the executable build with the command below. Append a blank line and
`// Timing probe: behavior-neutral source edit.` to the selected source, build and
preserve Cargo's HTML timing report. Restore the exact original bytes and warm the
restored build before the next probe. Every invocation uses the required local
slot limiter and this worktree's own target directory. No probe is committed.

```sh
build_slot=/private/tmp/claude-501/-Users-hashim-Downloads-cinnabar/30f7cca3-953b-4833-843f-34f90cbe1dae/scratchpad/cslot
CARGO_BUILD_JOBS=3 "$build_slot" env RUSTC_WRAPPER= CARGO_INCREMENTAL=0 \
  CARGO_TARGET_DIR="$PWD/target" \
  cargo build --offline --locked -p bedrock-client --timings
```

The limiter sets `CARGO_PROFILE_DEV_DEBUG=0`. Clearing `RUSTC_WRAPPER` inside it
disables its default sccache wrapper. Cargo's elapsed times exclude the limiter's
queue wait; the recorded invocation wall times include it.

[Structured results](chunk-pipeline-build-timings.json) record the environment,
commands, real exit codes, compiled units, per-unit timings and report SHA-256
hashes. Complete reports, logs, `measure.py`, `measure_pipeline.py` and
`summarize.py` remain in `/private/tmp/cinnabar-chunk-pipeline-timings/`:

- [Before stream edit](/private/tmp/cinnabar-chunk-pipeline-timings/before-stream-edit.html)
- [After moved stream edit](/private/tmp/cinnabar-chunk-pipeline-timings/after-pipeline-edit.html)
- [Before render edit](/private/tmp/cinnabar-chunk-pipeline-timings/before-render-edit.html)
- [After render edit](/private/tmp/cinnabar-chunk-pipeline-timings/after-render-edit.html)
- [Additional lower world edit](/private/tmp/cinnabar-chunk-pipeline-timings/after-stream-edit.html)

The additional world report retains the runner's `after-stream-edit` label; the
source paths above and in the JSON distinguish it from the moved stream probe.
See [ownership, ordering and vanilla references](../architecture/chunk-pipeline.md)
and [reusable pack compilation](../architecture/pack-compiler.md). No client or
live server was run.
