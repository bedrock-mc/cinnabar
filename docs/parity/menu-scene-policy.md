# Menu input and gameplay rendering

The hotbar read Bevy's accumulated wheel independently of the semantic router. Its
focus check covered chat, containers and forms, but omitted MenuRuntime. The
animated first-person rig likewise had no menu/background check. World queues
already stopped under the panorama; the hand used a separate render pass.

Screen input absorption and game visibility are separate flags. Pause absorbs
input while allowing the world and hand. A screen which disallows rendering the
game suppresses world queues and both first-person adapters. A covered menu layer
with `render_only_when_topmost` does not draw. Flags are read from resolved roots,
using the same catalog, inheritance, variables and server overlays as rendering.

## Vanilla rules

| Rule | Behaviour |
| --- | --- |
| Screen settings | Root controls supply input absorption, game visibility and topmost-only rendering. The associated masks are `0x08` at offset `0x18`, `0x04` at offset `0x18`, and `0x02` at offset `0x1a`. |
| Defaults | `render_game_behind`, `absorbs_input` and `render_only_when_topmost` default to true. |
| Input delivery | Deliver input to the top scene first, then only to other scenes which always accept input. |
| Scroll | Route scroll movement to a scroll-view control as UI input independently of hotbar selection. |
| Visibility | Traverse visible screens and read game visibility from root ScreenSettings. |

The read-only retail **v1.26.50.4** resource pack establishes the actual roots:

- `ui/hud_screen.json:3578` explicitly sets `absorbs_input=false`.
- `ui/settings_screen.json:72` inherits `settings_screen_base`, then
  `settings_common.screen_base` (`ui/settings_sections/settings_common.json:2347`),
  `dynamic_dialog_screen` (`:1856`), and `common.base_screen`
  (`ui/ui_common.json:6345`).
- `ui/pause_screen.json:1136` inherits `common.base_screen`; its retail background
  has alpha 0.1 (`:1182`). It omits `render_game_behind`, retaining the true default.

**The supplied Settings JSON does not set `render_game_behind=false`.** Both
Settings and Pause inherit the factory's true default. The implementation preserves
that value instead of inventing a pack property. Cinnabar's existing Settings
panorama is an independent full-screen replacement background; while shown it
also suppresses gameplay, including the hand. Pause and Death do not use that
panorama. This fixes the reported layering without changing the pack defaults.

## Raw input audit

Hotbar wheel selection, cursor capture/re-capture, middle-click pick block,
world Q/Control+Q drops and right-click book opening, and extension keybinds use
the shared absorption check. The semantic authority uses that same check.
`semantic_controls/physical.rs` samples devices for the router; UI readers in
menu, chat, containers, forms and sign editing deliver input to their own screens.
`camera::movement_axes` is test-only. F2 screenshots and the F3 diagnostics toggle
are application tools, rather than gameplay actions.

Offline snapshots exercise the real UI carrier. They do not capture the separate
GPU hand pass; regression tests check hand admission and clearing. No server
connection or native visual parity gate is closed by these offline checks.
