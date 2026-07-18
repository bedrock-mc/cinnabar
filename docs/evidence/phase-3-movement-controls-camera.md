# Phase 3 movement, controls, and camera evidence

Status: deterministic implementation tranche green; independent review and live/native acceptance remain open.

This record covers the Phase 3 integration corrections based on parent commit
`a9a165139438eba683efd08392d46f940949ac33`. It does not claim that normal-session
Physics authority or the Phase 3 live gate is complete.

## Deterministic contract

- `InteractionOriginSnapshot` is absent by default and after correction,
  session, or dimension invalidation. Its published outbound ray atomically
  copies session generation, committed FIFO sequence, physics tick, pose
  generation, perspective, complete `WorldCollisionIdentity`, eye origin, and
  look direction from one `LocalPlayerFrameCarrier` snapshot.
- Outbound movement sampling no longer reconstructs position or orientation
  from a separate live `LocalViewPose` resource.
- Production uses the exact schedule order `RawInput -> SemanticSample ->
  UiAuthority -> SemanticFinalize -> Physics -> Camera -> Interaction ->
  WorldPublication -> ActorPublication -> UiPublication -> NetworkSend`.
  The schedule regression inspects real registered function-system type sets
  and dependency edges, including local-frame publication before interaction.
- Raw device collection, semantic routing, and router-owned finalization are
  distinct production systems. The finalizer remains the sole writer of the
  published semantic snapshot.
- The production gameplay touch producer classifies bounded normalized contact
  regions for movement, jump, use, and directional look before raw sampling.
  Released contacts are pruned, look direction follows the current drag, and
  chat-focused UI targets remain owned by the chat producer.

## Test-first record

The atomic/schedule RED failed with `E0432` for the missing production schedule
assembler and `E0599` for the missing atomic interaction methods. The touch RED
failed with `E0432` for the missing gameplay producer and `E0599` for missing
explicit movement classification. The acceptance RED failed all five cases
because `scripts/acceptance/Phase3.ps1` did not exist.

The following gates were then green on 2026-07-18:

```text
cargo test --locked -p bedrock-client --lib interaction_origin_consumes_and_invalidates_with_the_atomic_local_player_frame
1 passed

cargo test --locked -p bedrock-client --lib production_client_systems_are_members_of_the_eleven_behavioral_sets
1 passed

cargo test --locked -p bedrock-client --lib gameplay_touch_targets_cover_movement_jump_use_look_and_release_transitions
1 passed

cargo test --locked -p bedrock-client --lib
282 passed, 0 failed

powershell.exe -NoProfile -ExecutionPolicy Bypass -Command '$result = Invoke-Pester -Script "scripts/tests/acceptance/Phase3.Tests.ps1" -PassThru; if ($result.FailedCount -ne 0) { exit 1 }'
5 passed, 0 failed

cargo clippy --locked -p bedrock-client --all-targets --all-features -- -D warnings
passed with zero warnings
```

`scripts/acceptance/Phase3.ps1` validates one bounded JSON evidence record
against the current build commit and checked-in PREG/BREG hashes. It rejects a
wrong target, stale registry identity, tick gaps, any free-camera packet, any
queue drop, non-finite metrics, oversized event/frame arrays, incorrect
perspective/local-avatar results, timeouts, and nonzero process exits.

## Gates still required before closure

- Independent code review of the final integration commit.
- Normal-path Physics authority enable only after candidate evidence is green.
- Local BDS controlled movement/correction/touch/perspective matrix.
- Five-minute authenticated runs on Lunar (`pvp.lunarbedrock.com:19134`) and
  Zeqa (`zeqa.net:19132`), plus LBSG (`play.lbsg.net:19132`).
- Matching native Bedrock comparison for movement cadence, special surfaces,
  camera collision, local-avatar visibility, and touch/controller equivalence.
- Performance/resource evidence and a validated JSON artifact from each
  binding run.

Until those gates are attached and reviewed, `P3-MOVEMENT`,
`P3.4-INPUT-CAMERA`, and Phase 3 remain open.
