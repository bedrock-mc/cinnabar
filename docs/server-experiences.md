# Cinnabar server experiences: client preview and wire contract

This is an optional **Cinnabar extension**, not vanilla Bedrock parity. This branch
contains locally compiled client code and regression tests whose validation is
recorded in `plan.md`. It does not ship a production sandbox or an end-to-end
cinema implementation. The Dragonfly server half lives in
`tools/localserver/extension` ([experience-runtime.md](experience-runtime.md#client-parts));
no server SDK, BDS script, or server sidecar is included.

## Implementation status

| Phase | Implemented in this branch | Remaining gate |
| --- | --- | --- |
| Discovery and delivery | Admitted-pack marker, Ed25519 offer and challenge verification, destination/key/scope pins, HTTPS fetch policy, digest cache, indexed ZIP verification | Real vanilla marker compatibility; optional-pack provenance in the Go handoff; CDN and hostile archive fixtures |
| Consent | Private JSON-UI catalog, join-time popup that holds the loading screen, once/always/never/not now, changed-key disclosure, persistent settings, running indicator and F9 revocation | Visual/layout/accessibility review; settings editor; controller/touch consent and disable controls |
| Runtime | Versioned WIT, fuel and memory limits, transactional capability checks, per-bundle developer processes, bounded IPC and watchdog, typed channels and JSON-UI label preview | Restricted OS launch and compiler containment; full screen, focus/input, scene, particle and material adapters; snapshot recovery |
| Media | Signed descriptors, authenticated HTTPS ranges, optional off-thread WebM decode, clock/timeline primitives, frame queue, retained GPU texture, bounded PCM mixer source | Live message routing, surface ownership/binding, production decoder process, device clock, resampling, efficient seeking/looping and underrun recovery |
| Fallback | No marker means no probe/download/helper/prompt; pre-consent messages dropped before the world FIFO; ordinary session survives extension failure | Compare real packet captures; verify no-advertisement equivalence |

Production execution deliberately fails closed. `Helper::spawn_restricted` returns
an error. The app negotiates only after consent, but never downloads or starts a
bundle without `CINNABAR_DEV_SERVER_EXPERIENCES=1`. Without that switch, its hello
advertises an empty capability set and it never sends `ready`.

With the switch, the developer app advertises only `ui` and `messaging`. The UI
adapter currently renders bounded labels in a separate JSON-UI area. It denies
modal screens, input, scenes and media imports. A data-only bundle is accepted
without a component, but its media is not automatically played. Enabling the
optional `server-experience/developer-media` feature compiles decoder primitives,
but workers remain unavailable until a helper enforces a process memory ceiling.
The developer switch does not bypass this requirement.

## Ordinary Bedrock remains the transport

There are no new packet IDs, login fields, custom framing or unsolicited probes.
The resource pack handshake remains unchanged. A cooperating server places one
inert JSON file at `cinnabar/extension-offer.json` in an **optional** resource pack.
It must keep ordinary forms, controls and gameplay available to vanilla players.
It must not disconnect or gate ordinary play when this marker is ignored.

The client reads the marker only from the already admitted resource-pack view.
That handoff currently loses the pack's mandatory/optional provenance: the server
requirement above is not independently enforced by this client. Layer precedence
selects one marker; it does not combine offers. A missing, oversized, malformed or
unsupported marker cannot start an extension. A separate host-selected destination
is required; an advertised address cannot redirect the trust decision.

The only extension carrier is the generated Bedrock `ScriptMessage` packet, with
`message_id = "cinnabar:extensions/v1"`. `message_value` is UTF-8 JSON bytes. This
preview supports actual packet-header subclient route zero only. Both header
subclient fields and the envelope route must agree; other routes are ignored.

Before consent, matching inbound messages are discarded before world sequencing.
The network gate also discards queued extension output after revocation. Other
packets pass through their existing encode and send path. There is no capability
probe in a login, movement packet or ordinary form response. A socket write already
in progress when permission is revoked cannot be recalled.

## Signed documents and canonical encoding

All keys, hashes, signatures and nonces use lowercase hexadecimal, without a
prefix. Public keys and SHA-256 digests are 32 bytes; Ed25519 signatures are 64
bytes. Signed documents have exactly these fields:

```json
{"payload":"<hex of canonical UTF-8 JSON>","signature":"<hex Ed25519 signature>"}
```

Sign the concatenation of the domain bytes and the decoded payload bytes:

| Document | Domain, with a final NUL byte |
| --- | --- |
| Deployment offer | `Cinnabar/experience/offer/v1\0` |
| Live acceptance | `Cinnabar/experience/accept/v1\0` |
| Bundle manifest | `Cinnabar/experience/manifest/v1\0` |

The digest of an offer is SHA-256 of its canonical payload alone. A bundle digest
is SHA-256 of the entire delivered ZIP byte string. File and range hashes cover
raw file bytes, not JSON, compression output headers or their hexadecimal text.

Canonical JSON here means **the exact compact `serde_json` serialization of the
versioned Rust structs**, not RFC 8785/JCS. Object fields appear in declaration
order as listed below; no whitespace, extra fields, duplicate fields or alternate
number/string escaping is accepted in signed payloads. Integers use decimal JSON
integers. Optional values are explicitly `null`. Preserve ordinary array order.
String sets sort by string order. Permission sets use this declaration order:
`ui`, `modal_ui`, `input`, `messaging`, `scene`, `media`. UTF-8 is preserved; escape
quotes, backslashes and control characters as `serde_json` does. Verify roundtrip
bytes against the Rust contract before publishing a signer in another language.
Unsigned outer wrappers do not require canonical object order, but reject unknown
fields. `cinnabar-cxb write-fixtures <dir>` writes golden offer, marker, hello,
accept, ready, envelope, channel and manifest documents with their signatures from
fixed seeds into `tools/localserver/extension/testdata`; a Rust test keeps them
current and verifies them with this crate. The Go package `tools/localserver/extension`
decodes, re-encodes and re-signs every one of them byte for byte (`go test ./extension`).

## Deployment advertisement

The marker is `{"server_key":"<key>","offer":<signed document>}`. The signed
`Offer` payload has these fields in order:

| Field | Meaning |
| --- | --- |
| `version` | Extension wire version, currently 1 |
| `audience` | Exact host-selected canonical `host:port`; bracket IPv6; explicit nonzero port |
| `server_key` | Same Ed25519 key as the marker |
| `revision` | Monotonically increasing deployment revision, unsigned 64-bit integer |
| `expires_unix` | Expiration in Unix seconds; at most the host's offer lifetime into the future |
| `scope` | Requested permissions, exact HTTPS origins, aggregate memory and GPU ceilings |
| `packages` | Ordered array of immutable package offers |
| `fallback` | Plain human-readable explanation of ordinary-client behavior |
| `carrier` | Exactly the carrier name above |

`Scope` fields, in order: `permissions`, `origins`, `memory_bytes`, `gpu_bytes`.
Origins are canonical HTTPS origins, without paths, credentials or query strings.
A non-default port is part of the approved origin. Memory/GPU values may reduce
host ceilings, never raise them. New origins, permissions, publishers or budget
ceilings require fresh approval.

Each package has `id`, `publisher_key`, `digest`, `bytes`, `url`, in that order.
The URL must belong to an approved origin. `bytes` is the exact compressed ZIP
length. Package IDs are unique. Identifier syntax is nonempty lowercase ASCII
letters, digits, `:`, `_`, `-`, `.`; the shared identifier length limit applies.
The publisher key signs the manifest; the server key signs the deployment that
selects that publisher and bundle digest. Neither signature certifies code safety
or establishes a publicly verified operator identity.

The current code has no certificate hierarchy, key-rotation cross-signature,
revocation service, dependency resolver, imported descriptor discovery fallback,
or hot update. Key changes require user approval. Updates take effect on rejoin.

## Consent, handshake and readiness

1. Admit the optional pack and verify its marker locally. Do not contact its URLs.
2. Show the host-owned JSON-UI consent popup as soon as the offer is verified. The
   marker arrives with StartGame, so the popup opens over the join's loading screen,
   like vanilla's join-time resource pack prompt; the loading screen and the
   readiness boundary (loading end and `SetLocalPlayerAsInitialized`) wait for the
   answer or the offer's expiry. The hello may therefore precede initialization; a
   gophertunnel server defers it until then. Remembered `Always`/`Never` decisions
   skip the popup. Disclose the destination, key, publisher keys, bundle sizes,
   requested permission/budget scope, origins and fallback. Warn that external
   origins see the user's IP.
3. `Allow once` grants this connection only. `Always` stores exact destination,
   key, scope and publisher set. `Never` suppresses the destination, including
   offers with a different key. Rejoining with a broader scope prompts again.
4. After approval, send `{"kind":"hello","body":<Hello>}`. `Hello` fields are
   `version`, `api`, `capabilities`, `offer_digest`, `client_challenge`,
   `connection`, `subclient`. API version is currently 1. Challenge and connection
   are independently random 32-byte values. Capabilities are coarse; no account,
   machine ID, filesystem path or hardware inventory is sent.
5. Reply on that same live Bedrock connection with
   `{"kind":"accept","body":<signed document>}`. The signed `Accept` fields
   are `hello`, `server_challenge`, `session`, `audience`, `offer_digest`,
   `revision`, `expires_unix`. Echo the **whole** hello, unchanged. Server challenge
   and session are fresh 32-byte values. Expiration cannot exceed the offer's.
6. The client accepts once, within the handshake timeout. A wrong key, challenge,
   destination, scope digest, revision, connection or expiration revokes this
   extension. It does not disconnect ordinary play.
7. Only after verified download and successful helper initialization does the
   developer client send `{"kind":"ready","body":...}`. Body fields are
   `session`, `packages` (ordered bundle digests), `generation` (1), `permissions`
   (package-ID map to actual granted permission arrays), `world_epoch`.
8. Wait for `ready`. Do not send bundle events during downloading or initialization.
   If readiness does not arrive, use the advertised fallback. Capability denial,
   timeout, missing helpers and every production build can take this path.

The `disabled` control tag is reserved. The client does not need to send it to
revoke authority. No fallback relies on receiving a final message from a crashed
or disconnected client. There is no auto-restart after a trap or protocol failure.

Trust lives in `server-experiences.json` alongside the existing menu settings.
Fields are `disabled`, `media_muted`, `media_autoplay`, `pins`. A pin contains
`audience`, `server_key`, `scope_digest`, `decision`, `highest_revision`. The scope
digest covers the scope plus the ordered package-ID/publisher-key pairs, allowing
content updates under the same trust decision. Remembered joins persist a new
revision floor before sending their hello. Save failures revoke the pending grant
and reload disk state. A malformed settings file disables extensions. Missing
settings grant nothing. The media mute/autoplay preferences are stored but await
live playback integration and a settings UI.

The popup's buttons are `Allow once` (F6), `Always allow on this server` (F7),
`Never on this server` (F8) and `Not now` (Escape; nothing is stored). It owns the
pointer and all input while shown, so gameplay and the screens behind it cannot
react. The approval buttons and keys stay disabled until the trusted popup has
rendered and its scrolled disclosure has reached the end. F9 immediately revokes
an offered/running experience and kills its helpers. It leaves ordinary keyboard
input alone when no experience is offered. Trusted status and consent come from a
private JSON-UI catalog which resource packs cannot replace; the popup is drawn
with host solid fills only, so pack textures cannot restyle it either. Guest
labels are rendered separately below the running indicator.

## Bundle container and manifest

A `.cxb` is a ZIP containing regular files only. It is never extracted into a
filesystem tree. Supported compression methods are stored and deflate. Reject
links, special files, encryption, duplicate names, directories, absolute paths,
empty components, `.`/`..`, backslashes, excessive depth and expansion. Paths use
lowercase ASCII letters, digits, `/`, `.`, `_`, `-`. Case aliases are not permitted.
Only the final validated ordinary directory is used. ZIP64, streaming data
descriptors and extra metadata are unsupported. Local headers must match that
directory. Entry decompression uses the streaming reader, so the ZIP library
cannot retry an earlier directory or allocate its index.

`manifest.signed.json` contains a signed manifest wrapper. The manifest payload
fields, in canonical order, are:

- `version`, `api`, `id`, `publisher_key`, `package_version`;
- `permissions`, `component` (indexed portable Wasm path, or `null`);
- `channels`, `actions`, `files`.

`files` lists every other ZIP entry exactly once. Each entry has `path`, `bytes`,
`sha256`. The manifest does not index itself. Check the outer digest, publisher
signature, API versions, identity, permission subset, paths and every indexed
hash before compilation. Serialized native Wasmtime artifacts are never accepted.
`package_version` is publisher metadata; rollback protection uses the server's
signed deployment revision. Dependencies must be bundled into the component;
there is no runtime package dependency loader.

`actions` is a set of declared action IDs; it grants no keyboard access by itself.
`channels` declares positional schemas (see below). JSON-UI templates, scene assets,
posters and media descriptors may be indexed, but their presence alone does not
make an unimplemented presentation adapter available.

`cinnabar-cxb` (`tools/cxb`) is the publisher tool. `keygen <file>` writes a new raw
32-byte Ed25519 seed as one line of lowercase hex and never replaces a file.
`build --manifest <toml|json> --component <wasm> --publisher-seed <file> --out
<x.cxb>` reads `id`, `package_version`, `permissions`, `channels` and `actions` from
the manifest source, componentizes a core module as `mod-host pack` does, stores
it as `component.wasm`, signs the manifest and checks the archive with this
crate's verifier before writing it. It prints the bundle's `sha256` and `bytes`.
Entries are stored with a fixed timestamp, so equal inputs give an equal digest.

The cache is under the install layout's per-user
`server-experiences/v1/objects/<sha256>.cxb`. It has an exclusive process lease,
private directory/file modes on Unix, atomic publication, rehashed reads and LRU
quota eviction. Interrupted partial files are removed after acquiring the lease.
Runtime grants are never cached. Local grants are separate from immutable bytes.
There is no cross-user or global cache and no persistent media cache in this phase.
The private cache assumes a trusted local user; it is not a hardened defense
against a same-user process racing filesystem operations.
`cinnabar-cxb seed-cache --cxb <x.cxb> --user-data <dir>` publishes a bundle into
the cache under user data root `<dir>`, as a finished download would.

HTTPS requests use exact approved origins, TLS validation, no redirects, no proxy,
no cookies, no credentials and no account headers. Resolve and reject private or
special-use addresses before connecting, pin all accepted DNS answers into the
request client, and verify the connected peer. Both mixed public/private answers
and DNS rebinding fail closed. Every response is length-bounded independently of
Content-Length. Full bundles require status 200; ranges require exact status 206
and Content-Range. Content encoding other than identity is rejected.

## Typed runtime messaging and publication

After `ready`, each direction has its own reliable sequence starting at 1, shared
by all bundles. Control messages are outside this sequence. Runtime JSON fields:

```text
version, session, connection, subclient, bundle, generation,
channel, schema, sequence, world_epoch, payload
```

All route values must match the live grant and ready record. Generation is 1 for
this preview; there is no in-place reload. The server must use the ready epoch.
A dimension/epoch change currently revokes the preview until rejoin instead of
attempting partial resynchronization.

Each manifest channel contains `id`, `schema` (u16), `direction` (`to_client` or
`to_server`), and `fields`. IDs must start with the owning package ID plus `.`.
A channel/schema pair is unique. A payload is an ordered array of typed scalars:

| Field declaration | Payload value |
| --- | --- |
| `{"type":"bool"}` | `{"type":"bool","value":true}` |
| `{"type":"integer","min":0,"max":100}` | `{"type":"integer","value":42}` |
| `{"type":"text","max_bytes":64}` | `{"type":"text","value":"hello"}` |
| `{"type":"choice","variants":3}` | `{"type":"choice","value":0}` |

Text length is UTF-8 bytes. Choice values are zero-based. No opaque arbitrary
Bedrock packet payload is exposed. The host validates outgoing records against
`to_server` schemas and incoming records against `to_client` schemas.

Charge bytes/messages before JSON parsing, then check identity, sequence, schema
and queue bounds. Unknown schema revisions are counted and skipped while consuming
their sequence. Replays, gaps, invalid known records and queue overflow quarantine
the optional channel. There is no unreliable lane, retransmission or snapshot
request in this preview; the server must fall back rather than expect recovery.

Inbound extension events share the existing ordered world publication stream.
Only committed UI events enter the experience controller. Stale dimension work
is discarded. Guest callbacks run later on committed state and publish whole
validated transactions; no callback reads or mutates unpublished world state.
The preview supervises one pending callback per bundle. Accepted events remain
in the bounded reliable queue until the helper and aggregate callback budget are
available. Each slice also schedules pending initializers; readiness remains
pending until all bundles finish initialization.

## Host capabilities and containment

The component world is `server-bundle` in
[`extension.wit`](../crates/mod-api/wit/extension.wit). Its imported interfaces use
`cinnabar:server-experience@1.0.0`, defined in
[`capabilities.wit`](../crates/mod-api/wit/deps/server-experience/capabilities.wit).
Guests export `init()` and `dispatch(channel, record-json)`.

| Permission | Host contract | App adapter today |
| --- | --- | --- |
| `ui` | Set/remove an owned label by ID | Bounded label preview through JSON-UI |
| `modal_ui` | Open an owned signed JSON template | Denied |
| `input` | Query a declared, host-delivered action edge | Always false; no focus adapter |
| `messaging` | Send a signed typed channel record | Connected to the existing packet send FIFO |
| `scene` | Put/remove a declarative quad, mesh or particle object | Validated host contract; renderer adapter denied |
| `media` | Control an indexed media descriptor | Validated host contract; app adapter denied |

Scene transforms are position xyz, normalized quaternion xyzw, positive scale xyz.
A quad declares an owned texture and size; a mesh an owned asset and triangle
count; particles an owned effect and count. These declarations do not confer raw
GPU handles or shaders. Actual mesh cost must be derived from validated assets
before enabling scene permission; guest-supplied counts are insufficient.

Owners are `(session, bundle, generation)`; retained transactions additionally
carry a world epoch. Wrong owners/epochs and invalid operations never partially
publish. Guest fuel, one linear-memory allowance, stack, instance/table counts,
output size and host-call counts are bounded. Aggregate session reservations and
callback fuel prevent multiplying allowances by adding bundles. Startup callbacks
also consume the aggregate budget and wait across slices when it is exhausted.
Readiness waits for every initializer; their bounded sends follow the ready record.
These are not OS resident-memory or compiler limits.

The developer helper uses a fresh process per component, cleared environment,
private stdio, bounded length-prefixed JSON IPC, asynchronous IPC workers and a
watchdog. Dropping a helper kills it and reaps it off the render thread. No WASI,
account tokens, filesystem, arbitrary network, raw packets, ECS mutation or
unrestricted shaders are imported. An explicitly guarded in-process developer
constructor also exists; the app uses the process constructor.

**Neither developer path is a production sandbox.** OS resource limits, descriptor
inheritance auditing, filesystem/network denial and compiler containment are not
implemented. Wasmtime and native decoders remain attack surfaces. A separate
process and a signature are not a security claim. Production stays unavailable
until restricted launch, fault injection and independent review pass on each OS.

## Built-in media contracts (not yet a live advertised channel)

The Rust media service is independent of Wasm: callers can prepare a signed
bundle's descriptor without starting a component. Its controls are serializable
contracts for the future built-in adapter. This app does **not** advertise media,
route these controls from `ScriptMessage`, or attach its outputs to a surface.
Do not send them to the working typed runtime channel and expect playback.
A future adapter must negotiate a media channel revision and validate the same
session, connection, sequence and epoch envelope before invoking these APIs.

A descriptor is an indexed, hashed JSON file signed indirectly by the manifest.
Fields are `id`, `timeline`, `profile`, `url`, `bytes`, `chunk_bytes`, `chunk_hashes`,
`sha256`, `width`, `height`, `fps`, `duration_us`, `audio_channels`, `poster`.
The profile is `webm_av1_opus_bt709`. `poster` names an indexed fallback image.
The whole-object SHA-256 is declared metadata; streaming integrity is enforced by
the signed chunk index. Each chunk hash covers the corresponding contiguous
`chunk_bytes` block, with only the final block shortened. A range is fully hashed
before the demuxer sees any of it. Whole-object digest reconciliation is not yet
performed after streaming.

The decoder profile is progressive WebM, exactly one AV1 video track and one Opus
audio track, 8-bit I420, limited-range BT.709 primaries/matrix/transfer, at most
1280×720 and 30 fps, and mono/stereo at 48 kHz. No chapters, tags, track encodings,
lacing, alpha, HDR or arbitrary profile changes. Opus uses mapping family zero,
version-one OpusHead, matched pre-skip/CodecDelay and packets up to 20 ms. Preserve
explicit BT.709 metadata in the encoded stream. Video is converted to sRGB RGBA
for the retained GPU texture. Opus header gain and pre-skip are applied.

A media `Message` contains, in order:

```text
owner: { session, bundle, generation }
instance, generation, timeline, world_epoch, revision,
effective_server_us, operation
```

Instance and generation are both 1 in this initial controller. `revision` must
increase. Effective times must be ordered, at most 30 seconds into the future,
and are measured in the server's monotonic clock microseconds, not Unix time.
Operations use an internally tagged `kind`:

| `kind` | Additional fields |
| --- | --- |
| `prepare` | `media_id` |
| `play`, `pause`, `seek` | `position_us` |
| `set_loop` | `bounds_us`: `[start,end]` or `null` |
| `set_volume` | `per_mille`: integer from 0 to 1000 |
| `attach` | `surface` |
| `detach`, `stop` | None |

Surface declarations are `{"kind":"ui","widget":"owned-id"}`,
`{"kind":"quad","object":1,"generation":1}`, or
`{"kind":"entity","runtime_id":1,"generation":1,"material_slot":"screen"}`.
These are intent only. A future surface manager must verify widget/object/material
ownership, current entity generation, user visibility and allowed dimensions.
IDs are never direct renderer or ECS handles.

Clock APIs issue an outstanding `(id,c0)` probe and accept only its matching
`(id,c0,s1,s2,c3)` reply: client send, server receive, server send, client receive.
The estimator selects the lowest-delay recent sample, exposes half-RTT uncertainty,
and invalidates stale/suspended clocks. No run-ID or end-to-end clock wire adapter
is present yet. Future routing must bind replies to the live negotiated server
clock generation; a raw timestamp is not sufficient authorization.

Future controls are applied only when due. Desired playback position comes from
the authoritative timeline and loop bounds. Drift decisions hold within 20 ms,
request rate correction up to ±0.5%, and seek beyond 250 ms. These decisions are
implemented, but **the output adapter does not yet apply resampling or automatic
seek correction**. Current restart/seek decodes from the beginning and discards
older output. Seamless loops and sparse keyframe seeking remain incomplete.

The worker uses bounded compressed-range, video-frame and PCM queues, one decoder
lease process-wide, a shared download-byte allowance across seeks, and generation
checks on output. Decoding is off the render and audio threads. GPU upload updates
one retained texture rather than allocating per frame; the texture has a reusable
view but no JSON-UI/world/entity binding yet. The mixer source consumes timestamped
stereo PCM from a fixed ring, supplies silence on underrun and obeys master/records
volume, user mute, pause, spatial attenuation and stereo-to-mono spatial playback.
It reports submitted PTS only: the actual audible Rodio/device timestamp is a stub.
No production synchronization claim is made from a submission timestamp.

## Decoder dependencies and rationale

- [`matroska-demuxer 0.8.1`](https://docs.rs/matroska-demuxer/0.8.1/matroska_demuxer/):
  Rust demuxer exposing seekable `Read + Seek`, tracks, sample bytes and timestamp
  scale. Its declared-size allocations are not bounded by authenticated ranges;
  it must only run in a memory-limited helper. Reader faults are retained separately
  so a failed download or hash check cannot become successful end-of-stream.
- [`dav1d 0.11.1`](https://docs.rs/dav1d/0.11.1/dav1d/): Rust wrapper around the
  native AV1 decoder, with strict compliance, thread count and frame-size controls.
  Decode is native code, not memory-safe Rust; platform dav1d provisioning and a
  restricted decoder process are release requirements.
- [`opus 0.4.0`](https://docs.rs/opus/0.4.0/opus/): Rust wrapper around libopus with
  float PCM and explicit sample-rate/channel control. It also requires containment
  and audited native-library provisioning before production use.

The choice favors established decoder implementations over writing codec parsers
or relying on an unverified pure-Rust AV1+Opus stack. The native dependencies are
optional. Worker availability and startup both fail closed, including with the
developer switch, until a memory-limited helper is implemented. Matroska internal
allocations occur before some sample checks, so post-decode validation is not a
substitute for process memory limits or adversarial fuzzing.
MP4/H.264/AAC remains the explicitly unavailable `PlatformDecoder` trait stub.

Other pinned direct dependencies reuse locked versions: reqwest 0.12.28,
ring 0.17.14, sha2 0.10.9, url 2.5.8, zip 7.2.0, serde 1.0.228,
serde_json 1.0.150, tempfile 3.27.0, and crossbeam-queue 0.3.13. Wasmtime remains
36.0.16; the existing WIT binding generators are reused. The branch already adds
the optional native decoders and their resolved transitive dependencies to
`Cargo.lock`. The local validation recorded in `plan.md` used that lockfile
without a further dependency update.

## Limits, fallback and verification still required

[`policy.rs`](../crates/server-experience/src/policy.rs) is the source of truth for
host budgets. [`media.rs`](../crates/server-experience/src/media.rs) owns media
ceilings. The current envelope is bounded by the protocol carrier constant; rates
are aggregate across all bundles, with at most a one-second burst. Queue bounds
apply independently to network publication, typed ingress, helper IPC and media.
No increase requested by a server bypasses those ceilings.

Leave, transfer, expiry, disable, broken trusted chrome and fatal game-session
errors discard session capabilities and developer helpers. Old generations never
receive new permissions. Signed cache objects may survive, but execution state,
workers, pending output and grants do not. Media adapters must eventually enforce
that same lifecycle for textures, mixer voices and entity attachments.

Every server must retain its fallback before readiness and after any loss of
extension state: ordinary forms for choices, pack art/posters/captions for video,
and ordinary controls or an explicit optional mode for richer mechanics.

Local validation is recorded in `plan.md`; it does not close production gates.
Before integration, compile default and developer-media configurations, format,
run focused tests, clippy and the architecture gate. Add hostile HTTP fixtures,
component trap/timeout tests, helper-exit races, consent-layout tests, independent
wire fixtures, verified media samples, reconnect/transfer tests and packet captures
proving no-advertisement equivalence. Production additionally requires OS sandbox
verification, hostile demux/codec fuzzing, compiler limits, resource measurements,
clock/device latency tests and visual checks for every supported surface.
