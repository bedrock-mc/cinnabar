# Read-only duel spectator ingress

This standalone Go module consumes Practice's existing local NATS connection and
serves transient duel geometry and live player frames. It accepts no browser
gameplay commands and does not connect a Minecraft player or use a database.

## HTTP interface

All spectator routes accept only `GET` and `HEAD`. Live duel responses return
`Cache-Control: no-store`; verified runtime assets use immutable caching. Browser requests must come from the configured public origin.

| Route | Response |
| --- | --- |
| `/api/spectator/duels` | `{ "duels": [Frame] }`, containing only fresh, complete matches |
| `/api/spectator/duels/{id}/arena` | `{id,name,palette,bounds,blocks}` for an active match |
| `/api/spectator/duels/{id}/events` | SSE `frame` events, followed by `closed` when unavailable |
| `/api/spectator/duels/{id}/skins/{sha256}` | PNG referenced by a fresh bot fighter; removed with duel consent |
| `/api/spectator/assets` | Runtime carrier manifest |
| `/api/spectator/assets/{sha256}/{filename}` | Verified immutable carrier, optionally gzip encoded |
| `/healthz` | Local process liveness |

The SSE connection sends the current frame immediately. Each subsequent message
contains the latest frame; slow browsers do not accumulate a replay backlog.
Matches disappear after five seconds without a fresh frame. Closed matches
cannot be reopened by delayed, still-fresh messages. Practice must stop exports
and publish `closed` immediately if any human revokes spectator permission.

The rest of the HTTP surface is reverse-proxied to the isolated dev website.
Non-GET/HEAD methods are blocked across the entire preview, including account,
friend and store mutations. Arena downloads have two shared concurrent slots.
Forwarded visitor IPs are accepted only from explicit proxy prefixes. Global and
per-IP stream limits are 64 and four; each IP receives 60 request admissions per
minute. The peer ledger is bounded at 4096 entries. Slow writes have three-second
deadlines, independently of the lifetime of SSE connections.

## Game exports

Practice publishes version 1 messages on:

- `practice.spectator.v1.arena`: `{version,id,name,palette,bounds,part,parts,blocks}`.
  Parts are zero-based; all parts repeat identical metadata. Palette index zero
  is `minecraft:air`. Blocks are `[x,y,z,paletteIndex]`; air is omitted. Bounds
  contain inclusive minima and maxima. Geometry is withheld until all
  parts are complete and validated.
- `practice.spectator.v1.frame`: `{version,id,arenaId,mode,ranked,roundActive,
  updatedAt,players,teamWins}`. Player records contain `id,name,bot,team,position,
  yaw,pitch,health,maxHealth,hits,dead`. Positions are feet coordinates; angles
  retain Minecraft degrees. `updatedAt` is RFC3339 UTC, and teams are zero-based.
- `practice.spectator.v1.replay-start`: `{version,id,arenaId,frames}`. This
  atomic, recording-only opening contains real preparation and pre-arena
  snapshots. It is limited to 200 frames, 512 KiB and ten seconds, with at
  most 500 ms between observed snapshots. Historical frames never enter the
  live cache. Deploy this ingress before the matching Practice exporter.
- `practice.spectator.v1.closed`: `{version,id,updatedAt,reason,finalFrame,
  replayIncomplete}`. Only a normal `finished` close can retain a recording;
  consent revocation, aborts and incomplete exports discard it.

Limits: 1 MiB per bus message, 4096 voxels per part, 256 parts and one million
voxels per complete arena, 16 cached arenas, two million cached voxels total,
32 simultaneous matches and 32 fighters per frame. Cache state is lost on
restart. Incomplete geometry expires after 30 seconds; arenas serving active
matches cannot be evicted. Only fresh exports from the trusted bus are accepted.

## Build and deploy

Run checks on the shared development host through `agent-check`, with one owner:

```sh
/home/danick/.local/bin/agent-check -- env GOWORK=off go test -p=1 ./...
/home/danick/.local/bin/agent-check -- env GOWORK=off go build -trimpath -o /tmp/zeno-spectator ./cmd/spectator
```

The build context for `Dockerfile` is this module directory. `deploy/compose.yaml`
caps the runtime to one CPU and 384 MiB RAM with no swap. Alternatively, install
the Linux binary at the systemd unit's release path and use
`deploy/zeno-spectator.service` with the same resource limits.

Copy the nonsecret defaults from `deploy/spectator.env.example`; put credentials
only in the deployment environment or a read-only NATS credentials mount. Point
the native ingress at `127.0.0.1:3003` with the public website on port 3000.
ForwardMe routes `zenomc.org` to that website; the former `dev.zenomc.org`
preview redirects there. Website session validation remains required for live
and replay routes.

## Replay storage

Set `REPLAY_DIRECTORY` to a persistent directory writable by the container's
nonroot user and supply `REPLAY_API_SECRET` through a private runtime env.
Practice uses the same secret for replay reads; browsers use validated website
sessions and never receive that bearer secret.

Recordings use independent Zstandard chunks for bounded seeking and share
content-addressed arena, skin and appearance assets. The 25,000,000,000-byte
storage limit counts completed and active files plus their assets. New writes
remove the oldest completed recordings and unused assets when space is needed.
Interrupted recordings are discarded on restart. Keep extra disk space for
filesystem overhead and temporary files.

The API exposes `/api/replays`, replay metadata and bounded frame windows,
plus frozen arena and appearance assets. A recording must start from a complete
opening and end normally. Lost exports and invalid timelines fail closed.

## Appearance and native POV

Frame player records optionally include `equipment` (mainHand, offHand, four
armour slots), native action timestamps, movement/use flags, skinModel and `pov`.
POV contains nine hotbar slots, selectedSlot, eyeHeight, food, absorption,
armourPoints, XP, effects, optional breathing state and public sidebar/popup/title
HUD snapshots. Items carry namespaced identifiers, metadata, counts, enchantment
presence, durability and leather color only. No item names/lore or private chat
are exported. `practice.spectator.v1.skin` carries bounded bot PNGs with content
hash, dimensions and model. Human skins keep their regular shared API/cooldown.

Generate the runtime carriers with the repository's pinned asset compiler,
including `make actor-assets particle-assets` under `agent-check`. Stage the
legacy eight carriers in `.local/runtime-assets`, then run
`tools/web-spectator/bundle_assets.py .local/runtime-assets --actors .local/assets/compiled/vanilla-v1.mcbeact --particles .local/assets/compiled/vanilla-v1.mcbept`.
The actor and particle paths may instead be pre-staged as `actors.mcbeact` and
`particles.mcbept` in that directory. The bundle requires both and records ten
verified carriers; the legacy asset-manifest route still exposes its original
eight in the same order. Set `SPECTATOR_ASSET_DIR` to the immutable release
directory and mount it read-only in Compose. The loader verifies sizes, hashes,
regular file types and gzip representations before serving. Assets remain
outside Git and container images.
Downloads share two nonblocking admission slots and a60-second write deadline;
active duel skin writes retain the three-second consent-atomic limit.
