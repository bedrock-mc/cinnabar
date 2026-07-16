# Environment Profile Plumbing Design

## Goal

Route the pinned Bedrock client biome and fog metadata into Cinnabar's runtime
atmosphere without inventing replacement colors, curves, or packet behavior.
The tranche must distinguish Overworld/default, Nether/hell, and End profiles,
preserve the existing server-authored clock and weather state, and leave native
lighting calibration and celestial behavior explicitly open.

## Authoritative inputs

The ignored local Bedrock Samples pin remains the sole source of native client
metadata. Compilation reads modern `*.client_biome.json` documents and the fog
settings they reference. No Mojang JSON, textures, screenshots, or generated
asset blobs are committed.

Each compiled biome visual profile contains its canonical biome identifier,
fog identifier, atmosphere identifier, lighting identifier, and optional exact
sky RGB. Each compiled fog profile contains bounded named media records with
the exact start, end, RGB, and fixed-versus-render-relative distance mode from
the source document. Input is sorted and validated for duplicate identifiers,
finite nonnegative distances, bounded strings/counts, supported media, and
references to an existing compiled fog profile.

## Runtime data flow

The atmosphere envelope owns both its existing pinned textures and the new
environment profile tables. The envelope remains deterministic, hashed, and
strictly decoded before allocation. Runtime lookup is by canonical biome or fog
identifier.

`WorldStream` exposes a fail-closed camera biome sample, the committed current
dimension, and the effective render distance in blocks. The app keeps one
environment context resource containing those values and the selected visual
route. A known camera biome wins; otherwise the route falls back by dimension
to `minecraft:plains`, `minecraft:hell`, or `minecraft:the_end`. Unknown custom
dimensions or absent profiles use the existing procedural frame.

The app derives time/weather exactly as before, then applies only information
present in the selected compiled profile. An explicit sky color replaces both
procedural sky endpoints. Fog is selected for air, water, lava, or weather and
resolved by multiplying render-relative distances by the effective render
distance while leaving fixed distances unchanged. Partial rain interpolation
retains the existing server rain channel but is labeled provisional because
the pinned fog files define endpoints, not the transition curve.

The selected lighting identifier is carried as an explicit route in app state.
The current grayscale WGSL curve and its `0.2` and `0.04` floors remain
provisional and unchanged in this tranche.

## Testing and safety

Strict TDD applies to every production behavior. Synthetic fixtures establish
the wished-for API and fail before implementation. Tests cover exact
plains/hell/End metadata, malformed or unbounded source rejection,
encode/decode preservation, fixed/render-relative resolution, camera biome
sampling, dimension fallback, and preservation of clock/weather fields.

No actor, interpolation, scheduler, runtime graphics override packet, or
celestial shader behavior is changed.

