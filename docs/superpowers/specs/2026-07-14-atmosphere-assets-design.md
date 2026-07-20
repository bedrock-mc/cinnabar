# Phase 2.7 Atmosphere Assets Design

## Goal

Deterministically ingest and carry the pinned vanilla sun, moon-phase, and
cloud textures through the local-only asset toolchain without changing render
or plugin code and without tracking Mojang payloads.

## Source boundary

The only accepted inputs are the tracked `assets/vanilla-source.json` manifest
and these exact paths beneath its matching Mojang `bedrock-samples` resource
pack:

- `textures/environment/sun.png`
- `textures/environment/moon_phases.png`
- `textures/environment/clouds.png`

Production compilation accepts only the reviewed manifest's exact content and
fields and the reviewed encoded SHA-256 for each of those three PNG files.
The validator accepts a uniformly LF or CRLF checkout, canonicalizes CRLF to LF,
and hashes the canonical LF bytes; bare CR, mixed line endings, and all other
byte changes fail closed. `.gitattributes` requests LF for new checkouts, but
correctness does not depend on Git renormalizing an existing worktree. A future
vanilla bump must deliberately update the manifest identity and all affected
source-hash constants and ratchet tests.

The cloud source is included because the pinned pack supplies an authoritative
256x256 texture and it fits the same bounded carrier. This task does not define
cloud rendering.

## Architecture

Add an independent `MCBEATM1` binary rather than changing the shared
`MCBEAS05` world-asset schema. `crates/assets` compiles the three fixed sources
in canonical role order, decodes them to RGBA8, records source paths,
dimensions, source-file SHA-256 values, decoded-pixel SHA-256 values, and the
tracked source-manifest SHA-256, then seals the complete blob with SHA-256.
`RuntimeAtmosphereAssets` validates the entire envelope before exposing
immutable texture records.

The `assetc atmosphere` command writes the ignored binary and a deterministic
JSON report. The report records the manifest provenance fields, manifest hash,
per-texture metadata, and final blob hash; it does not record a machine-local
canonical pack path.

`make assets`, `make atmosphere-assets`, and `make client` carry both generated
outputs as freshness prerequisites. A single portable producer command avoids
ordinary multi-target races: the report depends on the blob, and either a
missing blob or a missing/stale report reruns the same deterministic compiler.

The command constructs both byte payloads and preflights both destinations
before publishing either. Each file uses a same-directory temporary file and
atomic rename. The two separate destinations are not crash-atomic as a pair;
if an unexpected second-file I/O failure occurs, the Make dependency observes
the missing/older report and reruns the complete pair.

Preflight compares normalized absolute locations, conservatively case-folded
locations on every platform, canonicalized existing ancestors, and existing file identity. It
rejects exact, dot/parent, case, symlink/junction, and hardlink aliases before
either output is opened for writing.

## Validation and bounds

Compilation fails closed if the manifest is not the exact reviewed pin or if
any required file is absent, differs from its pinned encoded SHA-256, is not
PNG, exceeds 1 MiB encoded, fails decoding, has unexpected dimensions, or
produces a non-RGBA8 byte count. Exact dimensions are 32x32 for the sun,
128x64 for the moon phase sheet, and 256x256 for clouds. Paths and ordering are
constants, not pack-controlled discovery.

Runtime decoding rejects wrong magic/version/count, noncanonical offsets,
unsupported roles/formats/reserved bits, unsafe or unexpected source paths,
dimension or payload-length mismatches, per-record hash mismatches, trailing
data, a missing source hash, pixel-hash mismatches, and an invalid envelope
hash. Source-file hashes are metadata protected by the envelope because source
PNG bytes are intentionally not copied into the runtime blob. The decoder
applies allocation bounds before copying payloads.

## Testing

Tests are written and observed failing before implementation. Explicit
synthetic compiled fixtures cover deterministic encoding, runtime round trip,
and corrupt-envelope rejection without entering the production source
acceptance path. Environment-gated pinned-pack tests lock exact manifest bytes,
all three encoded sources, dimensions, and SHA-256 values, including rejection
of each valid-but-modified PNG. CLI tests cover output alias rejection without
tracking Mojang bytes.

## Out of scope

No renderer, shader, Bevy plugin, app loader, GPU upload, celestial UV, moon
phase selection, or cloud motion change is part of this task. Renderer work
will later load `MCBEATM1`, create the GPU textures once, and bind/sample them.
