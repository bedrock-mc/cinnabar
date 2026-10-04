# HUD paper doll and menu follow-up

Pack paths are relative to the install-fetched resource pack pinned by
`assets/vanilla-source.json`.

## Vanilla rules

| Rule | Behaviour |
| --- | --- |
| HUD control | `ui/hud_screen.json:8` defines a 15 by 15 custom `hud_player_renderer` with `#paper_doll_visible`. |
| Desktop placement | `ui/hud_screen.json:1372,3696` places it top-left at `[15,15]` inside the safe-zone GUI tree. |
| Pocket placement | `ui/hud_screen.json:1607` uses the same renderer in the pocket HUD. |
| Hold timer | Movement, armor and emotes refresh the timer in the priority order and for the durations described below; expiry removes the model without fading. |
| HUD actor | Use the resolved actor geometry and a separate full-body UI pose, preserving head rotation. |
| Visibility | Respect Hide Paper Doll, Hide HUD/F1 and spectator. |
| Menu projection | Use inverse GUI scale and `min(width/20,height/39)` with the authored model origin. |
| Model origin | Retain the 24-pixel ModelPart origin scaled by the player model scale; swimming adds a 0.8 vertical adjustment. |
| Inventory geometry | `ui/inventory_screen.json:987,1047,1114,1170` and `ui/ui_common.json:3794,5180` define inventory placement. |
| Pause dimming | `ui/pause_screen.json:1181` defines full-screen background alpha 0.1 and an additional left panel. |
| Home player and Profile | `ui/start_screen.json` uses `#is_paper_doll_visible` and `#profile_button_a_visible`. |
| Marketplace ribbons | Preserve `messageText.bannerText`, artwork and RGB color data for `ui/start_screen.json:1626,2031`. |

The supplied `inbox-vanilla.png` establishes the five categories and list layout.
The installed PlayCover OreUI `index-168bae443ec79c00823c.js` corroborates the
`/inbox`, `/inbox-settings` and `/message/…` routes, category keys and the seven-day
Recent boundary. That bundle identifies internally as 1.26.51, so it is a
near-patch reference, not proof of exact target-version acceptance.

## Implemented behavior

Sneaking, dry sprinting, swimming and crawling hold the HUD doll for one second.
Flying and gliding hold it for 0.35 seconds. A changed list of armor item IDs
holds it for three seconds; emotes hold it for 0.35 seconds. The renderer uses
that priority order: movement defers the armor comparison. Initial armor
observation changes the initially empty list. Otherwise elapsed time is
subtracted. Expiry removes the model without an opacity fade. Ordinary walking
and riding alone do not refresh the timer.

The visibility binding also respects Hide Paper Doll, Hide HUD/F1 and spectator.
F8 continues to toggle Hide Paper Doll through the existing setting. JSON-UI
owns placement, GUI scale and safe area; the built-in Java HUD overlay retains
the vanilla control, and server packs retain their normal higher precedence.

The HUD reuses the existing GUI triangle, skin, armor, held-item and lighting
pipeline. Its body comes from the local actor's resolved geometry and evaluated
bone pose, with a separate full-body UI evaluation independent of the world camera.
The UI context sets `query.is_in_ui`, clears `variable.player_x_rotation` and
leaves the head rotation queries intact. This prevents the first-person hand skeleton from becoming the
HUD body. The fixed preview yaw corresponds to native body yaw -22.5 degrees;
head movement remains in the evaluated pose. Vanilla fixes both body-yaw samples
at that angle and adjusts head yaw to `old_head - old_body - 22.5`.
The preview's Y rotation already uses the native yaw direction; negating that
fixed yaw again reversed the HUD body. It now faces screen-right, toward the
player's left, while world turns leave its facing fixed and the head keeps its
relative look animation. This changes the HUD view only. The HUD retains the
native 24-pixel ModelPart origin, scaled by the player model scale; its control
center is therefore not the body midpoint. Swimming applies the native 0.8
vertical adjustment. Original skin texels are sampled at final pixel resolution.
The existing UI lighting implementation is described in `player-preview-rendering.md`.

Home publishes the bindings which expose its loaded player and Profile button.
Menu paper dolls use the authored model origin, inverse GUI scale and native
`min(width/20,height/39)` scale instead of centering half the player's height.
This clears the nametag overlap. Pause already has the pack's full-screen dim;
its stronger left panel is intentional. No extra darkening was introduced.

The core preserves item-list artwork, `messageText.bannerText` and RGB color
fields previously lost in the service decoder. The normal runtime artwork cache
and JSON-UI bindings supply the Marketplace layers and ribbon. No campaign is
fabricated when the service has supplied no artwork.

Inbox lists the native categories, uses received dates for Recent/History, and
shows title, source, date, unread marker and a separate Delete button. Opening a
message displays a scrollable detail view and reports its read action. Deletion
requires confirmation. Optimistic read/delete identities survive stale refreshes.
The header action exposes mark-all-read and delete-read actions. Standalone
category sprites can be loaded from the existing optional local OreUI bundle.

## Evidence and remaining acceptance

Focused tests cover native timer boundaries, wet sprinting, armor priority,
settings, spectator, scale/safe-area layouts, the unmodified pack and Java HUD,
full-body versus first-person animation, service envelope preservation, long
inbox titles, and read/delete reconciliation. The offline native GPU harness
writes `native-home.png`, `native-pause.png`, `native-paper-doll.png`,
`native-inventory.png` and `native-inbox.png` to `CINNABAR_FORM_SNAPSHOT_DIR`.
It uses authored messages and an installed classic skin, without an account or
server connection. These frames do not prove movement, persona or server-pack
parity by themselves.

Incomplete: frame interpolation, complete
persona layer/material behavior, riding vehicle rendering and target-version
matched animation captures. The menu framing is vanilla but
still needs matched pause captures at identical GUI scale. The inbox's full
settings/preferences route, rich message actions, runtime category artwork in
ordinary installs and target-version pixel acceptance remain open. The open
font remains the project's accepted deviation.

The owner's inventory screenshot has no matched vanilla inventory capture.
The pack explicitly places the offhand grid at `[79,61]`, armor at `[7,7]`, and
stack counts one pixel below their bottom-right anchor. Those source-authored
positions were retained; moving them by eye would not establish parity.

The stretched-model report was not reproduced. The earlier local-player notes
and the classic-skin offline frames do not capture the owner's actual persona
geometry. HUD geometry now follows the resolved rig, but inventory's existing
classic-body fallback and world persona rendering still need the failing skin
and pose witness. This change does not claim to fix that world-rendering bug or
close the overall visual parity gate.
