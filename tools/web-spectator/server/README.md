# Read-only duel spectator ingress

This standalone Go module consumes Practice's existing local NATS connection and
serves transient duel geometry and live player frames. It accepts no browser
gameplay commands and does not connect a Minecraft player or use a database.

## HTTP interface

All spectator routes accept only `GET` and `HEAD` and return `Cache-Control:
no-store`. Browser requests must come from the configured public origin.

| Route | Response |
| --- | --- |
| `/api/spectator/duels` | `{ "duels": [Frame] }`, containing only fresh, complete matches |
| `/api/spectator/duels/{id}/arena` | `{id,name,palette,bounds,blocks}` for an active match |
| `/api/spectator/duels/{id}/events` | SSE `frame` events, followed by `closed` when unavailable |
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
- `practice.spectator.v1.closed`: `{version,id,updatedAt}`.

Limits: 1 MiB per bus message, 4096 voxels per part, 256 parts and one million
voxels per complete arena, 16 cached arenas, two million cached voxels total,
32 simultaneous matches and 32 fighters per frame. Cache state is lost on
restart. Incomplete geometry expires after 30 seconds; arenas serving active
matches cannot be evicted. Only fresh exports from the trusted bus are accepted.

## Build and deploy

Run checks on the shared development host through `agent-check`, with one owner:

```sh
/home/danick/.local/bin/agent-check -- env GOWORK=off go test -race ./...
/home/danick/.local/bin/agent-check -- env GOWORK=off go build -trimpath -o /tmp/zeno-spectator ./cmd/spectator
```

The build context for `Dockerfile` is this module directory. `deploy/compose.yaml`
caps the runtime to one CPU and 384 MiB RAM with no swap. Alternatively, install
the Linux binary at the systemd unit's release path and use
`deploy/zeno-spectator.service` with the same resource limits.

Copy the nonsecret defaults from `deploy/spectator.env.example`; put credentials
only in the deployment environment or a read-only NATS credentials mount. Point
ForwardMe's `dev.zenomc.org` route to `http://10.0.0.69:3002`, and run the separate
website on port 3001. Do not change the current port-3000 public website. The
ForwardMe config is loaded at startup; restart just its proxy after adding the
route. Configure dev DNS for that proxy and verify its certificate before use.
