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
