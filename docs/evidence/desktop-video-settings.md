# Desktop video settings evidence

Scope: the desktop GUI scale modifier and fullscreen setting. The semantic
reference is Bedrock 26.30 behavior. The current client
and codec target are defined by [bedrock-target.json](../../assets/bedrock-target.json);
the pinned resource pack is defined by
[vanilla-source.json](../../assets/vanilla-source.json). The desktop behavior is
transferred to that target; this is not a claim of complete version-matched or
cross-platform parity.

## GUI scale

### Vanilla rules

| Rule | Behaviour |
| --- | --- |
| Optimal GUI scale | Desktop viewport thresholds produce a zero-based scale index. |
| Maximum GUI scale | Use the same desktop thresholds. |
| Optimal index bound | Bound the optimal index against the current maximum. |
| Modifier choices | Offer viewport-dependent signed modifiers. |
| Modifier application | Apply a modifier relative to the optimal index. |
| Scale mapping | Clamp the modifier before mapping the final index to a scale. |
| Effective modifier | Report the modifier after viewport clamping. |

Vanilla's GUI scale values are `[1, 2, 3, 4, 5, 6, 7, 8]`. Desktop minimum
viewport dimensions are height 250 and width 376; the half-range factor is 0.5.

For an ordinary desktop viewport without safe-zone adjustments, the optimal and
maximum physical scale are `K = clamp(min(width / 376, height / 250), 1, 8)`
using integer division. Settings offer physical scales from `ceil(K / 2)` through
`K`, represented as signed modifiers from `ceil(K / 2) - K` through zero. The
selected modifier is retained across resizing; its applied value is clamped to
the current range. A fixed command-line scale remains a capture override until
the player selects a modifier in settings.

The pinned pack's `ui/settings_sections/general_section.json` defines
`gui_scale_slider@settings_common.option_slider` with control name `gui_scale`.
Its bindings are `#gui_scale`, `#gui_scale_steps`, `#gui_scale_slider_label`,
`#gui_scale_visible`, and `#gui_scale_enabled`. The selected slider position is
the index within the current modifier choices.

## Fullscreen

Setting and toggling fullscreen operate on the same
boolean option. Settings and the requested F11 shortcut therefore share one
window state rather than maintaining independent toggle values.

The same pinned JSON file defines
`fullscreen_toggle@settings_common.option_toggle`, control name `full_screen`,
state binding `#full_screen`, enabled binding `#full_screen_enabled`, and
visibility variable `$show_fullscreen_toggle`.

The Linux host adapter enters Bevy borderless fullscreen on the current monitor
and returns to `Windowed`. F11 handles non-repeated presses for the focused
primary window and remains available when menu input consumes gameplay keys.
Writing back the applied fullscreen state does not publish an unrelated complete
settings replacement, preserving camera and automatic VSync policies.

## Settings spacing

The pinned pack's settings selector and video section add 25 virtual-pixel
spacers when `$settings_spatial_pattern_fix_enabled` is true. Consecutive
spacers survive between video options hidden by their own bindings, producing
the large empty region before Brightness. The desktop context now selects the
pack's compact branch, preserving its normal control sizes and section insets.

The settings controller derives this variable from a FlightReader boolean and Realm edit/slot state.
It is not an unconditional desktop flag. The compact branch is a deliberate
local choice: matching Bedrock's service-controlled treatment rollout remains
unverified and incomplete. No JSON-UI layout rule or downloaded pack is changed.
The pinned-template regression checks adjacent navigation rows and the gap
between Graphics Mode and Brightness at two virtual viewport sizes.

## Validation scope

Focused tests cover scale geometry and pointer conversion, viewport-dependent
choices, fullscreen settings/hotkey synchronization, repeat and focus handling,
preservation of unrelated settings, durable preference restoration, and dragging
through native scrolling and scale-induced relayout. Derived display scales use
the text cache's existing fixed-point precision instead of user preference bounds:
physical GUI scale 1 at DPI 2 requires font scale 0.25. On Fedora, all 32 focused
application tests and 26 UI geometry, action, and text tests passed with the
prepared JSON-UI carrier available. They include native-menu control geometry,
font quads, HUD relayout, and pointer alignment at DPI 2. Formatting and the
architecture gate also passed.
The installed release was built with this patch against repository revision
`3c2c142754bc91b32650dd1262198663b3ecd04a`, before integration with the latest
`main`.
That build passed a live Fedora GNOME/Xwayland pass on 2026-10-01 at
DPI 2, with 1280x720 and 1920x1080 windows and 3840x2560 fullscreen. Checks cover
checkbox/F11 synchronization, held-key repeats, restored window geometry, native
GUI scales 1 through 4, continuous dragging through relayout, pointer alignment,
viewport clamping, and preference restoration across app restarts. Fresh frames
were inspected for legibility, geometry, clipping, layering, scaling, and colors.
The final minimum GUI step renders half the text and control height of the next
step at the same 1280x720 viewport. The installation's binary hash, full scenario
record, and untracked screenshots are retained in
`~/.local/share/cinnabar/logs/video-settings-qa/qa-report.json`.

Language-specific minimum-scale dialogs, safe-zone variants, touch/console rules,
and full native parity are outside this desktop wiring change.
