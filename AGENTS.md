# Repository agent instructions

Cinnabar is a Rust Bedrock client plus a Go core. The target is version-matched vanilla
Bedrock parity in every system — UI, rendering, controls, camera, movement, animation,
interaction, inventory, audio, protocol, timing. Base each behavior on an identified vanilla
reference, never on Java Edition, a custom aesthetic, or memory. A provisional approximation
may land only when labeled incomplete in `plan.md`; it never closes a parity gate.

| Load this | When |
| --- | --- |
| `docs/agents/multi-agent-workflow.md` | Worktrees, build limits, verify-before-push |
| `docs/agents/live-testing.md` | Running the client or BDS, capturing frames, closing a visual/performance gate |

## Parity sources and UI

Client references — the 26.30 reconstruction, Lens, and the vanilla packs — may be used
directly: constants, geometry, formulas, layouts, behavior. Write our own code from them; never
paste decompiled source. All UI renders through the JSON-UI engine (OreUI screens are drawn in
our own code), including the HUD and container screens. The one styling exception is the HUD's
built-in Java-look pack, which sits below server packs so they override it as on Bedrock. The
open font is an accepted deviation. Our shipped art (panorama, logo) is original; Mojang assets
are only loaded at runtime from the pack each install fetches.

## No hardcoded duplicates

A value that must stay in sync — game/protocol version, pinned pack, registry versions, service
hosts, carrier names, shared sizes — has exactly one source of truth (a constant, manifest, or
generated file) that everything else imports or reads. Never restate it as a literal, including
in UI text and tests.

## Server data: lenient

Malformed framing (truncation, bad lengths, envelope failures) is fatal. Odd but well-formed data
(unknown slot, sentinel id, non-finite float, unknown metadata, custom world height) is skipped,
counted and logged; the session stays up. Chunk payloads follow the vanilla client's lenient
decode exactly. Never disconnect over data the client doesn't use.

## Required carriers: fail closed

Startup requires the atmosphere, entity, HUD and JSON-UI carriers. If one is missing, malformed
or fails its pinned hash, abort from `main` naming the carrier path and its rebuild command
(`make assets`). Never hide required art behind a log line. Exceptions: no world carrier selects
diagnostic textures; no compiled font selects the diagnostic font. New optional carriers degrade
gracefully until startup truly requires them.

## Gophertunnel

Cinnabar's Gophertunnel work lives on `HashimTheArab/gophertunnel:resource-pack-changes` (based on
`lunar`); pin the Go module to a commit on it. Pull `lunar` into it; never push to `lunar` unless
asked.

## Git and payloads

Mojang assets, screenshots, recordings, `.local/` carriers, credentials and BDS binaries never
enter git. Use `git worktree`, one Cargo `target` per active worktree, and delete a worktree's
`target` once its work is integrated.

## Verify before pushing

Land through a PR into `dev`; its CI runs the full matrix, so don't run full-workspace sweeps
locally. Before pushing run `cargo run -p devtool --locked -- verify-affected --base origin/dev`:
fmt, the architecture gate (line limits, test-only public API, markers), clippy and nextest,
scoped to the affected crates. Merge only on green CI.

## Report state precisely

Distinguish pushed, locally committed, test-green uncommitted, and in-progress work.
