# Crossbow use state

## Charge, load, fire

The maximum use duration is 25 ticks minus five ticks per Quick Charge
level; loading does not change it to zero. Using the item checks the stack's
cached charged projectile. An uncharged item starts use; a
charged item fires and then removes `chargedItem` and its cached projectile.

Release computes normalized draw power from duration minus
remaining ticks. Only full power loads a projectile. It checks offhand arrows or
fireworks first, then inventory arrows; creative can synthesize an arrow when no
projectile is present. The compound mutation writes the serialized projectile into
`chargedItem` and retains a cached item stack. When the use duration runs out, the crossbow releases with zero remaining
ticks. A short release clears rather than loads the projectile.

After the item-complete gameplay event, completing the use skips the
transaction/depletion branch on the client. The sub-client id is a separate
field.

Only the server-side branch builds a release-item inventory transaction: it
fills selected slot, player position and action `Use` (1), runs the
use-time-depleted step and writes the mutated stack. That branch also records
inventory actions and `CompletedUsingItem`. It does not justify sending a
client completion transaction or a second click-air request. Ordinary early
button release still sends a release transaction. The item's animation frame is 4 for loaded arrows and frame 5 for fireworks
independently of whether the use button is still held.

## HUD and inventory icons

The crossbow registers five icon records from the `crossbow_pulling` atlas
key, in variants zero through four. Its icon lookup uses the ordinary standby
icon for frame zero; nonzero animation frame N addresses registered record N minus one.
The pinned pack's `textures/item_texture.json` orders those variants as
pulling 0, pulling 1, pulling 2, loaded arrow, and loaded firework. Thus damage
metadata is not a charging or loaded-icon selector.

HUD capture now routes every charged stack's icon through that mapping. Loaded
NBT applies to hotbar, inventory, offhand, storage, and cursor cells.
Player hotbar slots also receive the same animation frame and revision-scoped
local charge/fire prediction as their attachable; switching selection retains
the loaded icon, and authoritative corrections replace it. The renderer still
uses the original item identity for glint and never changes damage, NBT,
counts, network identities, or outgoing descriptors to select an icon.

The focused icon tests cover the frame-to-variant crosswalk, arrow/firework
NBT, malformed NBT, damage independence, charge persistence after reselection,
fire, and both identical and changed authoritative corrections. This closes
the tested behavior contract, not visual acceptance; a live vanilla-BDS rendered
frame is required. Stateful icon overrides in arbitrary server packs remain a
separate incomplete path: the existing session icon carrier retains one icon
per canonical item, not a complete atlas-variant table.

The October 1 offline vanilla-BDS run on macOS/Metal at Retina scale 2 shows
standby, partial/full drawing and persistent loaded-arrow hand/HUD states.
Reopening the survival inventory also shows the loaded-arrow icon. Ignored
captures `2026-10-01_23.47.17.png` through `23.47.21.png` and `23.48.14.png`
record that run; the arrow count changes from 64 to 63. This is live functional
and rendering evidence, not a version-matched vanilla frame comparison.

The October 2 UTC follow-up repeats loading against offline vanilla BDS with
the canonical client.
At 01:04:09 the server restates arrow count 63 and the loaded crossbow with a
new stack ID. The inspected `2026-10-02_01.04.10.png` HUD and, after selecting
another slot and returning, `2026-10-02_01.06.48.png` open-inventory frame show
the loaded-arrow icon and unchanged 63 arrows. The ordinary shield remains
visible in offhand. This specifically accepts the reported loaded inventory
icon/persistence bug; controlled full fire/reload and firework comparisons
remain open.

## Cinnabar implementation and boundaries

Item-use prediction retains at most one override per hotbar slot, bound to the
exact verified stack and its authoritative slot-write revision. Full charge
retains the chosen loaded projectile after the charging use ends. The next fresh
press fires immediately and clears the local loaded state rather than starting
another hold. The hand pose and next-press classification use the same override;
handled idle items clear stale server `using_item` metadata for the local rig.

Every authoritative content, slot, normal transaction, or accepted correction advances its
addressed player-slot revision, including byte-identical restatements. Therefore
the newest authoritative loaded/unloaded NBT always wins, and a rejected local
load cannot survive a same-value correction. Switching away after loading keeps
the original slot's prediction; switching during a partial charge cancels it.
New stack identities and new sessions do not inherit charge. Server inventory
truth, ammunition, damage, projectile entities and outgoing verified descriptors
are not rewritten by this local state layer.

Incomplete: local loaded-pose prediction is a revision-scoped overlay, not a
complete implementation of the client's item gameplay-event and inventory
observer path. Server-side `chargedItem` serialization, associated inventory
actions and `CompletedUsingItem` are not fabricated on the client wire. The
current change covers local loaded-state persistence and action classification,
not full item-use/inventory parity.
It needs a live load-persist-fire-reload pass on a version-matched server.

The ignored local fixture uses the pinned Dragonfly fork. Its
`session/handler_inventory_transaction.go` routes all release transactions to
`Player.ReleaseItem`, ignoring the action subtype. Crossbow implements
`Chargeable`, not `Releasable`; its `Charge` path is reached by a second
`Player.UseItem` call instead. Consequently the fixture cannot establish vanilla
crossbow loading from the client's silent duration completion. No click-air
workaround, fabricated loaded packet or production Go change masks this gap.
