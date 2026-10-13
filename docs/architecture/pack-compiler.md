# Reusable pack compilation

Compilation lives in four ordinary workspace packages. Each caller imports APIs
from the crate that owns them; there is no combined compiler facade.

- `pack-compile-input`, under `crates/pack-compiler/input`, owns bounded pack
  sources, JSON validation, source handles, texture decoding and selection.
- `pack-compiler` retains terrain, materials and independent carrier compilers
  such as atmosphere, audio, language and UI.
- `pack-entity-compiler`, under `crates/pack-compiler/entity`, owns entity, actor,
  equipment, icons, starter skins and block-entity artwork.
- `pack-font-compiler`, under `crates/pack-compiler/font`, owns bitmap and outline
  font compilation, including bundled FreeType rasterization.

Terrain and entity compilation both consume shared inputs and have no production
edge to each other. Font compilation depends only on `assets` within the workspace.
All four produce engine-independent `assets` data without Bevy or wgpu.

`asset-compiler` owns the `assetc` CLI and composes the needed compilers directly.
`pack-runtime` uses input and entity compilation for session packs. Renderer,
mesher and client fixtures keep compiler dependencies development-only.

The split preserves input bounds, ordering, source identities, malformed-file
skips, fallback rules and carrier formats. Tests move with their implementation;
CLI suites stay with `assetc`. Its source fingerprint traverses each reached
package separately, excludes package-root test directories and includes literal
embedded inputs even when their directory is excluded from the source scan.

## References and parity scope

This changes ownership only and closes no vanilla behavior or visual parity gate.
The existing compilation behavior and its source comments are retained.

## Vanilla rules

| Rule | Behaviour |
| --- | --- |
| Resource lookup | Load a resource from the selected pack stack. |
| Atlas metadata | Load atlas metadata and read its `texture_data` entries. |

These input roles are not a new current-version parity claim.

- The installed vanilla pack was read through the worktree's `.local` symlink:
  `.local/assets/bedrock-samples/v1.26.50.4/full/resource_pack/textures/terrain_texture.json:3`
  identifies `vanilla`, `atlas.terrain`, and the `texture_data` table. No pack
  source files were copied into the repository or modified.

The dependency and module restrictions for this step are registered in
`tools/architecture/policy.toml`; the shared architecture gate checks them during
`verify-affected`.
