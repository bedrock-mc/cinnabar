# Trusted packet-mod menus

A WASI preview 1 packet reactor may export `menu(i32, i32) -> i32` alongside
`native-init(i32)`. The host sets capability bit `2` only when the mod has the
controls grant. Camera frames use bit `1` independently. Reactors without either
optional callback retain their existing packet ABI.

`menu` receives UTF-8 JSON bytes. Its return uses the same canonical
`result<list<u8>, string>` layout as `frame`: a four-byte discriminant, then the
pointer and length of the successful byte list or error string. Exporting
`cabi_post_menu(i32)` releases the returned allocation after copying it.

Input is bounded to 64 KiB:

```json
{"version":1,"open":true,"viewport":[640,400],"events":[{"id":"enabled","value":1}],"keys":[{"key":"KeyA","text":"a"}]}
```

The optional `viewport` is the available authored GUI size before reference-size fitting; it follows window resizing and platform safe-area insets.

Version one starts with only these original fields, including for guests that
reject unknown JSON fields. To accept additional facts, return optional output
`input_extensions:["pointer_state","shift_state","hud_geometry","control_previews"]`. The next
input includes only those groups: `pointer_state` adds `pointer_held` and
`pointer_cancelled`, `shift_state` adds `shift_held`, and `hud_geometry` adds its
map. `control_previews` adds the `preview_sizes` map described below. Omission retains the accepted groups; an empty array clears them. Requests
must contain unique known groups. A new session starts with no extensions.
Each group's field set is frozen for version one. Future input facts require a
new opt-in group so strict decoders that accepted an existing group remain valid.

The host delivers native control IDs and values. Focused menu keys may carry
composed text; native number and choice editors receive their keyboard input
first. `capture_key` lets the guest handle Escape and the panel's toggle key while
open. The toggle key reopens the retained menu while closed. Other screens and
unfocused windows receive no guest menu keys.

Text controls are rejected before the panel is published. Version one delivers
numeric control events; committed text values need a separate input contract.

Output is bounded to 512 KiB. JSON `null` retains the current menu. A changed
output contains `open`, optional `panel` using `ui::mod_panel::Panel`, and optional `keys`:

```json
{"open":true,"panel":{"title":"Example","toggle_key":"ShiftRight","dark":true,"controls":[]},"keys":["KeyR"]}
```

Panel `show_gameplay_hud: true` keeps the resource-pack gameplay HUD visible behind
an open panel, for example for a placement editor. It defaults to false and can
change with full panel updates while retaining pointer capture. The player's Hide
HUD setting and death screen still take priority; a focused panel always hides
the crosshair. Other extensions' overlay cards remain suppressed during menus.

Omitting `panel` retains its previous document, allowing HUD-only updates before or
while the menu is closed. Optional `hud_bindings` supplies a bounded map of trusted
resource-pack HUD globals: names begin with `#mod_`, values are booleans, finite
numbers or numeric vectors of two to four components. An omitted map retains the
previous values; an empty map removes them. The host overlays these values without
mutating the shared server HUD data, and retires them with their packet session or
grants. HUD overrides do not require the menu to be visible.

Optional output `hud_geometry:["example_clock"]` requests the last painted bounds
of up to 128 unique inert resource-pack HUD `input_panel` instance names. Names
are opaque ASCII identifiers, at most 96 bytes; mapped controls are excluded.
Omitting the list retains it, and an empty list clears it. After opting into the
`hud_geometry` input extension, input returns a
`hud_geometry` map, for example:

```json
{"example_clock":{"rect":[12,24,80,27],"size":[40,13.5]}}
```

`rect` contains x, y, width and height after composed ancestor paint transforms;
`size` contains the root's layout width and height before paint transforms. Both
use the native panel GUI coordinate basis with safe-area origin removed. Missing,
hidden, ambiguous and stale-session roots are omitted. Geometry is a completed
frame observation and can lag one frame behind updated bindings.
Selectors match the engine's resolved instance name. For an authored literal
name, underscores avoid JSON-UI interpreting a leading `namespace.` qualifier.

An authored surface may declare `previews`, keyed by opaque preview ID. A
noninteractive custom node with `renderer:"cinnabar_control_preview"` and literal
`preview_id` contains the corresponding real pack control inside its rectangle:

```json
{"previews":{"example":{"reference":"example.actual_widget","variables":{"$compact":true},"globals":{"#example_text":"First line\nSecond line"},"collections":{},"size":[0,0]}}}
```

`reference` selects a qualified control in the current composed pack catalog.
`variables` supplies root `$` overrides; `globals` supplies independent `#`
bindings; `collections` maps collection names to arrays of `#`-keyed row objects.
These values never update gameplay HUD bindings or settings. `size` supplies a
reference canvas in GUI units; zero components measure content on a bounded
canvas. The host uses the same text, image, native HUD, item and player renderers
as the original control, preserves aspect ratio and clips to the destination.
Preview focus, hits and actions are discarded, leaving the enclosing menu control
as the input target. Nested filter/backdrop composites are not replayed.

At most 32 previews and 128 KiB of aggregate fixture data are retained per
surface. Fixture strings allow newlines and up to 16 KiB; ordinary surface
scalar limits remain unchanged. Resolution, factory prototypes and expanded
binding are bounded to the surface node budget. Missing or rejected controls
produce no thumbnail. Declaration changes invalidate their prepared source and
preview cache; sessions, owners, language, font and texture changes remain
isolated. Closed surfaces retire the cache after their exit settles.

After opting into `control_previews`, input includes
`preview_sizes:{"example":[48,28]}` for successfully measured current previews.
Dimensions cover the emitted artwork, including overflow beyond the root's
layout rectangle, before contain fit or parent paint transform. They use the
same native-panel GUI basis as `hud_geometry.size`. Texture transparency is not
sampled when measuring conservative painted bounds. Missing or stale
measurements are omitted. Measurements are delivered only to the packet session
that owns the currently painted declarations; another owner's previews cannot
be observed. This group's version-one field set is frozen like the others.

Trusted HUD catalogs support `cinnabar_position:[x,y]` or its
`#cinnabar_position` vector binding. Each normalized component selects the
remaining travel inside the parent after the widget's transformed bounds, keeping
scaled widgets within the parent. Ordinary resource-pack catalogs ignore this
native extension. The host enables it for a HUD with active trusted mod bindings.

An authored `surface.screen` identifies a logical route. Catalog revisions of the
same screen retain binding tweens; a different screen starts its declared entry.
Closing animates the displayed surface before a retained future route is installed.

`keys` subscribes to physical key press edges while focused gameplay owns input.
Those keys are reserved from ordinary gameplay, never collected from chat or
another menu. Each native packet session has its own menu identity, input queue
and prepared catalog; reconnecting or revoking grants retires its pending input
and retained presentation. Calls run on the existing serial packet worker.
Consecutive held-pointer edits of declared sliders retain their latest values
while the worker is busy. Buttons, keys, cancellation, visibility changes and
newly delivered panels preserve their ordering boundaries.

Custom panels support surface bindings, eased transitions, glass and paint
transforms through the shared JSON-UI engine. The `mod_panel_sans` font alias selects the shipped Inter outline face. `mod.control:N` invokes declared
control N. `mod.vector:X:Y` maps pointer position within its hit region to two
slider controls. `mod.drag:X:Y` preserves the initial grab offset and maps pointer
deltas across the available authored travel (reference dimensions minus the
handle's painted bounds) to the same bounded sliders. Capture freezes that travel
until release, so position and scale updates cannot change its gain.
These routes use the full authored root coordinate space; keep an interactive
placement canvas untransformed and size it to that reference. A widget's own
paint scale is included in its available travel.
Relative routes emit their current numeric values on press, without rounding or
moving them, so a guest can begin selection and an exact gesture rollback snapshot.
`mod.drag_axis:N:x` or `mod.drag_axis:N:y` changes one slider relatively across the
full authored reference dimension, suitable for a scale handle without a grab jump.
An optional positive reference extent, as in `mod.drag_axis:N:x:112`, replaces
that dimension when an authored handle needs a specific sensitivity.
The extent may instead name a declared positive numeric surface binding, as in
`mod.drag_axis:N:x:#resize_extent`; its value is captured on press, so geometry
updates during the gesture keep the same sensitivity and stable document.
Input `pointer_held` reflects focused menu capture, including release and focus
loss without another control edit.
`pointer_cancelled` marks an interrupted accepted gesture, such as lost focus,
cursor loss or another screen taking input. It is delivered once, including when
several samples coalesce; normal release leaves it false. `shift_held` is the
current focused physical Shift state. Ordered visibility phases retain earlier
edits while rejecting their stale panel results, so closing before a worker poll
cannot silently lose a final HUD edit.
The host owns drawing, input routing and animation; the guest owns feature state.

The Mods library detects the native callback permissions of raw reactors when
no explicit `cinnabar:grants` section exists. A native menu requests Controls;
native camera frames request Players and Camera. These requests still require
user consent and apply to a new server session. An explicit grant section stays
authoritative, including when it omits optional native capabilities.

For a window-free layout capture, run:

```sh
cargo run -p ui-snapshot -- --panel panel.json --out panel.png \
  --width 1280 --height 720 --gui-scale 2 --time 1 --carriers /path/to/compiled
```

The CPU snapshot path uses the production panel painter and local validated
carriers. A headless client capture verifies the GPU glass/backdrop appearance.
Pack-control previews require the full presentation runtime, including composed
pack fonts and player-renderer resources; the optional reactor painter scenario
installs these through their normal owners.
