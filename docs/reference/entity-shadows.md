# Entity shadows

Vanilla draws no shadow texture. Each caster hangs a polygonal volume under its feet, and visible
opaque surfaces inside a volume have their encoded colour multiplied by one shadow colour
when the camera is outside that volume.

## Vanilla rules

| Rule | Vanilla |
| --- | --- |
| Shape | 13-sided frustum, in caster radii about the feet: top ring radius 0.75 at y +0.01, bottom ring radius 0.25 at y −3. |
| Footprint | A surface `d` below the feet is shaded within `r · (0.25 + 0.5 · (3 − d/r) / 3.01)`; nothing below 3r, nothing above 0.01r. |
| Receivers | Every surface in the depth buffer after opaque terrain and opaque actors: block tops, sides past an edge, slabs, partial blocks, other actors. Translucent surfaces (water, clouds) are drawn later and receive none. |
| Camera inside | A volume containing the camera draws no shadow: there is no visible entry face to mark the receiver. |
| Opacity | Uniform across the footprint; no fade with height or distance. |
| Blend | `scene × colour` on the encoded (gamma) colour; overlapping volumes shade once. |
| Colour | 0.7 grey. Tint = lerp(sky × 0.5 + 0.4, sunrise rgb, sunrise alpha); each channel is offset by 0.03 × (tint − luminance) / max |tint − luminance| (Rec. 709 luminance). A grey tint leaves 0.7. Alpha 1. |
| Lighting | No light term: the multiplier applies to the already-lit colour. |
| Radius | Collision-box width (streamed, so already scaled and baby-sized). Players without streamed bounds use 0.6. |
| Radius overrides | ghast, happy ghast, creaking ×0.8; spider, cave spider ×0.7; armadillo ×0.575; horse, donkey, mule, skeleton and zombie horse ×0.6; ender dragon ×0.3; tadpole ×0.5; iron golem, shulker ×0.5 (×0.25 as babies); baby turtle ×0.33; slime, magma cube, sulfur cube: variant × 0.25; tripod camera, end crystal 0.5; boat, chest boat 1.0; parrot riding anything 0; spectator player 0. |
| No shadow | armor stand, area effect cloud, fishing hook, every minecart, XP orb, leash knot, eye of ender, lightning, primed TNT, falling block, firework, painting; every projectile type (arrows, tridents, thrown items, potions, fireballs, skulls, llama spit, shulker bullet, wind charges, evocation fang, ice bomb). |
| Hidden by state | Dead or zero health, on fire, invisible, breathing point under water or lava, riding a vehicle that is not invisible. |
| Drop | Ghast −0.875 and happy ghast −0.5 of height × scale below the feet. |
| Casters | Only actors in the frame's actor render list. Signs in some block states cast a 0.2 shadow from the block centre (not implemented). |
| Setting | None: the `options.entityShadows` string has no control; always on. |

## Cinnabar

- `render_model::EntityShadow` owns the volume and colour; `client_world` owns caster rules;
  `render::EntityShadowRenderPlugin` copies the opaque scene and draws every volume's back faces
  in one instanced draw. Volumes containing the camera are clipped in the vertex stage;
  remaining volumes test the depth-buffer point in the fragment.
- Rigged casters, including the local player, are the bodies the actor frame drew. First person
  hides the local body and its shadow; third person uses the body's render-time feet.
- Provisional: dropped items are culled by their volume; the breathing point is the eye at
  0.9 × height. These remain unconfirmed. Exact near-plane clipping and slope-bias raster
  edges also remain incomplete parity checks.
