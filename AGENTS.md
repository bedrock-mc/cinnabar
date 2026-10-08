# Repository agent instructions

Cinnabar is a Rust Bedrock client plus a Go core. Every system (UI, rendering, controls, camera, movement, animation, interaction, inventory, audio, protocol and timing) must match version-matched vanilla Bedrock in observable behaviour and output. Base each behaviour on an identified vanilla reference, never on Java Edition, a custom aesthetic, or memory. A provisional approximation may land only when it's labelled incomplete in `plan.md`, and it never closes a parity gate.

| Load this | When |
| --- | --- |
| `docs/agents/multi-agent-workflow.md` | Worktrees, build limits, checks |
| `docs/agents/live-testing.md` | Running the client or BDS, capturing frames, closing a visual/performance gate |
| `docs/agents/client-mcp.md` | Scripting the client or recording video through the MCP server |

## Performance: as fast as we can make it

Parity covers what the player sees and how the game behaves, never how we compute it. Choose every implementation, including CPU vs GPU, data layout, parallelism and caching, for the best performance achievable, not just better than vanilla. The bar is a world that streams in faster than the player can see, so flying at any speed shows no loading edge or pop-in, frames stay smooth with no stutter, and joins are near-instant. "Vanilla does it this way" never justifies a slower design. Measure before and after any performance claim. Any frame our code makes miss 1.5× the refresh interval is a hitch and a bug, including during joins and chunk loading: attribute it and fix it. CI asserts deterministic work (no allocations, rebuilds or uploads for unchanged input), never milliseconds; frame, streaming and join budgets are checked in hardware captures against `docs/agents/live-testing.md`.

## Parity sources

Vanilla behaviour, constants, geometry, formulas, layouts and vanilla packs may be used directly, as long as we write our own code. Never paste decompiled source. Committed files (code, comments, docs, commit messages, PR text) never name where behaviour was reverse-engineered from: no decompilation repos or paths, reconstruction line references, function addresses, RVAs or executable hashes. Describe the vanilla behaviour itself, for example as a "Vanilla rules" table. Vanilla pack file paths are fine. The one exception is `docs/agents/vanilla-refs-map.md`, the agent index mapping files to the vanilla code they match; add references there, never in code or other docs. Java Edition 1.7 player animations and capes are an owner-mandated exception to Bedrock parity: a built-in default, switchable to vanilla in Video settings.

## UI

All UI renders through the JSON-UI engine (OreUI screens are drawn in our own code), including the HUD and container screens. The one styling exception is the HUD's built-in Java-look pack, which sits below server packs so they override it as on Bedrock. The open font is an accepted deviation. Our shipped art (panorama, logo) is original. Mojang assets are loaded only at runtime, from the pack each install fetches.

## Architecture

Crate layering is enforced by `tools/architecture/policy.toml`, covering dependency directions, owned types and module boundaries. Register every new crate or dependency edge there. Domain crates stay Bevy-free. Put code in the lowest crate that owns its data rather than in `app`. Refactors move behaviour unchanged first, with the existing tests green, and never leave temporary facades or re-export shims behind. Production files stay under the gate's 1,000-line limit: split by cohesive owner, not to dodge the count.

## No hardcoded duplicates

A value that must stay in sync has exactly one source of truth (a constant, manifest or generated file) that everything else imports or reads. Examples: the game and protocol version, the pinned pack, registry versions, service hosts, carrier names, shared sizes, and the product name (`launcher::PRODUCT_NAME`). Never restate it as a literal, including in UI text and tests.

## Server data: lenient

Malformed framing (truncation, bad lengths, envelope failures) is fatal. Odd but well-formed data (an unknown slot, a sentinel ID, a non-finite float, unknown metadata, a custom world height) is skipped, counted and logged, and the session stays up. Chunk payloads follow the vanilla client's lenient decode exactly. Never disconnect over data the client doesn't use.

## Required carriers: fail closed

Startup requires the atmosphere, entity, HUD and JSON-UI carriers. If one is missing, malformed or fails its pinned hash, abort from `main`, naming the carrier path and its rebuild command (`make assets`). Never hide required art behind a log line. Exceptions: with no world carrier, use diagnostic textures; with no compiled font, use the diagnostic font. New optional carriers degrade gracefully until startup truly requires them.

## Tests

Reproduce a bug first in the smallest crate that owns the logic; JSON-UI and domain crates test in seconds. Every bug fix carries a regression test that failed before the fix. Tests that need local carriers or fixtures skip with a named missing-fixture message and are never `#[ignore]`d. Never pin source text, schedule spelling, wall-clock timing or exact layout trees; assert behaviour.

## Comments and docs

Keep comments to one or two lines that say what the code can't. Never restate signatures, and never leave process artefacts in code: no PR or issue numbers, phase names or "temporary" notes. Use doc comments for contracts and invariants. Prose docs stay short and point at the full document.

## Branches and landing

`dev` is the default and release branch; releases are tagged from it. Land work through a PR into `dev`: compile the crates you touched (`cargo check --tests -p …`), push, and open the PR straight away. PR CI (a Linux gate plus a Windows/macOS platform subset, aggregated as `ci-ok`) and code review run in parallel on it and are the gate; fix what they report in new commits, and merge once review is clean and CI is green. Don't repeat CI locally: no clippy, nextest or `verify-affected` ladders for PR work, and never full-workspace sweeps. A direct push to `dev` has no PR CI, so run `cargo run -p devtool --locked -- verify-affected --base origin/dev` once before it. Scheduled CI runs the full three-OS matrix on `dev` (cache warm-up hourly, full runs every 6 hours), and the pusher fixes a red result immediately.

PR titles are the changelog: an imperative, player-facing summary starting with a verb (Add, Fix, Reduce, …), with no `type:` prefix and no PR or issue numbers. Every PR carries exactly one of the labels `new`, `fix`, `performance` or `internal`; `internal` covers CI, tests, docs, tooling and refactors players can't see. `pr-label.yml` sets the label from the title, and you correct it when it's wrong. Merge by squash only (the repo allows nothing else); the squash commit takes the PR title as its subject and the body as its message, so make both final before merging. Branch commit messages stay descriptive, never `wip` or `fix`.

## Parallel work: minimise bottlenecks

When many agents run at once, compiling is the scarce resource. Route every cargo command through the shared build-slot limiter, run each check once, and hand verification to CI as early as possible instead of queueing local re-checks. Never hold a build slot or lock while waiting on another. Stop only processes you started, by PID: never `pkill`, `killall` or other pattern kills, which also match other agents' and builds' command lines (`.claude/settings.json` blocks them for Claude sessions). Details are in `docs/agents/multi-agent-workflow.md`.

## Gophertunnel

Cinnabar's Gophertunnel work lives on `HashimTheArab/gophertunnel:resource-pack-changes`, based on `lunar`; pin the Go module to a commit on it. Pull `lunar` into it, and never push to `lunar` unless asked.

## Git and payloads

Mojang asset files, recordings, `.local/` carriers, credentials and BDS binaries never enter git. Every PR that changes anything visible shows before/after screenshots: capture them with the offline UI snapshot harness or headless through the client MCP (`docs/agents/client-mcp.md`), never a visible window, keep real gamertags, emails and server addresses out of frame, and add them to the PR description (agents: `tools/pr-screenshots.sh <pr> <png>...` uploads and embeds them, since native attachments are web-only). Delete raw captures and traces once the PR is open. Use `git worktree`, with one Cargo `target` per active worktree, and delete a worktree's `target` once its work is integrated.

## Report state precisely

Distinguish pushed, locally committed, test-green uncommitted, and in-progress work.
