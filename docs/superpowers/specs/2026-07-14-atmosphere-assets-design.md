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

## Validation and bounds

Compilation fails closed if any required file is absent, is not PNG, exceeds
1 MiB encoded, fails decoding, has unexpected dimensions, or produces a
non-RGBA8 byte count. Exact dimensions are 32x32 for the sun, 128x64 for the
moon phase sheet, and 256x256 for clouds. Paths and ordering are constants, not
pack-controlled discovery.

Runtime decoding rejects wrong magic/version/count, noncanonical offsets,
unsupported roles/formats/reserved bits, unsafe or unexpected source paths,
dimension or payload-length mismatches, per-record hash mismatches, trailing
data, and an invalid envelope hash. It applies allocation bounds before copying
payloads.

## Testing

Tests are written and observed failing before implementation. Synthetic pack
tests cover exact compilation, absent/malformed/oversized/wrong-dimension
failures, deterministic encoding, runtime round trip, and corrupt-envelope
rejection. An environment-gated pinned-pack test locks the exact three source
paths, dimensions, and SHA-256 values without tracking Mojang bytes.

## Out of scope

No renderer, shader, Bevy plugin, app loader, GPU upload, celestial UV, moon
phase selection, or cloud motion change is part of this task. Renderer work
will later load `MCBEATM1`, create the GPU textures once, and bind/sample them.
