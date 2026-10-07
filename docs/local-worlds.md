# Local worlds

Single-player worlds run on a local server behind the same core and game socket as online play.
Full scope: `plan.md` Phase 7.

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
`world_create.v1` without `backend` still uses the store default (`-local-backend=auto`).
`docker_missing` / `docker_not_running` are reported as `backend_unavailable_reason` in status.

## BDS acquisition

Never bundled or committed. `-bds-dir` (default `bds/` beside the worlds dir) holds `<version>/` builds with a
`manifest.json` (URL, zip SHA-256, size, platform, time).

- The build is `server_version` in `assets/bedrock-target.json`; the client passes it as `-bds-version`, fetched from
  its versioned official URL. Without it nothing is downloaded.
- Only https `minecraft.net` / `minecraft-services.net` hosts (including redirects) are accepted; zips are
  size-capped and unpacked with path-escape checks. Mojang publishes no hash, so the SHA-256 is provenance, not a pin.
- The EULA gate: `world_open.v1` on a BDS world fails with code -32012 until `bds_accept_eula.v1 {"accepted":true}`;
  nothing is downloaded before then. Status carries `setup` (state, bytes, runtime, reason, `eula_accepted`).
- Container runtime: the core downloads the Linux build on the host (with byte progress) and mounts it as `/data`;
  `bedrock_server-<version>` beside it makes the image skip its own download. The image is `bds_container_image` in
  the manifest (a tag plus sha256 digest, passed as `-bds-image`); the core refuses an image without a digest.
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
- **Downloads** name the agent `Cinnabar-local-worlds`; minecraft.net resets Go's default one.
- **Exposure:** BDS cannot bind loopback only; it listens on all interfaces on a random port, offline, one slot.
  The container maps its port to `127.0.0.1` only.
- **Lifecycle:** ready on "Server started."; stop is `docker stop` (container) or `stop` on stdin, then kill after 30 s.
- **Pause:** on focus loss or while the pause menu is open, dragonfly suspends its tick loop (`World.SetPaused`, the fork's equivalent of the
  integrated server's sim-time pause): entities, block ticks, time and weather stop; connections stay up and resume
  continues from the same state. BDS does not register `/globalpause` and has no other true pause, so BDS worlds
  keep running and status reports `pause_supported: false`.
- **Test commands:** dragonfly worlds give every player `/speed [multiplier|reset]` (vanilla fly speed and movement
  attribute ×0.1–100, sent as UpdateAbilities and UpdateAttributes), `/fly` and `/tp <x> <y> <z>`.
- **Login:** signed in, the core presents the account's identity. Signed out, it presents a self-signed NetherNet
  identity (BDS refuses anonymous HTTP offers) and an offline login from the client's identity, both admitted
  because `online-mode=false`. Player-data persistence needs a stable client identity.

## Control methods

`world_list/create/update/delete/open/close/pause/status.v1`, `bds_accept_eula.v1`, and `local_worlds_prefs.v1`
(`docker_prompt_dismissed`, `redetect`), all schema v1. `world_update.v1` takes `id` plus any of `name`, `game_mode`,
`difficulty` (applied on the next open); listed worlds carry `size_bytes`.

## Client

`crates/launcher/src/local_worlds` owns the screen model (list, create, templates, edit, delete, open stages,
EULA, Docker modal) and control worker; Play and the OreUI create/edit screens bind to it.

## v1 limits

The owner-requested natural generator has not passed version-matched Bedrock terrain parity; its source targets
Java-style generation. Dragonfly's mob AI and other parity gaps remain open. Docker-mounted LevelDB on macOS
can be slow. A killed core can leave a container running; the next start of that world removes it.
