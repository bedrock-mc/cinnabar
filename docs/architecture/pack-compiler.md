# Reusable pack compilation: restructuring step 6

`pack-compiler` owns the reusable resource-pack compiler that previously lived in
the `asset-compiler` library. It reads bounded pack inputs and produces the same
engine-independent `assets` catalogs, textures, geometry, animation programs,
reports and carrier bytes. Its only internal production dependency is `assets`.
It has no command-line parser, renderer, world owner or session lifecycle.

`asset-compiler` now contains only the `assetc` command-line program. Argument
parsing, output-path validation, atomic output bundles and CLI reports stay there.
It depends on `pack-compiler` and `assets`. The command name, arguments and output
formats are unchanged. The app's session-time entity, actor and icon compilation
calls `pack-compiler` directly. Renderer and mesher compilation fixtures use it as
a development dependency. There is no compatibility library in `asset-compiler`.

The extraction preserves compiler entry points, input bounds, deterministic
ordering, source identities, malformed-file skips and all existing fallback
rules. The tracked legacy fallback table moved with the compiler without changing
its bytes; its provenance records and registry rekey tools point to its new path.
`make assets` tracks both the reusable compiler and the CLI sources so edits to
either invalidate generated carriers.

Unit tests and independent library integration suites moved with the compiler.
Suites that exercise `assetc` alongside library calls remain in `asset-compiler`
and import `pack-compiler` directly. Synthetic fixtures remain independent of
installed assets. Optional local font qualifications return early when their
source or carrier is absent. Scratch output remains outside the installed pack.

## References and parity scope

This changes ownership only and closes no vanilla behavior or visual parity gate.
The existing compilation behavior and its source comments are retained.

- Lens source search for `terrain_texture.json` in reconstructed client
  `1.26.50.26` returned derived-source matches at RVAs `0xade020` and `0x7f5ba0`.
  The subsequent batched function reads reported that the analysis service was
  unavailable; the search results are discovery evidence, not a new verification
  of those functions' behavior.
- **R:ResourcePackManager:488** in the 26.30 reconstruction loads a resource from
  the selected pack stack. **R:TextureAtlas:908** loads atlas metadata and
  **R:TextureAtlas:1839** reads its `texture_data` entries. These older references
  explain the input roles; they are not current-version parity proof. Reference
  root: `~/coding/go/lunar/refs/mcsrc-1.26.50/reference/26.30/src/by-owner`.
- The installed vanilla pack was read through the worktree's `.local` symlink:
  `.local/assets/bedrock-samples/v1.26.50.4/full/resource_pack/textures/terrain_texture.json:3`
  identifies `vanilla`, `atlas.terrain`, and the `texture_data` table. No pack
  source files were copied into the repository or modified.

The dependency and module restrictions for this step are registered in
`tools/architecture/policy.toml`; the shared architecture gate checks them during
`verify-affected`.
