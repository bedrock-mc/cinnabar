# Loading animation

The pinned vanilla `ui/progress_screen.json` selects frames from
`textures/ui/loading_bar.png`, using its authored UV rectangle, frame count,
frame step and timing. Resource packs can replace both the image and animation.
Dimension changes retain their separate animation visibility rule.

Animated frame strips wait for their artwork to become resident. Static images
can use a reduced preview while full artwork loads; resizing a frame strip would
blend columns and frames before the authored UV selection. Strips exceeding an
artwork page retain its existing size-limited approximation; that parity remains
incomplete.

Focused regressions cover a reduced preview becoming resident at full resolution
and a strip that already fits the UI atlas. Larger custom strips keep drawing
once their artwork settles. Manual comparison of the loading screen remains pending.
