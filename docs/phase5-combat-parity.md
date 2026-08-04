# Phase 5 combat parity matrix

This matrix is the acceptance record for the supported protocol-1001 player-versus-player lane.
The client submits input and target evidence; the server remains authoritative for damage,
inventory, movement, knockback, cooldowns, and visible action results.

| Behavior contract | Current implementation | Acceptance state |
| --- | --- | --- |
| Gameplay input timing | Attack and entity-use actions are sampled from the semantic gameplay context on the press edge. UI focus, session changes, and dimension changes cannot leak an input edge into gameplay. | Implemented locally; native held-repeat/cadence witness remains open. |
| Interaction origin | Targeting uses the frozen physics-owned eye origin and view direction, never the third-person camera boom. | Implemented locally; live movement/rotation witness remains open. |
| Player target set | Only retained remote player actors are eligible for this PvP lane. Removed, stale, non-player, and invalid-coordinate actors are rejected. | Implemented locally; actor-authority dependency remains open. |
| Target geometry | The selector uses the retained player pose metadata for standing, crouching, and sleeping bounds and selects the nearest deterministic hit. | Implemented locally; exact pose-transition parity remains open. |
| Solid-block occlusion | A read-only palette collision query covers the complete ray segment. Unloaded chunks, unknown collision data, or a changed collision identity fail closed. | Implemented locally; live occlusion witness remains open. |
| Reach | A 3-block bounded player reach is enforced before a transaction is built. | Implemented for the normal lane; game-mode-specific reach rules remain a required native/live gate. |
| Entity attack transaction | The packet uses transaction type `ItemUseOnEntity`, action `Attack`, the selected slot, verified held `ItemV4`, player position, and click position relative to the target base. | Wire builder implemented; packet fixture and live-server witness remain open. |
| Entity use transaction | The same transaction path supports action `Interact` when a remote player is targeted. | Implemented for entity use; block/item use is a separate interaction tranche. |
| Missed swing | An attack miss emits the server-visible `MissedSwing` player action when the local runtime identity is known. | Wire builder implemented; live animation witness remains open. |
| Damage and knockback | No client damage, health, inventory, or velocity result is synthesized. `SetEntityMotion` is retained as a distinct authoritative motion event; remote actor velocity is updated and active local physics applies the server velocity while discarding stale prediction history. | Motion reconciliation is implemented locally; packet ordering, impulse timing, and live health/damage witnesses remain open. |
| Hurt/death/attack action timing | Server `EntityEvent` notifications are normalized into the existing action timeline for swing, attack, hurt, death, use, and stop-attack triggers. No local result is invented. | Ingress and bounded retention are implemented; actor presentation and live trigger timing remain open. |
| Cooldown | Server-declared item cooldowns are retained by category and suppress matching local attempts. The client does not invent a damage cooldown; the server owns acceptance. | Server-declared item cooldown handling is implemented; exact native attack cadence/held-input behavior remains open. |
| Session replacement | Pending packets and retained cooldowns carry session scope and are cleared across replacement. | Implemented locally. |
| Backpressure | At most one frozen combat packet is retained, retried only briefly, and then discarded rather than replayed against a stale target. | Implemented locally; transport stress witness remains open. |
| PvP edge cases | Deterministic tie-breaking, block occlusion, unavailable collision data, stale actors, unknown held-item data, spectator mode, and invalid runtime IDs fail closed. | Implemented locally; live edge-case witness remains open. |

## Remaining closure gates

This tranche must not be treated as full combat parity. Closure still requires:

1. A reproducible packet fixture or capture proving the transaction bytes and the
   server's acceptance/rejection behavior.
2. A live authoritative-server witness for normal, crouching, sleeping, moving,
   occluded, out-of-reach, removed-target, and rapid-click cases.
3. Exact game-mode reach and attack cadence confirmation, including held-input behavior.
4. Live authoritative validation of local health/damage and motion
   reconciliation, including impulse timing that cannot be confused with
   ordinary movement interpolation.
5. Server-driven hurt, death, swing, and attack action presentation through the
   actor authority/interpolation path.
6. Exact game-mode reach, attack cadence, and held-input confirmation; block/item
   use remains a separate transaction target lane.
7. Independent review after the actor dependency lands, followed by focused and
   workspace verification.

Validation performed for this tranche:

```text
cargo check --locked -p protocol
cargo check --locked -p bedrock-client
git diff --check
```

No client-side hit, damage, knockback, or inventory mutation is used as a substitute for those
remaining server-authoritative gates.
