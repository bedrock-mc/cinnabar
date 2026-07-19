# Phase 2.7 fast-transfer local chunk handoff

Status: implementation and local verification complete; independent review and native LBSG `/transfer sm3` validation remain integration-owned.

## Scope and commits

- `a7d8a90` — provisionally rebase publisher retention for a genuine local `MovePlayer::Teleport` whose destination is outside the active scope. Purge the old resident/request/retry/deadline cohort, accept destination request-mode `LevelChunk` work before `NetworkChunkPublisherUpdate`, and retain only compatible destination work when the authoritative publisher update arrives.
- `9aae607`, `e654431`, `e176b9a`, `81a0e69` — fail closed on publisher-epoch exhaustion, intersect provisional membership with the authoritative clamped active scope, cover late transport acknowledgement and negative movement boundaries, and keep tests within repository architecture limits.
- `0ab5be1` — stable bounded `RequestClass` scheduling: `PlayerRetry`, `PlayerInitial`, `VisibleRetry`, `VisibleInitial`, `PrefetchRetry`, `PrefetchInitial`; squared horizontal distance within a class; original queue sequence for ties and unsent transport restoration; bounded aging after 16 bypasses.
- `c4ed5a9` — retain absolute resend precedence for an unsent transport retry and hard-bound unconfirmed popped ordering metadata to the 64-slot outbound ceiling.

The request player column is derived only from the last finite camera position supplied to `WorldStream::poll`. A non-finite poll does not replace it. Reservations continue to block later ready work until their FIFO event is prepared. Semantic retries preserve their exact chunk/Y/count, retry-attempt, transport-pending, sent-ack, and timeout ownership.

## Regression evidence

- Red: `player_and_visible_retries_precede_far_initial_prefetch_without_losing_fifo_ties` returned the far initial prefetch at `(6, 0)` before the player retry.
- Green: the same test returns player retry Ys `-4`, `-3` in FIFO order, then visible `(2, 0)`, then prefetch `(6, 0)`.
- `disjoint_local_teleport_accepts_destination_chunks_before_publisher_update` proves destination request-mode work is accepted before the publisher update and survives the later authoritative update.
- Negative regressions prove remote teleports, ordinary local movement, and in-scope local teleports preserve publisher center, resident/request state, and deadlines.
- Boundary regressions prove publisher-epoch overflow clears provisional membership, clamped radius cannot retain unfulfillable required columns, and a late transport acknowledgement cannot restore purged origin work.
- Queue regressions prove exact retry cannot starve behind continuous prefetch, bounded aging eventually services prefetch under player work, unresolved reservations remain barriers, distance ordering uses the last finite poll, and a failed send retains its original FIFO tie identity.

## Verification

- `cargo test -p client-world --locked` — passed (231 passed, 1 ignored, plus 11 entity-runtime and 14 item-action integration tests).
- `cargo clippy -p client-world --locked --all-targets -- -D warnings` — passed.
- `cargo run -p architecture --locked -- check --root . --policy tools/architecture/policy.toml` — passed.
- `cargo fmt --all -- --check` and `git diff --check` — passed.

## Integration/native follow-up

Build and run the exact reviewed head against `play.lbsg.net:19132`, enter a world, execute `/transfer sm3`, and prove the destination player column plus nearby spawn columns become resident/presented without an `InactiveLevelChunk` increase or movement lock. Capture publisher epoch/center, required/loaded columns, request class depths/order, stale/timeout counters, world-ready time, and a native screenshot. Then repeat the ordinary movement/small-teleport controls.

LBSG's same-connection fast proxy does not use a Bedrock `TransferPacket`; this fix intentionally remains in `client-world`. Client blob-cache behavior is protocol-owned and was not changed here; investigate it only if native telemetry shows missing cache/status exchange after this local reset fix.
