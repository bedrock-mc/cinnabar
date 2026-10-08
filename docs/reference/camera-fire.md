# Burning camera effect

The first-person burning effect uses the active fire block's animated Down
binding on five faces of a camera-relative cube. The rendering target is
Vanilla Bedrock 1.26.50.26.

## Vanilla rules

| Property | Behavior |
| --- | --- |
| Geometry | Cube coordinates ±1, with its top omitted. |
| Transform | Cancel player yaw/pitch; rotate Y by 45 degrees, translate Y by -0.5, scale uniformly by 0.7. |
| Color | White RGB, alpha 0.9, multiplied by the sampled texel. |
| Texture | Fire's destruction/Down binding, `fire_1`, including pack overrides. |
| Sampling | Original point-sampled texels with perspective-correct face UVs. |
| Animation | Retain frame order, repeated frames, ticks per frame and interpolation on the shared terrain clock. |
| Composition | After first-person geometry and before the JSON-UI HUD, including Enhanced's post-grade route. |
| Depth/blend | Always pass, no depth writes, alpha blending. |

`ScreenFireTexture` resolves the admitted material and preserves its original
texels and timeline. Repeated frames share GPU layers; asset identity changes
rebuild the texture. Unavailable world art leaves this optional effect absent.

The shader intersects the active perspective camera ray with the transformed
cube, preserving independent face UV orientations, its open top and the active
FOV/aspect ratio. The crosshair, hotbar and paper doll render afterward.
[HUD actor flames](hud-paper-doll.md) use a separate atlas, geometry and draw clock.

## Acceptance limits

Focused regressions cover binding selection, cross-page art, frame storage,
timing, interpolation, metadata gating and post-hand composition. Accepted live
frames are recorded in `plan.md`. The installed Vanilla app is 1.26.51.01;
that comparison does not close the target-version rendering parity gate.
