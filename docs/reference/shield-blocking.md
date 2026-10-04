# Shield blocking query and owner equipment

The runtime pack is selected by `assets/vanilla-source.json`.

## Rendering contract

`query.blocking` reads `ActorDataFlagComponent` (type hash `0xc67426f3`),
byte 9 bit 0, which is actor flag 72, and returns a boolean script argument.
It does **not** call `Player::isBlocking`, derive blocking from the processed
sneak state, or apply a local five-tick timer.

The pinned Shield attachable's pre-animation scripts require both owner hands.
The main-hand blocking predicate is `query.blocking`, no Shield in the offhand,
and a Shield in the main hand. The offhand predicate is `query.blocking` and a
Shield in the offhand. The authored wield controller selects hand by typed
`context.item_slot` and transitions using the same blocking query. Pose
keyframes, rotations and transition timing remain in the pack; our runtime
must provide the owner equipment and metadata rather than duplicate poses.

Attachable input now retains both owner hand identifiers. Previously it
constructed an artificial query actor holding only the rendered item, which
lost offhand priority and the main-hand Bow predicate for an offhand Shield.

## Gameplay and authoritative state

`ServerPlayer::normalTick`
sets flag 72. Its eligibility checks include cooldown, sneak/using state,
vehicle/swimming state and the actor's scaffolding flags 69/70.
The distinct Shield-blocked flags are 74/75. None of those metadata flags
are additional PlayerAuthInput bits.
Current `SneakTriggerSystem::doActionTick` updates the
processed sneak/swim/crawl flags but does not set blocking 72.

The separate damage-blocking predicate `Player::isBlocking` requires flag 72, an active Shield
(offhand preferred), and level tick minus stack blocking timestamp greater
than four. `ShieldItem::inventoryTick` maintains that
timestamp on the server only. `readUserData`/`writeUserData` transmit its trailing signed 64-bit value.
`ShieldItem::use` is a no-op: starting ordinary ranged-item
use is not Shield blocking. These gameplay references corroborate authority;
they are not substitutes for a version-matched gameplay acceptance gate.

Local player movement exclusion does not exclude authoritative metadata.
The regression publishes an actual `ActorEvent::Metadata` update, then
client-fed movement and predicted item-use updates, and verifies flag 72
survives. The attachable VM regression separately verifies that sneak alone
does not satisfy blocking and that a Shield in the other hand takes priority.

## Verification limitation

The pinned Dragonfly fixture's `server/item/shield.go` explicitly leaves
raising, absorbed damage and axe disable unimplemented. Its projectile
implementation also notes that Shield blocking is not implemented. That
fixture therefore cannot establish a native Shield gameplay or animation
gate, and injecting flag 72 into it would only manufacture the witness.
Use a vanilla BDS or a compatible server which supplies authoritative flag
72. No client-side fallback bypasses the native metadata query.

The offline vanilla-BDS test supplied actual flag 72 transitions when sneaking:
false/standing, false/sneaking, true/sneaking, true/standing, false/standing.
The owner subsequently confirmed the blocking animation works. The temporary
state trace was removed after that authority check. Application submission now
includes separate main/offhand rigs and texture bindings; fresh macOS/Metal
Retina captures show both shields and their inventory/offhand icons.
These are functional and rendered-frame checks, not a matched native gallery.
Patterned/glint layers and damage/cooldown parity remain incomplete.
