# Repository agent instructions

Cinnabar is a Rust Bedrock client with a Go core. Match version-matched Bedrock behavior and output with original code. Use identified behavior references rather than memory or Java Edition assumptions. Label provisional approximations as incomplete in `plan.md`; they do not close a parity gate. Preserve accurate license notices and attribution.

## Maintainer setup

Before starting agent-assisted work, maintainers must clone [cinnabar-internal](https://github.com/bedrock-mc/cinnabar-internal#developer-setup) separately and follow its Developer setup for this Cinnabar clone. Read its `AGENTS.md` and `projects/cinnabar/AGENTS.md` alongside this file. The setup installs local agent instructions and publication checks; review hook trust, then restart agent sessions. Register each new clone; linked worktrees share its Git guards.

Keep internal notes and agent reports in the private checkout. Public implementation, tests, and contributor documentation belong here. Contributors without private access can build, test, and contribute using the public instructions below.

| Read | When |
| --- | --- |
| [Build workflow](docs/agents/multi-agent-workflow.md) | Worktrees, build limits, and verification |
| [Live testing](docs/agents/live-testing.md) | Rendered frames, profiling, and acceptance gates |
| [Client MCP](docs/agents/client-mcp.md) | Scripted client input and recordings |

## Behavior and architecture

- Choose algorithms, CPU/GPU work, data layout, and caching for the best measured performance. Parity constrains observable behavior, not implementation. Attribute every hitch caused by our code, including during joins and chunk loading. CI checks deterministic work; hardware captures check the frame, streaming, and join budgets in the live-testing guide.
- Render UI through the JSON-UI engine, with OreUI screens drawn in our own code. The built-in Java-look HUD pack sits below server packs. Bedrock player animations are the default, with Java Edition 1.7 animations as a Video-settings option; Java capes remain an accepted default; the open font is also an accepted deviation. Shipped art is original; game assets load at runtime from each install's fetched pack.
- Follow `tools/architecture/policy.toml` for crate boundaries, owned types, and dependency directions. Register new crates and edges. Keep domain crates Bevy-free and put logic in the lowest crate that owns its data. Refactors preserve behavior and tests, without temporary facades or re-export shims. Split production files by cohesive owner before the 1,000-line limit.
- Keep shared values in one constant, manifest, or generated file, including versions, hosts, carrier names, sizes, and `launcher::PRODUCT_NAME`. Consumers and tests must read that source instead of repeating literals.
- Reject malformed framing. Skip, count, and log odd but well-formed server data without disconnecting. Match Bedrock's lenient chunk decoding, and tolerate unknown data the client does not use.
- Require valid, hash-checked atmosphere, entity, HUD, and JSON-UI carriers at startup. Fail from `main` with the path and `make assets` rebuild command. Missing world or font carriers may use their diagnostic textures or font; optional carriers degrade gracefully.

## Tests and documentation

Reproduce bugs in the smallest owning crate and include a regression test that failed before each fix. Assert behavior, not source text, schedule names, wall-clock timing, or exact layout trees. Domain code takes `now` from its caller and only edge code reads the clock; tests never sleep, but wait with `test_time::eventually` (Rust) or `testwait`/`testing/synctest` (Go), which the architecture gate enforces. Tests requiring unavailable local carriers or fixtures skip with a named missing-fixture message, never `#[ignore]`.

Write short, clear documentation and comments. Give helpers documentation comments that explain their contract. Avoid comments that repeat signatures and process notes such as PR numbers or phase names. Keep `plan.md` and the architecture gate's completion ledger consistent with the implementation.

## Branches and verification

`dev` is the default and release branch. Normally open a PR into `dev` after `cargo check --tests -p <touched-crate>`. PR CI and review run in parallel; fix failures in new commits and merge only with clean review and green `ci-ok`. Do not repeat full-workspace checks, clippy, nextest, or `verify-affected` locally for PR work. Before an authorized direct push to `dev`, run `cargo run -p devtool --locked -- verify-affected --base origin/dev` once. Fix failures from scheduled `dev` CI promptly.

PR titles become the changelog: use an imperative summary without a `type:` prefix or issue numbers. Apply exactly one label: `new`, `fix`, `performance`, or `internal`. Check the label assigned by `pr-label.yml`. Merge by squash with the final title and body; keep branch commits descriptive. Report pushed, committed, tested but uncommitted, and in-progress work accurately.

## Worktrees, captures, and payloads

Use one worktree per concurrent change; each gets its own Cargo `build-dir` outside the checkout (`.cargo/config.toml`) and its own `target` for final artifacts. Bound parallel builds, use the configured shared build limiter when available, and never hold one build lock while waiting for another. Stop only processes you started, by PID; never use pattern-based kills. Remove a worktree's target directory after integration.

Capture visible changes with the offline UI snapshot harness or a headless client MCP session. Include before/after screenshots in the PR description; `tools/pr-screenshots.sh` can upload them. Keep real account details and server addresses out of captures. Do not commit game assets, BDS binaries, credentials, recordings, traces, or `.local/` carriers. Delete raw captures and traces after the PR is open.

Cinnabar's Gophertunnel integration uses `HashimTheArab/gophertunnel:resource-pack-changes`, based on `lunar`. Pin the module to a commit there, pull updates from `lunar`, and do not push to `lunar` unless explicitly asked.
