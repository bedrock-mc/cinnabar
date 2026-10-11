# Pinned upstream sources

The protocol crate resolves Valentine and Jolyne from the checked-in
`vendor/valentine` and `vendor/jolyne` paths. The retained source is MIT
licensed; see `crates/protocol/vendor/LICENSE`.

Machine-checked provenance:

- Dependency resolution: local vendored paths
- Axolotl Stack merge revision: `c4540512dc47833bb40363da7ad1161110d64b67`
- Protocolgen submodule, manifest, and generated-source revision: `0b8f17e3b321f7cb89e21dc8563398b9981e632f`
- Retained license normalized SHA-256: `62c75fcb256604584191434b605dc3fe661d938a94b2c35836ef55011bf24184`

The copied surface contains the shared codec/runtime, the generated protocol
2193 crate, and the Jolyne client/server transport facade. Upstream examples,
benches, generator executables, unrelated workspace crates, and uninitialised
generator-input submodules are omitted. Local manifests replace workspace
inheritance with direct versions and local paths.

The generated crate is regenerated locally: `valentine_gen` at the pinned
Axolotl revision lowers protocolgen's `generated/1.26.51/manifest.json`
(`CARGO_MANIFEST_DIR=crates/valentine_gen valentine-gen --protocolgen-manifest
<protocolgen>/generated/1.26.51/manifest.json`, then `cargo fmt`). The
generator names the crate after the manifest (`valentine_bedrock_1_26_51`);
protocol 2193 is spoken by Minecraft 1.26.50 through 1.26.52, and Jolyne
reports game version 1.26.50 to match gophertunnel. Protocolgen emits
lowercase-derived enum member names for this manifest (for example
`Loginsuccess`); Cinnabar uses them unchanged. Cinnabar preserves its local
shared-codec and Jolyne transport hardening.

## Local source patches

Jolyne's login retains client-authored geometry, resource patches and minimum engine
versions. Uploads without explicit geometry select the classic or slim resource patch.

`DisconnectPacket` is hand-patched after generation to read
`hide_disconnection_screen` and skip both message strings when it is set, as
gophertunnel's `Disconnect.Marshal` does; the manifest still lacks that
conditional. The normalization input fingerprints include this patch.

Jolyne's client ends a join-time Disconnect with `ProtocolError::ServerDisconnect`,
keeping the server's reason and message texts for the disconnect screen.

Jolyne hands off required resource packs instead of refusing them: either
required bit makes stack selection strict and is carried as
`ResourcePackHandoff::required`, so the client refuses a join it cannot fully apply.

Jolyne's client can resume at StartGame (`BedrockStream::from_session_handoff`) for a session
whose login and packs the Go core completed, so Cinnabar keeps Jolyne's spawn sequence; it
exports `raw::MAX_RAW_BATCH_PACKETS` so handed-off startup packets are batched within it.

Jolyne's client requests only offered packs its `ResourcePackStore` cannot supply, answering
HaveAllPacks when nothing is missing, as the vanilla client does for its pack cache.

The self-signed login's client data reports `DeviceOS` 8 (Win32, the GDK Windows client) with a
lowercase-hex `DeviceId` instead of upstream's Win10 and UUID; BDS 1.26.52 closes logins claiming Win10.

Generated protocol reservations are normalized locally after generation by
`tools/protocol-normalize/normalize.py` and its pinned neutral-only manifest.
Numeric packet selectors, enum values, union discriminators and field ordinals
select reserved API bindings, including their supporting owned/borrowed records
and debug labels. Shared retail records remain unchanged. This is a local
generated-source naming patch, not pristine upstream output: codecs, numeric
wire values, field order, sizing, limits, allocation and error paths are retained.
The tool validates complete input/output fingerprints and a reversible scoped
binding transformation plus an exact reversible, fingerprinted local layout
patch before emitting an `apply_patch` patch. The layout spans reproduce the
reviewed Rust formatting without invoking a formatter at tool runtime; they
do not generalize syntax transformations. Uniform LF and CRLF input retain
their line endings, while mixed line endings are refused. The tool never edits
upstream repositories or generator inputs. Run the tool without `--patch` to
check canonical output, and run its stdlib Python unit tests separately. The
Rust protocol suite independently checks the complete normalized source hashes
without requiring Python. Existing conformance fixture bytes remain unchanged.

The retained Jolyne changes preserve negotiated compression, bounded batch
ingress, deferred packets, strict login sequencing, compact raw-frame error
context, exact packet-entry boundary checks, and a stack-encoded packet-ID
varint in raw header decoding (resolution still goes through the generated
codec, whose normalized source stays hash-locked). The shared codec includes a
fixed-width little-endian NBT scanner with bounded nesting and Bedrock UUID
encoding as two little-endian `u64` halves.

The shared codec inlines its per-item capacity check and keeps rare capacity
growth out of line. Collection storage still grows fallibly, with the same
allocation limits and errors; generated codecs are unchanged.

Jolyne's StartGame handoff also retains the first decoded `ItemRegistry` and its
shield ID. This matches the one-time initialization guard in the native
1.26.50 `ItemRegistry::matchServerItemIds`; later empty or
custom-only packets must not replace the startup table. The Cinnabar play
ingress wire-decodes these repeats but does not publish replacement events.

Jolyne's StartGame handoff also records whether a publisher update, level chunk or
sub-chunk preceded PlayerSpawn (`terrain_before_spawn`): servers such as Dragonfly
stream terrain only after the client's initialized notification.

The generated protocol crate is lowered from protocolgen's reconciled 1.26.51
manifest (protocol 2193), which pins Mojang's `v1.26.51` metadata release and
Endstone's 1.26.51.1 dump and is checked against the gophertunnel oracle.
Reconciliation requires two byte-equivalent complete source
claims or a fingerprinted adjudication with independent wire evidence. Reviewed
corrections cover binary buffers, little-endian scalar union arms, strict
actor-ID varints, optional inventory filtered names, adjacent structure names, the two-selector PlayerList
entry layout, and opaque preservation of unavailable packet bodies. Pinned conformance fixtures under
`crates/protocol/tests` cover these shapes.

The runtime and fixture generator resolve the project pin
`hashimthearab/gophertunnel` commit
`b725d82563e93308fd1f92d27da5e97301ad5040` (`resource-pack-changes`, module
pseudo-version `v1.25.3-0.20260929084839-b725d82563e9`, protocol 2193 as
`minecraft.DefaultProtocol`). The fork includes lunar's 1.26.50 support and the
restored Cinnabar resource-pack APIs. Fixtures are regenerated by
`tools/fixturegen` against that pin.

## Generated-code caveats

The generated 1.26.51 decoders validate signed and platform-sized lengths and
grow decoded collections through fallible allocation without trusting untrusted
wire counts for eager capacity. Byte buffers validate their declared size against
the remaining packet before allocating. These checks do not impose a global
collection ceiling; valid larger values remain accepted where the field contract
permits them.

Content registries are maintained independently of the generated wire schema.
The protocol crate uses reviewed retail item and biome allowlists under
`crates/protocol/data/`.
