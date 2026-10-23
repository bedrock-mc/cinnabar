# Verification and build caches

`cargo run -p devtool --locked -- verify-affected --base origin/dev` selects
checks for the files changed since the base. It runs every selected check and
reports all failures. A failure is accepted only when complete diagnostics show
that the same failure also occurs on the resolved base commit. Unknown output,
build failures and incomplete comparison evidence remain failures.

`tools/cargo-free check -p <package>` chooses an available build cache on Unix
hosts. It keeps the cache lock while Cargo runs, shares dependency artifacts,
and isolates workspace artifacts by Cargo workspace. It resolves the workspace
from the invocation directory or `--manifest-path`, including independent
workspaces in one Git checkout. Clippy uses a separate cache for each selected workspace.
`CARGO_FREE_BUILD_ROOT`, `CARGO_FREE_SLOTS` and `CARGO_FREE_CARGO`
configure the cache root, number of slots and Cargo executable.

Both tools require Python 3.11 or newer. They retain Cargo's environment and an existing
compiler wrapper from Cargo config or the environment. Config-relative wrapper
paths keep their original meaning. Nested invocations unwrap wrappers from either tool to their
recorded external target. Each generated wrapper stores that target in its own
file; different targets have different wrapper paths.

`cargo run -p devtool --locked -- check-web` checks `web-client` and `web-transport`
on `wasm32-unknown-unknown` and runs both browser JavaScript test suites. Install
that Rust target and Node.js first.

`cargo run -p devtool --locked -- build-web --assets <compiled-carriers> --out <new-directory> [--release]`
builds a static browser bundle from the workspace root. It uses the locked
wasm-bindgen version, installing a matching CLI under `target/web-tools` when needed.
The output includes the page, bindings, runtime carriers and canonical delivery
manifest; it must stay outside `web/site` and must not already exist. See
[relay setup](../../docs/web-relay.md) for HTTPS hosting and current limitations.

## Startup acceptance

`cargo run --locked -p devtool -- startup --client target/release/cinnabar --runs 100 --cache warm --out startup.json`
launches an acceptance-enabled release client, primes its driver cache, and reports
launch-to-presented-menu and per-stage medians and p99s. It fails above the 1 s warm
or 4 s cold p99 budget. Use `--baseline <report>` to identify stages that grew and
`--budget-ms` / `--cold-budget-ms` to select another hardware tier.

`--cache cold` gives Mesa a fresh driver-cache directory per launch. Other drivers
need `--cold-reset <program>` to reset their shader cache before each sample; this
option is required on macOS and Windows. `--cache both` measures both modes separately.
Arguments after `--` pass directly to the client. Run in a configured display or the
headless client environment on a hardware runner, separately from unit-test CI.
See the [release budgets](../../docs/agents/live-testing.md#frame-budgets).
