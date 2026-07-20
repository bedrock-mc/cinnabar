# Celestial Compositing Parity Design

## Goal

Remove the dark square surrounding the sun and moon without erasing dark lunar detail, while making the exact composition rule testable against the decoded atmosphere asset bytes.

## Evidence and chosen behavior

The pinned Bedrock sun and moon PNGs are fully opaque. Their nominally black borders are not uniformly zero: the sun border includes `(1, 1, 0)` texels and the moon atlas borders include `(0, 0, 1)` texels. The current shader converts every non-zero RGB texel to alpha one and then replacement-composites it over the sky, so those border texels become dark opaque quads.

The installed Bedrock renderer identifies the material as `SunMoon` in the `Transparent` pass and supplies opaque, black-backed images. The compatible composition is emissive addition: black contributes nothing, near-black contributes negligibly, and real lunar pixels retain their internal dark values instead of being discarded by an RGB key. Cinnabar will therefore add `sampled.rgb * quad_coverage * horizon_visibility` to the current sky color. It will not derive coverage from RGB and will not replacement-mix the source over the sky.

The composition remains inside the existing opaque fullscreen atmosphere pass. SDR targets clamp naturally; HDR targets retain the additive energy for Bevy's existing tone mapping. Sun/moon UV mapping, phase selection, horizon visibility, and the single identity-stable atmosphere bind group remain unchanged.

## Boundaries

- This change fixes only sun/moon composition and evidence. It does not redesign the sky gradient, light engine, moon phase order, or cloud rendering.
- The exact problematic border colors are regression inputs.
- Tests decode a real `MCBEATM1` envelope assembled from deterministic texture bytes, then inspect every sun border and every moon-tile border before exercising the same non-darkening composition rule.
- Shader validation must prove both bodies call one shared additive helper and that no RGB-derived opacity or `mix(colour, sun/moon...)` path remains.
- Startup logs must include the already-computed atmosphere asset identity and a stable shader-source identity so stale executables can be distinguished during live acceptance.
- No Mojang texture or generated atmosphere blob is committed.

## Verification

The focused Rust tests must cover decoded border traversal, exact `(1, 1, 0)` and `(0, 0, 1)` regressions over bright and dark skies, moon-detail retention, WGSL parse/validation, and startup identity formatting. Full render and app tests, strict Clippy, rustfmt, and an independent review gate the merge. Final visual acceptance uses a fresh `%TEMP%` GDI screenshot from the stable client executable.
