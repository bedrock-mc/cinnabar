# Repository agent instructions

Cinnabar is a Rust Bedrock client plus Go server-side tooling. The product target
is version-matched vanilla Bedrock parity — not a functional approximation —
across UI/HUD, rendering, controls, camera, movement and physics, animation,
interaction, inventory, audio, protocol behavior, timing, and server-authoritative
reconciliation. Establish each parity contract from an identified authoritative
vanilla Bedrock reference; Java Edition behavior, a custom aesthetic, and
remembered behavior are not references. A provisional approximation may be
committed only when labeled incomplete in `plan.md` and in user-facing status, and
it never closes a vanilla acceptance gate.

## Gameplay HUD: Java Edition parity exception

By explicit owner decision, the gameplay HUD — chat, scoreboard, hearts and other
status bars, crosshair, boss bars, hotbar, and the rest of the in-game HUD — targets
Java Edition appearance and layout parity. This is a scoped exception; version-matched
Bedrock parity still governs everything else. Bedrock textures remain acceptable, and
the open font is an accepted permanent deviation for copyright reasons. Decompiled
Java sources (for example, `mcsrc.dev`) may be consulted only to understand behavior.
Never copy or paraphrase decompiled code, identifiers, class or method names, or
literal constants into this repository. Express findings as observed behavior or
layout facts; prefer running-game screenshots or public documentation as citable
references. A value available only from decompiled source must be marked as needing
independent measurement, not quoted.

These are the things that are not obvious from the code. Read the linked docs when
the work touches them.

| Load this | When |
| --- | --- |
| `docs/agents/multi-agent-workflow.md` | Coordinating subagents, worktrees, review, integration, or pushing |
| `docs/agents/live-testing.md` | Running the client or BDS on Windows, capturing frames, closing a native/visual/performance gate |

## Remote server data: be lenient, not strict

Inbound server data is untrusted and imperfect. Malformed *wire* (truncation, bad
lengths, decode failures) is fatal. A semantically odd but well-formed packet
(unexpected slot, sentinel id, non-finite float, unknown metadata key, custom world
height) is not: skip that packet/field, log/count it, keep the session alive. Never
disconnect over data the client doesn't even use.

## Required local assets: fail closed at startup

The production runtime requires the compiled atmosphere, entity, and HUD carriers.
If one is missing, unreadable, malformed, or fails its pinned hash, abort via
`bail!`/`?` → `main`, naming the exact carrier path and its rebuild command
(`make hud-assets`, or `make assets` for all). Carriers live under gitignored
`.local/`, so `git pull` never delivers them and the fatal error is what tells a
developer to rebuild. Never skip or hide required player-facing art; a blank HUD
with only a log line is the failure this forbids. Two documented exceptions remain:
an absent world carrier selects the programmatic diagnostic-texture mode, and an
absent compiled font selects the bounded diagnostic font fallback. Apply the
required-carrier rule to a future carrier only once production startup actually
requires it.

## Gophertunnel branch ownership

Cinnabar-specific Gophertunnel work belongs on
`HashimTheArab/gophertunnel:cinnabar`, which is based on `lunar`. Never push
Cinnabar changes directly to `lunar`; pull useful `lunar` updates into `cinnabar`,
and keep Cinnabar's Go module pinned to an exact commit reachable from `cinnabar`.
Promote a change back to `lunar` only on an explicit user request.

## Rust build-cache discipline

Keep each concurrently active worktree on its own Cargo `target` directory — a
shared `CARGO_TARGET_DIR` lets Cargo file locks and path-based fingerprints reuse
incompatible local crate artifacts across branches. Share compiler results through
the installed `sccache` (this machine disables incremental compilation and caps the
cache at 20 GiB). Delete a worktree's reproducible `target` after its commit is
reviewed and integrated, preserving the canonical checkout's
`target/debug/bedrock-client.exe` and any actively compiling agent's directory. Use
`git worktree` rather than another full clone.

## Keep local payloads out of git

Mojang assets, screenshots, recordings, generated `.local/` carriers, credentials,
and BDS binaries never enter git. Store captures under temporary or ignored paths
and commit only compact lawful rules, provenance and checksums, and independently
authored evidence descriptions.

## Report state precisely

Distinguish pushed work, locally committed work, test-green uncommitted work, and
work still in progress. Never describe locally committed or merely reviewed work as
pushed or integrated.
