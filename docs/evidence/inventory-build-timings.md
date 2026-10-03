# Inventory authority build measurements

These measurements compare edits to inventory authority and UI presentation before
and after restructuring step 2. They build the app executable, including its library
and final link. They are not runtime or clean-build benchmarks.

## Results

Measured on the same local Apple M3 Pro (12 cores, 36 GB), macOS 26.5.1,
`aarch64-apple-darwin`, Rust 1.93.1. Both phases use the repository's dev profile:
workspace optimization level 1, dependency optimization level 3, three Cargo jobs,
no debug information, no incremental compilation and no compiler cache.

| Edit and resulting executable rebuild | Before | After |
| --- | ---: | ---: |
| Inventory ledger edit | 70.9 s | 60.9 s |
| UI presentation edit | 70.7 s | 53.8 s |
| Compiled units after inventory edit | 2 | 3 |
| Compiled units after UI edit | 2 | 2 |

Before extraction, either edit compiles the app library and executable. After
extraction, the inventory edit compiles inventory plus the app library and
executable. The UI edit compiles only the app library and executable; inventory,
protocol, client-world and render stay fresh. An inventory edit still rebuilds the
app because the app consumes inventory; this step does not promise independent app
linking or eliminate that downstream compilation.

The times above come from Cargo's HTML reports and exclude the shared limiter's
queue time. There is one sample per case. Other agents were compiling on the same
machine, so these elapsed-time differences are observations, not an isolated or
statistically established speedup. The compiled-unit lists directly demonstrate
that UI edits leave the new inventory crate fresh.

## Reproduction and artifacts

- Before commit: `792e7adbc6aed482609b6626cef56e7ae8452c54` (step 1 complete).
- After commit: `676ff287a89afdf2856c42ba161da579c0242d0d`.
- Worktree: `/Users/hashim/Downloads/cinnabar-wt/render-api`.
- Warm the app with the command below. Append a blank line and
  `// Timing probe: behavior-neutral source edit.` to the inventory source, build,
  save the timing report, and restore the original bytes. Build once more to warm
  the restored source before doing the same edit/build/restore for UI.
- Inventory source before: `app/src/ui_runtime/inventory_ledger.rs`; after:
  `crates/inventory/src/inventory_ledger.rs`.
- UI source in both phases: `app/src/ui_runtime/presentation.rs`.
- No probe is committed, and no target directory is shared with another worktree.

```sh
build_slot=/private/tmp/claude-501/-Users-hashim-Downloads-cinnabar/30f7cca3-953b-4833-843f-34f90cbe1dae/scratchpad/cslot
CARGO_BUILD_JOBS=3 "$build_slot" env RUSTC_WRAPPER= CARGO_INCREMENTAL=0 \
  CARGO_TARGET_DIR="$PWD/target" \
  cargo build --offline --locked -p bedrock-client --timings
```

The limiter sets `CARGO_PROFILE_DEV_DEBUG=0`. Clearing `RUSTC_WRAPPER` after entering
the limiter disables its default sccache wrapper. Preserve
`target/cargo-timings/cargo-timing.html` after each invocation.

[Structured results](inventory-build-timings.json) contain each compiled unit,
report hashes, command, environment and real exit status, with limiter-inclusive
wall time stored separately. The complete HTML reports, logs, `measure.py` runner
and `summarize.py` parser remain in `/private/tmp/cinnabar-inventory-timings/`:

- [Before inventory edit](/private/tmp/cinnabar-inventory-timings/before-inventory-edit.html)
- [After inventory edit](/private/tmp/cinnabar-inventory-timings/after-inventory-edit.html)
- [Before UI edit](/private/tmp/cinnabar-inventory-timings/before-ui-edit.html)
- [After UI edit](/private/tmp/cinnabar-inventory-timings/after-ui-edit.html)

See the [migration, ordering and vanilla references](../architecture/inventory.md).
No client or live game server was run.
