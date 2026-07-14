# Phase 2 resin-clump native reference

Date: 2026-07-14

Native client/BDS release line: 1.26.33.1

Pinned resource pack: `bedrock-samples` tag `v1.26.30.32-preview`, commit
`020f1cf4b2baef78e635d4ce7498eb16a429dcbb`

The local BDS reference cell was centered at `50 200 50`. Exact-state
`/testforblock` readback plus one-support removal established the face-bit table:

| Bit | Plane | Required support |
|---:|---|---|
| 1 | down | `50 199 50` |
| 2 | up | `50 201 50` |
| 4 | south | `50 200 51` |
| 8 | west | `49 200 50` |
| 16 | north | `50 200 49` |
| 32 | east | `51 200 50` |

Removing only the listed support made the one-hot resin clump become air;
removing the opposite support preserved the requested exact state. The six
views also establish the existing glow-lichen face-relative UV projection and
the 1/256-block attachment inset. Resin planes remain visible from both sides
and through transparent texels.

Writing mask 0 visually produced all six planes, but exact readback for mask 0
failed and exact readback for mask 63 succeeded. Cinnabar therefore retains all
64 protocol records while compiling mask 0 as a visual/template alias of mask
63. It does not claim that native preserves a loaded zero state.

Local-only native captures (not committed):

| View | SHA-256 |
|---|---|
| mask-0 write / mask-63 readback | `df723cd0f6bebc304017b271d48501a1801316d8c03d74dc4357b0535f2dcd37` |
| east, facing west | `246f9306c9fc2adddb71155bdac7fff7288b29c458606d093012955d6e184b0b` |
| west, facing east | `e3e2c1ed985ab45e6a1673f6cec1cc7552f99b3ce64ab9d638991c94b6b06712` |
| south, facing north | `3cca64685881a408e9de83123e1f55c0b6ee735e4d34e7744c7e89f40137bd37` |
| north, facing south | `0690ba2a2e22ea12f53cb8739cb4707e2584447b23913e6a31e4f72638705933` |
| above, facing down | `d91dfdde44beee5516d31464ab9d85393a417a83ff96226c8fc1d7707ec69bab` |
| below, facing up | `358647512a2d1cf87fb81ce0792bd5cc7cf6a60172199f5a5173e29c52b103fe` |

The exact Cinnabar compiler and mesh gates cover every mask, both network-ID
modes, all six subchunk boundaries, cave openness, opaque support visibility,
layered water composition, and the dense 4,096-reference/24,576-draw-light
bound. The matching-view live presentation acceptance and two consecutive
GPU-completed Cinnabar frames remain open; no native screenshot, Mojang image,
or resource-pack payload is stored in the repository.
