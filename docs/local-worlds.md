# Local worlds

Single-player worlds run on a local server behind the same core and session endpoint as online play.
The launcher (`crates/launcher-host/src/local_worlds`) owns the world store, the BDS download and the
server processes; the core only routes sessions to the open world. Full scope: `plan.md` Phase 7.

## Backends

Each world records the backend that created it (`world.json`) and always reopens on it; switching is never silent.

| Backend | Where | Notes |
| --- | --- | --- |
| `bds` native | Windows and Linux x86-64 | Official Bedrock Dedicated Server: vanilla terrain and mobs. |
| `bds` container | macOS (or any host without a native build) with a Docker-compatible runtime | Linux BDS in the manifest's pinned `itzg/minecraft-bedrock-server` image, `--platform linux/amd64`. |
| `dragonfly` | built-in local server on every supported host | Normal terrain through `bedrock-mc/vanilla-gen`, or Dragonfly's flat generator; mob behavior has parity gaps. |

The create screen defaults to Dragonfly and Normal (Vanilla). Backend and generator are independent choices;
both backends accept Normal or Flat. Selecting unavailable BDS shows its runtime warning. Creation then offers
Get Docker / Retry or Dragonfly, preserving the chosen generator, name and seed.
A world created without a backend takes the machine's default: BDS where it can run, else Dragonfly.
`docker_missing` / `docker_not_running` are reported as `backend_unavailable_reason` in status.

## BDS acquisition

Never bundled or committed. `bds/` beside the worlds dir holds `<version>/` builds with a
`manifest.json` (URL, zip SHA-256, size, platform, time).

- The build is `server_version` in `assets/bedrock-target.json`, fetched from its versioned official URL.
  Without it nothing is downloaded.
- Only https `minecraft.net` / `minecraft-services.net` hosts (including redirects) are accepted; zips are
  size-capped and unpacked with path-escape checks. Mojang publishes no hash, so the SHA-256 is provenance, not a pin.
- The EULA gate: opening a BDS world fails with `EulaRequired` until the user accepts the EULA; nothing is
  downloaded before then. Status carries `setup` (state, bytes, runtime, reason, `eula_accepted`).
- Container runtime: the launcher downloads the Linux build on the host (with byte progress) and mounts it as
  `/data`; `bedrock_server-<version>` beside it makes the image skip its own download. The image is
  `bds_container_image` in the manifest (a tag plus sha256 digest); an image without a digest is refused.
- Open stages in `setup.state`: `checking_runtime` (`docker info`), `pulling_image` (`layers_done/total`),
  `downloading` (`bytes_done/total`), `unpacking`; the client shows each on the loading screen.

## Runtime behavior

- **Storage:** `<worlds>/<id>/world.json` plus `db/` (a Bedrock world folder: `level.dat` + LevelDB) and `players/`.
  BDS sees `db/` through a directory link at `worlds/<id>` (junction on Windows) or a bind mount, so one world
  runs at a time. Worlds move between backends only where formats allow; dragonfly-written `level.dat` files may
  not satisfy BDS and vice versa, so do not share one folder across backends.
- **server.properties / env:** name, gamemode, difficulty, seed, level type, `online-mode=false`, `max-players=1`,
  view and tick distance from the client's `view_distance` (5-32), `transport=raknet` (1.26.5x defaults to NetherNet,
  which the core cannot dial) and `enable-lan-visibility=false`. Seed and level type apply at creation only.
- **Dragonfly terrain:** the core forwards the saved generator and signed seed to the local server. Normal uses
  the pinned `bedrock-mc/vanilla-gen` generators for Overworld, Nether and End. Its initial spawn comes from the
  generator; reopening preserves the saved spawn and chunks. Flat retains Dragonfly's default generators.
- **Dragonfly animals:** Passive animal simulation and natural population are unsupported. Previously saved
  unsupported actors retain their stored data when their chunks are saved, but are not loaded as live entities.
- **Downloads** name the agent `Cinnabar-local-worlds`; minecraft.net resets unnamed downloaders.
- **Exposure:** BDS cannot bind loopback only; it listens on all interfaces on a random port, offline, one slot.
  The container maps its port to `127.0.0.1` only.
- **Lifecycle:** ready on "Server started."; stop is `docker stop` (container) or `stop` on stdin, then kill after 30 s.
  Server processes are tracked children: on exit the client asks the open world's server to stop its
  own way (`stop`, or `docker stop`), cancels a start in progress, and gives the server its 30 s save
  budget before the exit sweep escalates to SIGTERM and kill.
- **Pause:** on focus loss or while the pause menu is open, dragonfly suspends its tick loop (`World.SetPaused`, the fork's equivalent of the
  integrated server's sim-time pause): entities, block ticks, time and weather stop; connections stay up and resume
  continues from the same state. BDS does not register `/globalpause` and has no other true pause, so BDS worlds
  keep running and status reports `pause_supported: false`.
- **Test commands:** dragonfly worlds give every player `/speed [multiplier|reset]` (vanilla fly speed and movement
  attribute ×0.1–100, sent as UpdateAbilities and UpdateAttributes), `/fly` and `/tp <x> <y> <z>`.
- **Login:** signed in, the core presents the account's identity. Signed out, it presents a self-signed NetherNet
  identity (BDS refuses anonymous HTTP offers) and an offline login from the client's identity, both admitted
  because `online-mode=false`. Player-data persistence needs a stable client identity.

## Core route

The launcher reports the open world to the core with `local_world.v1`: `state` (`idle`, `starting`,
`running`, `stopping`, `failed`), `world` (`id`, `name`, `game_mode`, `backend`), `target` while running
(`address`, `transport` = `raknet` | `nethernet` | `nethernet-lan`, `lan_address`, `level_name`) and a path-free
`error` while failed. The menu treats a world as ready only once the core has acknowledged the running server's exact route. A session waits while the world starts, fails while it stops or has failed, and falls
through to other routes only when idle. A world that begins opening clears any `connect.v1` selection and
pending transfer. The core hosts a running BDS world to Xbox friends and serves `world_invite.v1` for it.

## Client

`crates/launcher/src/local_worlds` owns the screen model (list, create, templates, edit, delete, open stages,
EULA, Docker modal); `crates/client-shell/src/local_worlds` runs its requests against the launcher's world
manager. Play and the OreUI create/edit screens bind to it.

The Edit world screen saves changes before Play starts the server and when Back or Escape leaves
the screen. Invalid names stay in the form for correction; failed saves keep the edits for retry.
The selected game mode sets the world's default for new players and the local Dragonfly host's
mode on each join, while keeping the host's other saved data. BDS launches use `force-gamemode=true`
(including `FORCE_GAMEMODE` in containers), so existing worlds use the selected mode too.

## v1 limits

The owner-requested natural generator has not passed version-matched Bedrock terrain parity; its source targets
Java-style generation. Dragonfly's mob AI and other parity gaps remain open. Docker-mounted LevelDB on macOS
can be slow. A killed client can leave a container running; the next start of that world removes it.
