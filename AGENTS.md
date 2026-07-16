# Repository agent instructions

## Bevy client screenshots on Windows

- Use native Computer Use/WGC as the primary path for Cinnabar window inspection and input testing.
- Do not assume the Bevy window is inaccessible from an earlier failure. Refresh app/window discovery for each live run and diagnose a missing target as a current integration bug.
- If native capture genuinely fails after fresh discovery and recovery, use Windows GDI `CopyFromScreen` only as an explicit fallback, write PNG files beneath `%TEMP%`, and inspect those fresh files with the image-viewing tool.
- Never claim visual verification from a stale or occluded capture.
- Keep Mojang assets and all screenshots out of git.

## Stable Windows live-test executable paths

- Reuse `.local/bds-runtime/bedrock-server-1.26.32.2/bedrock_server.exe` for
  BDS live tests; this is the copied executable path the user already approved
  in Windows Firewall.
- Launch the Rust client from the stable Cargo output
  `target/debug/bedrock-client.exe`. Rebuilding in place keeps the executable
  path stable.
- Do not copy either executable to a new worktree or temporary path for a live
  run. Windows Firewall consent is path-specific and a new path can prompt
  again.
- Do not change firewall policy or automate UAC/security-consent dialogs. If a
  genuinely new listening executable is required, explain why and wait until
  the user is at the PC.

## Gophertunnel branch ownership

- Cinnabar-specific Gophertunnel work belongs on
  `HashimTheArab/gophertunnel:cinnabar`, which is based on `lunar`.
- Never push Cinnabar changes directly to the `lunar` branch. Pull useful
  `lunar` updates into `cinnabar`, then keep Cinnabar's Go module pinned to an
  exact commit reachable from `cinnabar`.
- Move a generally useful Cinnabar change back to `lunar` only when the user
  explicitly requests that promotion.

## Throughput and evidence discipline

- Prioritize implementation of plan-critical functionality over repeated
  polishing of already-correct per-family evidence.
- For each implementation tranche, use one focused independent review cycle.
  Fix all Critical and Important findings, but do not start additional review
  loops unless a fix materially changes production behavior.
- Reuse an existing authoritative native checkpoint when it covers the exact
  state product and geometry/material questions. Do not recapture equivalent
  views merely to improve presentation.
- Batch native screenshots, matching-view GPU witnesses, and visual polish at
  the deterministic gallery/live-acceptance gate whenever they are not needed
  to decide the implementation contract.
- Keep independent implementation lanes parallel when they use separate
  worktrees or have no shared-write conflict. Merge or cherry-pick only after
  each lane is green and reviewed.
- Report status precisely: distinguish pushed work, locally committed work,
  test-green uncommitted work, and work that is only in progress.

## Rust build-cache discipline

- Keep each concurrently active Git worktree on its own Cargo `target`
  directory. Never point divergent worktrees at one shared `CARGO_TARGET_DIR`:
  Cargo file locks and path-based fingerprints can reuse incompatible local
  crate artifacts across branches.
- Share compiler results through a bounded `sccache` instead. On this Windows
  development machine the user Cargo configuration disables incremental
  compilation, uses the installed `sccache`, and caps it at 20 GiB.
- Delete a worktree's reproducible `target` directory after its commit is
  reviewed and integrated. Preserve the canonical checkout's stable
  `target/debug/bedrock-client.exe` and the target directories of agents that
  are still actively compiling or testing.
- Do not create another full clone merely to isolate a feature. Use `git
  worktree`, and keep Mojang assets/BDS runtimes in ignored local storage rather
  than copying them into every worktree.
