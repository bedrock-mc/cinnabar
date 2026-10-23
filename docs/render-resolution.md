# Render resolution

Open **Settings > Video > Render Resolution** to choose the world rendering size,
from 10% to 100% of each output dimension. For example, 50% renders a 1920 × 1080
world at 960 × 540 before upscaling it. The default is 100% on every device.
Your choice is saved for later sessions. Video presets preserve it; resetting
Video settings restores 100%.

Upscaling uses bilinear filtering. **Sharpen Upscaling** adds a light sharpening
filter by default. Turn it off to reduce the filter's cost or prefer a softer image.
Neither filter runs at 100%.

Menus, HUD text, crosshairs and inventory previews render at the output's native
resolution. The hand and world-space nametags render with the world, including their
depth tests. Mouse input, picking and aim keep the output's original coordinates
and camera projection. Screenshots and recordings keep native output dimensions
and include the upscaled world and native HUD.

Shader packs and Enhanced world effects receive the scaled view dimensions.
Scaling applies to a single full-window game camera, including HDR views. The
launcher, partial viewports and multiple cameras sharing one output retain their
native rendering size.
