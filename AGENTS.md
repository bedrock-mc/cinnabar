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
  loops unless a fix materially changes production behavior or the evidence
  contract.
- Reuse an existing authoritative native checkpoint when it covers the exact
  state product and geometry/material questions. Do not recapture equivalent
  views merely to improve presentation.
- Batch native screenshots, matching-view GPU witnesses, and visual polish at
  the deterministic gallery/live-acceptance gate whenever they are not needed
  to decide the implementation contract.
- Keep independent implementation lanes parallel only when every writing lane
  uses a dedicated worktree and unique task branch, with no shared-write
  conflict. Merge or cherry-pick only after each lane is green and reviewed.
- Report status precisely: distinguish pushed work, locally committed work,
  test-green uncommitted work, and work that is only in progress.

## Default multi-agent task workflow

Use this workflow for non-trivial implementation, debugging, protocol, asset,
rendering, performance, and integration work. The root agent is the coordinator
and integrator; subagents own bounded implementation or review assignments.
For read-only diagnosis or review requests, apply only the preflight, evidence,
and reporting portions; do not create branches, edit, commit, integrate, or push
unless the request authorizes changes.

### Model and reasoning defaults

- When the runtime exposes model and reasoning controls, use GPT-5.6 with
  medium reasoning for both implementation and review; select `solve` only
  when that is the runtime's actual control name. If those controls are not
  exposed, use the configured runtime default, record that explicit selection
  was unavailable, and continue. Never claim a model selection the runtime did
  not expose.
- Escalate above medium only for genuinely architectural, unsafe, protocol-
  ambiguous, concurrency-sensitive, or cross-cutting decisions. Do not spend
  high reasoning effort on mechanical edits, formatting, or routine test
  updates.
- Dispatch only as many subagents as there are bounded, dependency-independent,
  non-overlapping lanes. Never assign multiple implementers the same tranche
  speculatively. Keep the root agent available to coordinate, inspect results,
  integrate, and communicate, and do not turn Cargo or Go build contention into
  the bottleneck.

### Preflight and task decomposition

1. Read this file, the relevant section of `plan.md`, and the files directly
   involved in the requested tranche. Inspect `git status`, the current branch,
   recent commits, and `git worktree list` before any mutation.
2. State the exact acceptance condition. Distinguish implementation completion,
   test completion, native/live acceptance, performance acceptance, and phase
   closure; they are separate gates.
3. Split broad work into the smallest independently testable tranches that a
   reviewer could approve or reject on their own. Prefer reusable family-level
   or data-driven solutions over repeated one-off fixes.
4. Parallelize only explicitly disjoint tranches. Every writing agent must have
   a coordinator-provisioned dedicated linked worktree, unique task branch, and
   non-overlapping file ownership. Keep dependent tasks sequential, and never
   allow two agents to edit the same branch/worktree concurrently.
5. Give every subagent a self-contained assignment containing:
   - for writers, the exact repository, worktree, branch, and base commit; for
     reviewers, the exact repository, base/head SHAs, and any coordinator-
     provisioned detached review worktree;
   - the bounded goal and the reason it matters;
   - binding requirements and explicit out-of-scope/prohibited changes;
   - the relevant source-of-truth and acceptance evidence;
   - exact focused verification expectations;
   - for writers, `commit only; do not push, merge, switch the authoritative
     worktree, or edit plan.md/AGENTS.md unless explicitly assigned`; for
     reviewers, `inspect only; do not edit, commit, push, merge, or switch
     branches`;
   - the required report: status, commit hash, changed behavior, commands and
     results, open risks, and remaining native gates.

### Worktree and ownership rules

- The root agent owns the authoritative integration worktree. Before dispatch,
  the coordinator runs `git worktree list`, then creates a missing writing
  worktree with `git worktree add -b <branch> <path> <base>`. Do not create or
  remove worktrees concurrently. Writing subagents must use their assigned
  linked worktree and unique task branch; they must not switch branches in the
  authoritative worktree.
- Reviewers inspect commit objects without mutating a working tree, or use a
  coordinator-provisioned detached clean worktree at the exact reviewed head.
  Do not attempt to check out an implementer's branch in a second worktree.
- Before dispatch, record the task base commit. Review and integrate the entire
  `base..head` range, never an assumed `HEAD~1`, because a task may contain
  multiple commits.
- Only the root integrator merges or cherry-picks task commits, updates the
  authoritative plan, pushes shared branches, and monitors CI.
- Preserve useful commit history. Never force-push, reset, or otherwise rewrite
  protected/shared history. Squash or rewrite an unshared task branch only when
  the user explicitly requests the exact operation and branch.
- Preserve unrelated user changes in dirty worktrees. Stop and report a real
  overlap instead of discarding or overwriting it.

### Implementation contract

1. Establish the behavior contract from authoritative data or evidence. Never
   turn an inference into a claimed vanilla/protocol fact.
2. Use test-driven development for behavior changes: add a focused failing
   regression or conformance witness, observe the expected failure, implement
   the smallest correct change, and observe it pass.
3. Keep runtime work bounded and fail closed on malformed, ambiguous, stale,
   unsupported, or unproven data. Preserve Cinnabar's palette-native,
   allocation-bounded, and version/provenance-pinned architecture.
4. Run the focused tests that exercise the changed contract, plus formatting,
   strict warnings/clippy or vet, applicable architecture checks, and
   `git diff --check`. Run broader suites in proportion to integration risk.
5. Self-review the complete diff for scope creep, duplicated logic, unchecked
   limits, guessed semantics, stale provenance, accidental assets, and missing
   negative tests. Then commit locally with an intentional message and leave
   the task worktree clean.

### Mandatory independent review loop

1. Dispatch a fresh reviewer that did not implement the task, using the model
   policy above. Give it the task contract, implementer report, and complete
   `base..head` diff. Reviewers inspect only and do not edit or push.
2. Require an explicit `APPROVE` or `NEEDS CHANGES` decision with findings
   classified as Critical, Important, or Minor and tied to concrete files,
   behavior, and evidence.
3. Send every Critical and Important finding back to an implementer/fixer.
   Require focused regression coverage and fresh verification for the fix.
4. The integrator must verify that every Critical and Important finding is
   dispositioned. Dispatch one fresh re-review of the complete
   `base..<new-head>` range only when a fix materially changes production
   behavior or the evidence contract. Non-material corrections require fresh
   focused verification and a recorded disposition, not another review cycle.
   A self-review, green test suite, or plausible native screenshot does not
   replace the independent gate.
5. Record Minor findings for the integrator/final review. Do not create repeated
   review loops merely for wording or presentation changes that cannot affect
   behavior.

### Integration, verification, and pushing

1. Record the approved head SHA. Inspect the approved commits and verify that
   the task worktree is clean and contains no prohibited files. Immediately
   before integration, verify the task branch still resolves to the approved
   SHA and integrate exactly that reviewed SHA/range without flattening
   history. If the branch moved, review the new complete range first.
2. Run fresh post-integration verification covering the changed crates/modules
   and their consumers. For cross-cutting changes, run the full workspace/Go
   suites, strict linters, architecture enforcement, acceptance harnesses, and
   `git diff --check` as applicable.
3. Update `plan.md` in the integration branch using evidence-backed language.
   Mark a checkbox complete only when its stated implementation, native/live,
   performance, and review gates are all satisfied; otherwise record the landed
   tranche and the exact remaining gate.
4. Commit integration and plan updates intentionally. Push only when the
   current task explicitly authorizes the target remote branch; otherwise stop
   at a clean local integration commit and report that state. Before pushing,
   fetch and verify the intended remote ref and that the push is fast-forward.
   On divergence, stop and reconcile without rewriting shared history. Never
   force-push a protected/shared ref. After pushing, verify that the remote ref
   resolves to the expected commit and monitor the resulting CI run.
5. Report the exact pushed commit, verification commands/results, review
   decision, CI state, user-visible unlock, and remaining work. Never describe
   locally committed or merely reviewed work as pushed or integrated.
6. Treat a CI-caused code or evidence correction as a bounded fix tranche:
   reproduce it, add focused coverage where applicable, implement and verify
   it, independently re-review material behavior/evidence changes, integrate
   the approved head, push a new authorized commit, and monitor the replacement
   run. If CI cannot complete, report it as pending or blocked rather than
   CI-green.
7. After successful integration and any authorized push/CI cycle, verify either
   that the approved task commits are reachable from the durable integrated
   ref, or, after cherry-picking, that an evidence-recorded original-to-
   integrated SHA mapping has the equivalent patch. Then verify the task
   worktree is clean before removing it, its task branch, and its reproducible
   `target` directory according to the cache rules below.

### Native, visual, and performance evidence

- Use native Bedrock/BDS comparison when it is needed to decide a contract or
  close an explicit acceptance gate. Prefer version-matched, reproducible,
  fixed-state galleries and exact protocol fixtures over visual guesswork.
- On Windows, perform live BDS/client acceptance only from the firewall-approved
  stable executable paths defined above, after integration and a build at the
  canonical path. Never launch or copy a task-worktree executable for live
  acceptance. Apply the WGC-first, fresh-capture, `%TEMP%`, and no-screenshot-
  in-git rules above to every visual gate.
- Batch equivalent native captures and reuse an authoritative existing witness
  when it covers the same version, state product, camera, geometry, material,
  and behavior question.
- Keep Mojang assets, screenshots, recordings, generated local carriers,
  credentials, BDS binaries, and other local payloads out of git. Store captures
  under temporary/ignored paths and commit only compact lawful rules,
  provenance/checksums, and independently authored evidence descriptions.
- Performance claims require measured release evidence against the stated
  `plan.md` budgets. A debug screenshot, a small test scene, or a green unit
  suite is not performance acceptance.

### Continuation across chats

- Treat git history, pushed refs, `plan.md`, and committed evidence as the
  durable record; do not rely on conversational memory after compaction or a
  new chat.
- At handoff or restart, enumerate active worktrees/branches and classify each
  tranche as in progress, locally committed, review-blocked, approved,
  integrated, pushed, CI-green, or native-accepted before dispatching new work.
- Before handoff, record each active tranche's worktree, branch, base SHA,
  reviewed head SHA, decision, finding dispositions, verification/native/CI
  state, and next action in the relevant durable `plan.md` section or another
  committed coordination record.
- Do not redo a completed tranche or merge an unreviewed one merely because a
  new chat lacks its earlier discussion. Recover the exact base/head commits and
  review evidence first. If the durable review record is absent, inspect the
  existing range and rerun independent review; do not redo implementation or
  infer approval.

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

## Visual changes

- Do not push or describe any UI, HUD, text, graphics, shader, or rendering change as ready without a real rendered-frame visual acceptance pass on the target platform, resolution, and DPI/scale. Unit tests, snapshots, draw-list checks, GPU adapter tests, lint, and code review are necessary but are not substitutes for seeing the final output.
- The visual pass must explicitly check legibility, geometry, clipping, depth/layering, scaling, colors, and the relevant live input/focus behavior. Record the tested platform and visible result. If the target-platform pass cannot be performed, keep the change local and state that it is not cleared to push.
