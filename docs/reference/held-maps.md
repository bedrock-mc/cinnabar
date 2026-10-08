# First-person filled maps

A filled map retains the signed 64-bit `map_uuid` from its inventory stack.
Selecting it requests the missing server image through the same deduplicated retry
owner used by framed maps. Unknown images still draw paper; received MapInfo
pixels update the selected map when their retained revision changes.

The map plane includes the paper margin around the image. Its background comes
from the selected resource pack's `textures/map/map_background`, with the fetched
built-in pack as fallback. Transparent unexplored pixels leave the paper visible.
The raster is cached per hand, map identity, server session, background and image
revision. Main-hand two-arm, main-hand one-arm and offhand placement use distinct
stacks; the offhand follows its independent equip animation.

## Verification and remaining gates

Protocol tests preserve signed IDs beyond floating-point integer precision and
reject missing, truncated or wrongly typed identities. Raster/pose regressions
cover paper margins, unexplored pixels, transparent custom backgrounds, updates,
hand offsets, equip dips and invalid sampled input. A live local-server pair uses
the same physical map transferred between the before and after clients, with the
same world, player, camera and GUI scale. The fixed client draws real server
terrain pixels.

Map decorations, third-person dynamic map artwork and a matched-version native
pose comparison remain incomplete. This first-person visibility fix does not
close those parity gates.
