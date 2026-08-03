# Multi-agent workflow

Load this when coordinating agents or Luna CLI worker threads on a non-trivial
implementation, protocol, asset, rendering, performance, or integration tranche.
For read-only diagnosis or review, apply only the preflight, evidence, and
reporting portions; do not create branches, edit, commit, integrate, or push
unless the request authorizes changes.

## Model routing

Model routing is a quality and throughput decision, not a price ranking. The
user's OpenAI subscription has generous limits, so correctness and verified
completion outrank list price.

| Agent role | Default model | Reasoning | Use and limits |
| --- | --- | --- | --- |
| Root coordinator and integrator | `gpt-5.6-sol` | `high` | Own decomposition, architectural consistency, synthesis, integration, review of worker evidence, and the final completion decision. |
| Implementation writer | `gpt-5.6-sol` | `medium` | Use for all production code, behavior changes, refactors, tests tied to changed behavior, and fixes that may ship. Run at most two isolated writers concurrently. |
| Independent reviewer | `gpt-5.6-sol` | `high` | Use after implementation for architecture, correctness, protocol, concurrency, performance, security, and player-visible review. This must be a fresh agent when independence is required. |
| Read-only explorer | `gpt-5.6-luna` | `xhigh` or `max` | Use a Fast Codex CLI worker thread for repository mapping, targeted research, dependency tracing, test/log triage, and other bounded read-only investigations. Use `max` for subtle or ambiguity-heavy work. Return concise evidence to the Sol-high coordinator; do not make shipping edits or give final approval. |
| Mechanical read-only worker | `gpt-5.6-luna` | `xhigh` | Use a Fast Codex CLI worker thread for inventories, classification, extraction, and repetitive scans with an explicit output schema. Escalate on ambiguity. |

The normal starting topology is one Sol-high coordinator, zero to two isolated
Sol-medium writers, zero to two Luna read-only workers, and one fresh Sol-high
reviewer after the writing tranche; do not spawn every slot merely because it is
available. Do not route work to `gpt-5.6-terra` — escalate Luna misses directly
to Sol. These are defaults, not ceilings: rerun weak work at Sol or higher effort
without asking, because escalation is cheaper than integrating mediocre work.
For non-coordinator Sol work, escalate above medium only when it is genuinely
architectural, unsafe, protocol-ambiguous, concurrency-sensitive, cross-cutting,
or difficult review work. For UI, HUD, copy, public APIs, and other
player/developer-facing work, use Sol and enforce the applicable visual or API
acceptance bar. Benchmark routes on representative
Cinnabar tasks; generic intelligence charts are inputs, not proof of reliability
here. When the runtime exposes model and reasoning controls, use the exact model
and effort above; if controls are unavailable, use the runtime default, record
that explicit selection was unavailable, and continue. Never claim a model
selection the runtime did not expose, and never build a proxy agent to disguise
one model as another. Dispatch only as many subagents as there are bounded,
dependency-independent, non-overlapping lanes; keep the root agent free to
coordinate and integrate, and do not turn Cargo/Go build contention into the
bottleneck.

## Preflight and decomposition

1. Read `AGENTS.md`, the relevant section of `plan.md`, and the files directly
   involved. Inspect `git status`, the current branch, recent commits, and
   `git worktree list` before any mutation.
2. State the exact acceptance condition. Implementation completion, test
   completion, native/live acceptance, performance acceptance, and phase closure
   are separate gates.
3. Split broad work into the smallest independently testable tranches a reviewer
   could approve or reject on their own. Prefer reusable family-level or
   data-driven solutions over repeated one-off fixes.
4. Parallelize only explicitly disjoint tranches. Every writing agent needs a
   coordinator-provisioned dedicated linked worktree, unique task branch, and
   non-overlapping file ownership. Keep dependent tasks sequential; never let two
   agents edit the same branch/worktree concurrently.
5. Give every subagent a self-contained assignment containing: for writers, the
   exact repository, worktree, branch, and base commit; for reviewers, the exact
   repository, base/head SHAs, and any coordinator-provisioned detached review
   worktree; the bounded goal and why it matters; binding requirements and
   explicit out-of-scope changes; the source-of-truth and acceptance evidence;
   exact focused verification expectations; for writers, `commit only; do not
   push, merge, switch the authoritative worktree, or edit plan.md/AGENTS.md
   unless explicitly assigned`; for reviewers, `inspect only; do not edit,
   commit, push, merge, or switch branches`; and the required report — status,
   commit hash, changed behavior, commands and results, open risks, and remaining
   native gates.

## Worktree and ownership

The root agent owns the authoritative integration worktree. Before dispatch the
coordinator runs `git worktree list`, then creates a missing writing worktree with
`git worktree add -b <branch> <path> <base>`; do not create or remove worktrees
concurrently. Writing subagents must stay in their assigned linked worktree and
task branch, and must not switch branches in the authoritative worktree.
Reviewers inspect commit objects without mutating a working tree, or use a
coordinator-provisioned detached clean worktree at the exact reviewed head; do
not check out an implementer's branch in a second worktree. Record the task base
commit before dispatch and review the entire `base..head` range, never an assumed
`HEAD~1`. Only the root integrator merges or cherry-picks, updates the
authoritative plan, pushes shared branches, and monitors CI. Never force-push,
reset, or otherwise rewrite protected or shared history; rewrite an unshared task
branch only on an explicit user request naming the exact operation and branch.
Preserve unrelated user changes in dirty worktrees — stop and report a real
overlap instead of discarding it.

## Implementation contract

1. Establish the behavior contract from authoritative data or evidence. Never
   turn an inference into a claimed vanilla/protocol fact.
2. Use TDD for behavior changes: add a focused failing regression or conformance
   witness, observe the expected failure, implement the smallest correct change,
   and observe it pass.
3. Keep runtime work bounded and fail closed on malformed, ambiguous, stale,
   unsupported, or unproven data. Preserve Cinnabar's palette-native,
   allocation-bounded, and version/provenance-pinned architecture.
4. Run the focused tests exercising the changed contract, plus formatting, strict
   warnings/clippy or vet, applicable architecture checks, and `git diff --check`.
   Run broader suites in proportion to integration risk.
5. Self-review the complete diff for scope creep, duplicated logic, unchecked
   limits, guessed semantics, stale provenance, accidental assets, and missing
   negative tests. Commit locally with an intentional message and leave the task
   worktree clean.

## Independent review loop

1. Dispatch a fresh reviewer that did not implement the task. Give it the task
   contract, implementer report, and complete `base..head` diff. Reviewers
   inspect only.
2. Require an explicit `APPROVE` or `NEEDS CHANGES` decision with findings
   classified Critical, Important, or Minor and tied to concrete files, behavior,
   and evidence.
3. Send every Critical and Important finding back to an implementer/fixer, with
   focused regression coverage and fresh verification for the fix.
4. The integrator verifies every Critical and Important finding is dispositioned.
   Dispatch one fresh re-review of the complete `base..<new-head>` range only when
   a fix materially changes production behavior or the evidence contract;
   non-material corrections need fresh focused verification and a recorded
   disposition, not another cycle. A self-review, green suite, or plausible native
   screenshot does not replace the independent gate.
5. Record Minor findings for final review. Do not loop on wording or presentation
   changes that cannot affect behavior.

## Integration, verification, and pushing

1. Record the approved head SHA, verify the task worktree is clean and free of
   prohibited files, and immediately before integration verify the task branch
   still resolves to the approved SHA. Integrate exactly that reviewed SHA/range
   without flattening history; if the branch moved, review the new range first.
2. Run fresh post-integration verification covering the changed crates/modules and
   their consumers. For cross-cutting changes run the full workspace/Go suites,
   strict linters, architecture enforcement, acceptance harnesses, and
   `git diff --check` as applicable.
3. Update `plan.md` in the integration branch with evidence-backed language. Mark
   a checkbox complete only when its implementation, native/live, performance, and
   review gates are all satisfied; otherwise record the landed tranche and the
   exact remaining gate.
4. Push only when the current task explicitly authorizes the target remote branch;
   otherwise stop at a clean local integration commit and report that state.
   Before pushing, fetch and verify the intended remote ref and that the push is
   fast-forward. On divergence, stop and reconcile without rewriting shared
   history. After pushing, verify the remote ref resolves to the expected commit
   and monitor the resulting CI run.
5. Report the exact pushed commit, verification commands and results, review
   decision, CI state, user-visible unlock, and remaining work.
6. Treat a CI-caused correction as a bounded fix tranche: reproduce, add focused
   coverage, implement and verify, independently re-review material behavior or
   evidence changes, integrate the approved head, push an authorized commit, and
   monitor the replacement run. If CI cannot complete, report it pending or
   blocked rather than CI-green.
7. After integration and any authorized push/CI cycle, verify either that the
   approved task commits are reachable from the durable integrated ref or, after
   cherry-picking, that an evidence-recorded original-to-integrated SHA mapping
   has the equivalent patch. Then verify the task worktree is clean before
   removing it, its task branch, and its reproducible `target` directory.

## Throughput discipline

Prioritize plan-critical functionality over repeated polishing of already-correct
per-family evidence. Use one focused independent review cycle per tranche; start
another only when a fix materially changes production behavior or the evidence
contract. Reuse an authoritative native checkpoint when it covers the exact state
product and geometry/material question rather than recapturing for presentation,
and batch native screenshots, matching-view GPU witnesses, and visual polish at the
deterministic gallery/live-acceptance gate when they are not needed to decide the
implementation contract.

## Continuation across chats

Git history, pushed refs, `plan.md`, and committed evidence are the durable
record; do not rely on conversational memory after compaction or a new chat. At
handoff or restart, enumerate active worktrees/branches and classify each tranche
as in progress, locally committed, review-blocked, approved, integrated, pushed,
CI-green, or native-accepted before dispatching new work. Before handoff, record
each active tranche's worktree, branch, base SHA, reviewed head SHA, decision,
finding dispositions, verification/native/CI state, and next action in `plan.md`
or another committed coordination record. Do not redo a completed tranche or merge
an unreviewed one merely because a new chat lacks its discussion: recover the exact
base/head commits and review evidence first, and if the durable review record is
absent, rerun independent review rather than redoing implementation or inferring
approval.
