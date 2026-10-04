# Agent build, cache, and verification discipline

Use as many agents as help for reading and editing; only compiling is limited.

## Compile limits

Route every heavy cargo command through a shared slot limiter (an atomic `mkdir` lock over N slot
dirs with dead-PID reclaim; `flock` is not on macOS). On the owner's M3 Pro use **3** slots with
`CARGO_BUILD_JOBS=4`. Uncapped builds drove load to 60–75; 2 slots × 6 jobs left the machine half idle
while agents queued for hours. Build with `CARGO_PROFILE_DEV_DEBUG=0`, run sccache with a 30 GB cache
(the 10 GB default thrashes), prefer `cargo check -p <crate>`, and skip release builds in agent
worktrees. Restart sccache only when nothing is compiling.

## Worktrees and disk

- One Cargo `target` per active worktree; never share a `CARGO_TARGET_DIR` across branches. New
  worktrees may share a per-slot `CARGO_BUILD_BUILD_DIR`: dependencies compile once per slot, and
  distinct worktree paths keep workspace crates apart.
- Delete a worktree's `target` as soon as its work is integrated; each can reach 20–60 GB and
  the disk has filled twice.
- Never let an agent build into the owner's shared `.local`; build carriers into a scratch dir.
- An `Undefined symbols for architecture arm64` link error after a crash or full disk means a
  corrupt rlib: `cargo clean -p <crate>` and rebuild.

## Checks

For PR work, compile the touched crates, push, and open the PR; CI's full matrix and code review are
the gate and run in parallel. Don't rerun clippy, nextest or `verify-affected` locally on top of CI.
Before a direct push to `dev`, run `cargo run -p devtool --locked -- verify-affected --base origin/dev`
once. Read the real exit code — never let a pipe hide it, and never treat "the background job
finished" as "the check passed". A test that fails only on a network fetch or on stale local
carriers is an environment artifact: confirm by rerunning it, and say so.
