# Gameplay and session extraction: local rebuild measurements

These are paired edit-and-build samples for architecture step 4. They show no
consistent end-to-end rebuild improvement. The new domain libraries compile
quickly, but Cargo still recompiles and links the app after either library changes.

| Behavior-neutral edit | Before | After | Change |
| --- | ---: | ---: | ---: |
| Movement control modes | 37.36 s | 45.59 s | +8.23 s (+22.0%) |
| Session event pump | 48.55 s | 39.42 s | −9.13 s (−18.8%) |

Before: `01f54528f5b206d9dd12d6399ce48f74a4eabe03` (step 3 head).
After: `80b8c579ae1d92bd261113aaf99ed42f509078b0` (only a trailing blank line
in a test-only include differs from the measured snapshot). The implementation is described in
[gameplay-session.md](../architecture/gameplay-session.md).

## Method

Each sample appends a comment to one source file and runs the same executable
build. The comment is removed afterward. The before edits target
`app/src/movement/control_modes.rs` and `app/src/runtime/network/session/pump.rs`.
The after edits target `crates/gameplay/src/movement/control_modes.rs` and
`crates/client-session/src/session/pump.rs`.

Both revisions use a frozen source copy at
`/private/tmp/cinnabar-step4-timings/source` and its isolated target directory at
`/private/tmp/cinnabar-step4-timings/target`. A warm build precedes measurement;
the restored gameplay source is rebuilt before measuring the session edit so
that it does not add a dirty gameplay library to the second sample. Warm builds
and limiter wait time are excluded from the table. Source copies contain no
`.local` installation, credentials or Git metadata.

The command runs from the frozen source directory:

```sh
CARGO_BUILD_JOBS=3 \
/private/tmp/claude-501/-Users-hashim-Downloads-cinnabar/30f7cca3-953b-4833-843f-34f90cbe1dae/scratchpad/cslot \
  env RUSTC_WRAPPER= CARGO_INCREMENTAL=0 \
  CARGO_TARGET_DIR=/private/tmp/cinnabar-step4-timings/target \
  cargo build --offline --locked -p bedrock-client --timings
```

The limiter sets `CARGO_PROFILE_DEV_DEBUG=0`. Both builds use the workspace's dev
profile, three build jobs, disabled incremental compilation and no rustc wrapper.
The host is macOS 26.5.1 with rustc 1.93.1. These are single samples on a shared
machine; other local compilation overlapped. They establish observed rebuild
cost and which units rebuilt, not a statistically isolated performance change.

## Rebuilt units

| Sample | Domain library | App library | App executable | Fresh units |
| --- | ---: | ---: | ---: | ---: |
| Before movement | — | 35.62 s | 1.24 s | 669 |
| After movement | gameplay: 1.95 s | 43.14 s | 1.38 s | 670 |
| Before session | — | 46.46 s | 1.61 s | 669 |
| After session | client-session: 1.75 s | 37.39 s | 1.26 s | 670 |

Cargo pipelines some compilation, so unit durations need not sum to elapsed time.
The peer domain stays fresh in both after samples.

[Machine-readable measurements](gameplay-session-build-timings.json) retain the
Cargo unit records, exact elapsed values, commands and report SHA-256 hashes.
Raw local reports and logs are under
`/private/tmp/cinnabar-step4-timings/reports/{before,after}-{gameplay,session}-edit.*`.
Only the summary and numeric evidence are committed.
