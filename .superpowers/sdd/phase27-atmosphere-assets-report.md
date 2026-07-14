# Phase 2.7 Atmosphere Asset Carriage Report

Date: 2026-07-14
Base commit: `46a4e9f41b9b9d23249a0bfd51cf50b8bfa63b9a`
Branch: `phase27-atmosphere-assets`

## Outcome

The local-only asset toolchain now compiles the pinned vanilla sun,
moon-phase sheet, and cloud texture into an independent, bounded
`MCBEATM1` runtime blob. No render, shader, plugin, app, or GPU file changed.
Mojang payloads and generated local outputs remain untracked.

Cloud data is included because the pinned Mojang pack supplies the exact
authoritative `textures/environment/clouds.png` source and its 256x256 decoded
payload fits the same fixed three-record carrier. This change does not define
cloud rendering, motion, UVs, or blending.

## Source provenance

Tracked source manifest: `assets/vanilla-source.json`

- tag: `v1.26.30.32-preview`
- commit: `020f1cf4b2baef78e635d4ce7498eb16a429dcbb`
- archive: `bedrock-samples-v1.26.30.32-preview-full.zip`
- archive SHA-256:
  `12d5cddc03acd507e9e0bd412f2e94d34d0a1a855758af7a9eef61b03630ad7c`
- source-manifest SHA-256:
  `0cc3e494d634cf3f9c0795d526b9f91e973dfe1009aae50b8db4418f2386304d`
- artifact policy: `local-only`

Pinned resource-pack records, in canonical binary order:

| Role | Exact source path | Dimensions | Encoded bytes | Source SHA-256 | Decoded RGBA8 SHA-256 |
|---|---|---:|---:|---|---|
| sun | `textures/environment/sun.png` | 32x32 | 523 | `f7273544b691f08aaef76373d526e00793cf1e1aa0e1df8518f738d44a8e526b` | `854ae0f412cf6e441b0d9e742b5fca358fb99edb4ccab6e7af8ea4776b4567c1` |
| moon phases | `textures/environment/moon_phases.png` | 128x64 | 1,142 | `01c566d48e0cc8618cf6fdce811b61175fc246f12f2e8f2c567d6acd3a2b35d8` | `bda31044936525a46afcb0242db04149e4116e7bb24fe21151688997a0bec9fa` |
| clouds | `textures/environment/clouds.png` | 256x256 | 8,927 | `4f57cfe866779ef82be0058e244a77b0a279ee75e9eb40ac9ce6eb372445adc8` | `703542c95b24b30090043a99b88f52d2ff6d887f7168bb2210a858e0f359e634` |

No BDS replica, decompiled/reverse-engineered source, Zuri data, or collision
shape is used by this carrier.

## Binary and runtime contract

`MCBEATM1` is separate from the shared `MCBEAS05` world-asset schema. Its
canonical layout is:

1. a 128-byte header containing magic/version/count, the source-manifest
   SHA-256, and four checked section offsets;
2. exactly three 112-byte descriptors containing role, dimensions, RGBA8 sRGB
   format, source-path range, encoded source size, pixel range, source SHA-256,
   and decoded-pixel SHA-256;
3. exact UTF-8 source paths in role order;
4. exact decoded RGBA8 payloads in role order; and
5. a trailing SHA-256 over the complete preceding envelope.

The pinned output is 299,599 bytes with SHA-256
`0fef7cab3c6b420af08517f8f0c7b5c98556ba15aeb2961df9fcd16c3df3470c`.

`RuntimeAtmosphereAssets::decode` validates the complete envelope and all
fixed metadata before copying bounded pixel payloads. Compilation rejects a
missing source, a malformed PNG, encoded input above 1 MiB, wrong dimensions,
invalid/oversized manifest input, and non-local or malformed provenance.
The provenance validator composes and requires the official Mojang Bedrock
Samples release URL from the manifest tag and archive rather than accepting an
arbitrary HTTPS origin.

`assetc atmosphere` writes both the ignored blob and a deterministic JSON
report using per-file atomic replacement. The report contains the complete
parsed source manifest, manifest and blob hashes, and every per-texture field
above. It intentionally excludes machine-local canonical paths.

The exact publication guarantee is per-file atomicity, not impossible
cross-file crash atomicity: both blob/report byte payloads are completely built
and both destinations are preflighted before the first write, then each uses a
same-directory temporary file and rename. A known-invalid report destination
therefore leaves an existing blob/report destination untouched. If an
unexpected second-file I/O failure occurs after the blob rename, Make sees the
missing or older report and reruns the deterministic pair.

Build integration uses one portable producer command. `make assets`, the
explicit `make atmosphere-assets`, and `make client` depend on both outputs.
The report has a dependency on the blob, which serializes `make -j` without the
GNU Make 4.3-only grouped-target syntax. Deleting either output reruns the same
compiler command; no Mojang source becomes a tracked prerequisite or output.

## TDD and verification

RED was observed before each production slice:

- compiler tests first failed on missing atmosphere APIs and error variants;
- blob/runtime tests first failed on the absent accessors/decoder;
- CLI/report tests first failed with `unrecognized subcommand 'atmosphere'`;
- encoded-source-size coverage first failed because the field was absent; and
- strict manifest coverage first failed because the oversized-manifest error
  did not exist;
- official-source coverage first failed because an arbitrary HTTPS origin was
  accepted before the Mojang release URL was constrained;
- bundle publication coverage first proved the blob was replaced before an
  invalid report destination failed; and
- Make integration first failed because the manifest/blob/report freshness
  contracts were absent, then rejected the unsafe ordinary multi-target rule.

Focused pinned verification passed all seven atmosphere integration tests,
including exact source and pinned blob hash ratchets. Standard full assets
verification passed 244 tests with zero failures and eight existing ignored
opt-in tests.

For completeness, setting `PINNED_VANILLA_PACK` for the entire legacy assets
suite activates unrelated pack-wide acceptance tests. That run passed the new
atmosphere suite but exposed the existing
`compiler_real_pinned_pack_preserves_checked_transparent_cubes_with_exact_huge_mushrooms`
expectation (`16` materials observed versus historical `43`). No changed file
touches that compiler path; the normal full regression gate and the focused
pinned atmosphere gate are green.

## Renderer handoff

Renderer/GPU ownership remains responsible for:

- loading the ignored `MCBEATM1` alongside the existing runtime assets;
- creating stable GPU textures once from `sun`, `moon_phases`, and optionally
  `clouds` records;
- binding them to the atmosphere pass without per-frame upload or bind-group
  churn;
- selecting the correct moon-phase cell and celestial UV/orientation;
- integrating the authoritative cloud tile only after shader/render ownership
  adjudicates camera-relative movement, fog, alpha, and native parity; and
- failing closed on a configured carrier that is absent or does not pass the
  `RuntimeAtmosphereAssets` decoder, rather than reinterpreting damaged bytes.
