# Finite Vanilla Cloud Mesh Design

## Goal

Replace the infinitely thin fullscreen cloud plane with a finite, world-anchored cloud layer that exposes top, bottom, and side faces, remains bounded at all camera angles, and preserves the existing weather, fog, and absolute-time motion contracts.

## Native evidence

The installed matching Bedrock client provides a 256×256 `clouds.png` with exactly two alpha values: 52,180 texels use alpha 1 and 13,356 texels use alpha 255. Its renderer configuration names `cloud_mesh_size: 64`, quality-dependent `grid_size`, render-distance scaling, and cloud lighting. The `Clouds` material consumes positions, vertex colors, and optional instance transforms, has no cloud texture sampler, and contains transparent and depth-only passes. This establishes that the texture is an occupancy source for finite geometry rather than a color plane sampled per screen pixel.

## Alternatives considered

A bounded fullscreen DDA through a slab would expose side faces, but its work scales with screen pixels times traversed cells and would be most expensive at grazing angles—the exact view this gate must support. A volumetric raymarch would be still more expensive and invent density data absent from the vanilla occupancy texture. Bevy `Mesh` plus `StandardMaterial` would render geometry but introduce a second material/resource path and lose the compact immutable vertex-pulling contract. The selected greedy packed mesh follows the native geometry evidence, makes cost depend on exposed faces rather than resolution, and keeps one custom atmosphere-owned resource set.

## Architecture

At atmosphere-asset preparation, Cinnabar will classify alpha values below 128 as empty and values at or above 128 as occupied. It will mesh the periodic 256×256 occupancy field into exposed rectangular faces using deterministic binary/greedy merging. Neighbour checks wrap at both texture axes, so the generated geometry tiles without seam faces. The layer spans world Y 128 through 132: the existing altitude remains the underside seen from below and four blocks provide visible vanilla-style thickness.

The mesher emits one eight-byte packed record per quad. Word zero stores `axis0_start`, `axis1_start`, `axis0_extent_minus_one`, and `axis1_extent_minus_one` as four bytes. Word one stores the face ID in bits 0–2 and requires every reserved bit to be zero. Top/bottom records map the two axes to X/Z. North/south map axis zero to X and axis one to the fixed four-block vertical extent, with the Z plane in `axis1_start`; east/west use the symmetric Z-run/X-plane interpretation. The face lookup makes those interpretations explicit and reconstructs the fixed Y=128…132 bounds. The renderer uploads this immutable record array once per atmosphere asset identity. One cloud draw instances the periodic mesh in a bounded 3×3 arrangement around the camera. The instance origin snaps to the 256-block period; absolute Bedrock time supplies the existing +X offset of 0.03 blocks per tick modulo one period without rebuilding or re-uploading geometry.

Clouds use a dedicated custom pipeline, not Bevy `Mesh`/`StandardMaterial`. Reversed-Z world depth makes terrain and clouds occlude each other physically. The color pass uses the existing atmosphere weather and fog values, fixed face lighting (bright top, shaded sides, darker underside), and distance fog at the bounded tile edge. GPU resources are identity-cached: one immutable quad buffer, one pipeline family specialized for the view sample count/HDR target, and one bind group. No per-frame texture upload, mesh rebuild, bind-group creation, or per-cell draw is allowed.

The fullscreen atmosphere shader stops sampling `clouds_texture`; it remains responsible for the sky and celestial bodies. The `MCBEATM1` schema remains unchanged because the exact source texture and its provenance remain authoritative; cloud geometry is a deterministic runtime derivative of its validated pixels.

## Correctness and failure behavior

- Unexpected cloud dimensions fail atmosphere preparation rather than silently changing the 256-period contract.
- Empty and occupied alpha classes are exact and tested against alpha 1/255.
- An all-empty field emits no records; a toroidally all-filled field emits only merged top and bottom faces; isolated and adjacent cells prove all six orientations and internal-face culling.
- Packed records must round-trip their exact position, extents, and face ID and remain eight bytes.
- Output order is canonical by face and coordinate, with a stable digest for the installed source.
- The worst-case checkerboard record count and byte size are checked before allocation and bounded below the renderer arena policy.
- Cameras below, above, within, and at grazing angles see the correct finite faces; negative world coordinates and period crossings cannot pop or drift.
- Missing/malformed atmosphere assets retain the existing hard failure and rebuild instruction.

## Verification

Unit tests cover occupancy classification, toroidal adjacency, greedy merges, packed record ABI, fixed vertical geometry, snapped 3×3 origins, absolute-time motion, and bounded worst-case output. Render tests parse/validate both shaders and prove MSAA/HDR/depth specialization, stable GPU identity, one draw, and no residual fullscreen cloud sampling. The live gate uses the stable release client against BDS, records fresh `%TEMP%` GDI screenshots below/above/within/grazing, and compares them to matching native reference views. It also checks that settled frame time, CPU, RSS, and upload counters remain within the binding Phase 2 budgets.
