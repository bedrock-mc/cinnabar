# Lifeboat session block palette

The gray terrain and blocked movement reproduce with freshly compiled, coherent
carriers. Lifeboat supplies 17 custom definitions and no vanilla data-driven
definitions. Cinnabar admitted every state in its complete carrier anyway, so
wire air resolved to an opaque mushroom stem. Framing and chunk decoding reported
no errors because the incorrect internal ID was valid.

## Vanilla rules

| Rule | Behaviour |
| --- | --- |
| Remote baseline | Register built-in vanilla blocks. Admit vanilla data-driven blocks only from the server's definitions. |
| Name order | Sort all admitted block types by unsigned FNV-1 64 name hash, then name. |
| State order | Preserve each admitted type's canonical permutation order. |
| Carrier identity | Keep complete carrier IDs stable; translate the session's sequential IDs in both directions. |
| Unknown wire ID | Resolve to air, count and log the disagreement. |
| Hashed mode | Preserve all hash bits; sequential admission does not renumber hashes. |
| Inventory identity | Retain raw stack descriptors for protocol roundtrips; resolve wire IDs when selecting visuals or predicting placement. |

The pinned `behavior_pack/blocks` contains 98 data-driven vanilla definitions,
covering 1,417 canonical states. Omitting them removes 1,181 states before air.
The metadata is bound to the complete registry digest, including definitions
whose carrier visuals are reserved. Its generator and reproduction command are
documented in `tools/registrygen/cmd/serverdefined/README.md`.

Captured remote terrain agrees with the admitted palette:

| Wire ID | Resolved block |
| --- | --- |
| 15844 | `minecraft:air` |
| 3219 | `minecraft:stone` |
| 9650 | `minecraft:polished_blackstone_bricks` |
| 19595 | `minecraft:brown_terracotta` |
| 17951 | `minecraft:gray_concrete` |

The custom names sort after the vanilla names, so their sort algorithm was not
the source of this shift. No lighting constants or server movement checks need
changing to repair these identities.

## Validation

Focused regressions cover remote admission, partial and complete definition
sets, session replacement, interleaved custom blocks, inverse interaction IDs,
unknown-ID bounds, and raw descriptor preservation in presentation. The original
admission regression failed with air resolving to mushroom stem before the fix.
The metadata reproduces byte for byte from the pinned public inputs.

A fresh macOS/Metal run of build `04d6e1e0` stayed connected for the complete
300-second acceptance window and sent 6,323 movement samples, with zero decode
errors, outbound drops or authority stalls. Walking advanced 16.5 blocks down
the lobby steps; strafing advanced another 8.7 blocks and jumping was exercised.
Rendered frames show the lobby terrain, signs and actors instead of gray air.
The window used 1280x720 logical content size (2560x1440 physical, scale 2). Lifeboat reported
PocketMine-MP 4.23.3+dev and the connection resolved to `135.148.32.47:19132`.

A second join opened the compass Navigator and selected the Mini Game Selector,
confirming both request and form-response paths. `/transfer sm3` completed the
server's fast-transfer path: Survival Mode terrain and its HUD appeared, a
three-second walk/jump advanced 10 blocks and climbed two blocks, and the held
book opened the Survival Mode menu. The second connection resolved to
`15.204.237.170:19132`. No server disconnect or decoding failure occurred.

The supplied native lobby screenshot is a near-version visual witness, not an
identical-version parity or performance qualification. Some carrier blocks
still use diagnostic art; this repair does not close those separate visual
support gates.
