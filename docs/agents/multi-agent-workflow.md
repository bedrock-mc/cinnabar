# Agent build, cache, and verification discipline

Use as many agents as help for reading and editing; only compiling is limited.

## Compile limits

Route every heavy cargo command through a shared slot limiter (an atomic `mkdir` lock over N slot
dirs with dead-PID reclaim; `flock` is not on macOS). On the owner's M3 Pro use **2** slots with
`CARGO_BUILD_JOBS=6` — more drove load to 60–75 and stalled the machine. Build with
`CARGO_PROFILE_DEV_DEBUG=0`, prefer `cargo check` and focused `cargo test -p <crate>`, run the
workspace suite once at the end, and skip release builds in agent worktrees.

## Worktrees and disk

- One Cargo `target` per active worktree; a shared `CARGO_TARGET_DIR` lets fingerprints reuse
  incompatible artifacts across branches.
- Delete a worktree's `target` as soon as its work is integrated; each can reach 20–60 GB and
  the disk has filled twice.
- Never let an agent build into the owner's shared `.local`; build carriers into a scratch dir.
- An `Undefined symbols for architecture arm64` link error after a crash or full disk means a
  corrupt rlib: `cargo clean -p <crate>` and rebuild.

## Verify before pushing

Locally run `cargo run -p devtool --locked -- verify-affected --base origin/dev` (fmt, the
architecture gate, clippy and nextest scoped to affected crates); the full matrix runs on the PR
into `dev`, and merges wait for green CI. Read the real exit code — never let a pipe hide it, and never
treat "the background job finished" as "the check passed". A test that fails only on a network
fetch or on stale local carriers is an environment artifact: confirm by rerunning it, and say so.
