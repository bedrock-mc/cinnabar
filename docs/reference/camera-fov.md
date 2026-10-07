# Camera FOV projection

Vanilla scales the configured angle, with and without gameplay modifiers, by
`min(normalized_viewport.y / normalized_viewport.x, 1)`. The normalized viewport
divides each viewport dimension by the corresponding full-screen dimension, so
the values are `(1, 1)`
for a full-window viewport at any display aspect ratio. This scale adjusts
partial viewports, such as split screen; it is not pixel height divided by
pixel width.

The camera converts that angle to radians and builds a right-handed perspective
projection from the viewport width and height separately. That projection places `cot(FOV / 2)` on the vertical axis and divides it by aspect
on the horizontal axis. The full-window setting therefore defines the
vertical FOV. A setting of 110 degrees stays 110 degrees vertically, about
137 degrees horizontally at 16:9.

Cinnabar previously treated this setting as a horizontal angle and converted
it through `2 * atan(tan(FOV / 2) / aspect)`. At 16:9, setting 110 consequently
rendered only about 77.55 degrees vertically. The camera now uses the setting
directly as vertical radians and supplies aspect independently. The legacy
`horizontal_fov_degrees` settings field and accessor remain compatible with
existing saved settings; their name does not describe the corrected axis.

Tests cover configured angles, landscape and portrait projection matrices,
the live 110-degree settings handoff, malformed values, startup, and window
resize. This establishes the full-window base projection contract. Dynamic
gameplay FOV magnitudes remain provisional, and split-screen viewport scaling
is not implemented by the current single-window client.
