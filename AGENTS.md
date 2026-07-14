# Repository agent instructions

## Bevy client screenshots on Windows

- Computer Use/WGC currently cannot capture the Cinnabar Bevy window reliably.
- For live visual verification, capture the desktop with the native Windows GDI `CopyFromScreen` method and write PNG files beneath `%TEMP%`.
- Inspect those temporary PNGs with the image-viewing tool. Do not rely on stale Computer Use observations or claim visual verification without inspecting a fresh capture.
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
