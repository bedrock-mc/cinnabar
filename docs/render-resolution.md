# Render resolution

Open **Settings > Video > Render Resolution** to choose the world rendering size,
from 10% to 100% of each output dimension. For example, 50% renders a 1920 × 1080
world at 960 × 540 before upscaling it. The default is 100% on every device.
Your choice is saved for later sessions. Video presets preserve it; resetting
Video settings restores 100%.

Upscaling uses FSR1 EASU. **FSR Sharpness** enables its RCAS sharpening pass;
Off skips RCAS. Bilinear filtering covers pending or failed FSR pipelines.
Upscaling pipelines compile only when scaling is enabled; none run at 100%.

Menus, HUD text, crosshairs and inventory previews render at the output's native
resolution. The hand and world-space nametags render with the world, including their
depth tests. Mouse input, picking and aim keep the output's original coordinates
and camera projection. Screenshots and recordings keep native output dimensions
and include the upscaled world and HUD. If native UI composition is unavailable,
the UI draws with the scaled world so Settings remains reachable.

Shader packs and Enhanced world effects receive the scaled view dimensions.
Scaling applies to a single full-window game camera, including HDR views. The
launcher, partial viewports and multiple cameras sharing one output retain their
native rendering size.
