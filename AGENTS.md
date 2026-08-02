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

These are the things that are not obvious from the code. Read the linked docs when
the work touches them.

| Load this | When |
| --- | --- |
| `docs/agents/multi-agent-workflow.md` | Coordinating agents or Luna CLI worker threads, worktrees, review, integration, or pushing |
| `docs/agents/live-testing.md` | Running the client or BDS on Windows, capturing frames, closing a native/visual/performance gate |

## Luna workers are Codex CLI threads, not subagents

The in-process agent runtime used for this repository cannot select Luna for a
subagent. For bounded, dependency-independent Luna work, launch a separate,
persistent Codex CLI session with `codex exec` and call it a **worker thread**,
not a subagent. Do not use `--ephemeral`; capture JSONL stdout (whose
`thread.started` event contains the session ID) and the final report so the
coordinator can inspect the evidence or continue it with
`codex exec resume <session-id>`.

Luna is a capable focused coding and reasoning worker, not merely a mechanical
scanner. Use it for deep repository tracing, bug/test/log triage,
dependency/protocol mapping, bounded correctness analysis, and independent
checking of a well-specified change. It is not the authority for ambiguous
architecture, vanilla parity contracts, high-stakes integration decisions, or
final acceptance.

Pin every Luna thread to `gpt-5.6-luna` and enable Fast mode. `xhigh` is the
capable default for normal bounded work. `max` is a meaningful quality step and
should be preferred when the task has subtle cross-file reasoning, competing
hypotheses, protocol ambiguity, or a costly false conclusion; accept its extra
latency rather than treating it as interchangeable with `xhigh`. These are
Cinnabar routing judgments, not a claim that either effort exactly equals a
GPT-5.4 or GPT-5.5 effort level. The canonical PowerShell form is:

```powershell
codex exec -C <worktree> -m gpt-5.6-luna -s read-only --json -o <report-path> -c 'model_reasoning_effort="xhigh"' -c 'service_tier="fast"' -c 'features.fast_mode=true' '<self-contained assignment>'
```

Substitute `max` only deliberately. Never silently run a Luna worker at Standard
speed; if the installed CLI, current authentication, or model catalog does not
offer Fast mode, report the worker unavailable. Apply the decomposition,
evidence, worktree, and reporting contracts in the multi-agent workflow. Luna
threads gather evidence for the root `gpt-5.6-sol` `high` coordinator; they do
not make shipping edits, give final approval, integrate, push, or close a
vanilla acceptance gate. The Sol-high coordinator must review Luna's evidence
before relying on it. When the workflow requires an independent gate, spawn a
fresh `gpt-5.6-sol` `high` reviewer instead of treating coordinator review as
independent review.

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

## Verify before pushing, not after

CI is a backstop, not a compiler. Before pushing a shared branch, run what the push
will trigger: the focused tests, `cargo fmt --all`, clippy, and
`cargo run -p architecture -- check --root . --policy tools/architecture/policy.toml`.
The architecture gate is the one most often skipped and the one that most often
fails, because nothing else catches its rules — per-file line limits, forbidden
test-only public API, and marker registration are invisible to every test.

When local verification is genuinely unavailable — a machine killing builds, a
sandbox denying the toolchain — that does not convert CI into the check. Say the
state is unverified, and either wait, hand the run to the user, or push while
labeling it unverified in the same breath. A red CI run must never be the first
thing that discovers whether the code compiles.

## Report state precisely

Distinguish pushed work, locally committed work, test-green uncommitted work, and
work still in progress. Never describe locally committed or merely reviewed work as
pushed or integrated.
