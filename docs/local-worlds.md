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

New worlds use BDS and offer Infinite or Flat terrain. If BDS is unavailable, creation shows
its runtime warning and the Docker setup prompt. Existing Dragonfly worlds keep their backend.
Each Worlds-tab row tags its server (Dragonfly or BDS) after its game mode and generator.
`docker_missing` / `docker_not_running` are reported as `backend_unavailable_reason` in status.

## World options

General settings select the name, game mode and difficulty. Advanced, Multiplayer and Cheats
use the shared option catalog in `launcher::local_worlds::options`. The catalog owns the labels,
defaults, bounds, cheat gating and BDS destinations. Changing a cheat setting enables cheats;
turning cheats off restores the gated rules' defaults.

World metadata stores individual pending edits and inactive preferences. After a successful open,
the world file supplies the current rules, including changes saved during play. Before BDS starts,
the host updates the owned fields in `level.dat` and preserves other tags. Game rules, starting map,
bonus chest, sleep settings,
respawn radius and daylight settings use level data. Server properties apply cheats, simulation
distance and default player permissions to both native and container servers.
Like a vanilla world owner, the signed-in host is an operator through the install's
`permissions.json`, keyed by XUID; friends and other joiners get the default permission. A start
made while the account's profile loads waits for its XUID. A signed-out host has no XUID, so it
keeps the default too.
Turning multiplayer off limits the server to its host and withdraws friend hosting.
LAN visibility is not offered: both runtimes bind and advertise loopback only, so discovery stays off.
A new world's first start lets BDS create it from server.properties and stops it once ready, so its
own new-world setup (spawn search, world version, flat layers) runs; the host then patches only
owned fields of that level.dat and starts it again. The host never writes level data BDS has not
created.
Reopening waits for the prior server to finish saving before it reloads rules and applies pending edits.
Seed and terrain apply during creation; starting map and bonus chest are also fixed after creation.
The edit screen exposes the remaining options.
Older and imported worlds recover their options from level data. A `level.dat` over 1 MiB is
never parsed or rewritten; that world's options show as unavailable.

While the host plays a BDS world, Settings lists a Game section under World in vanilla's in-game
order and wording. Name, seed, world type, starting map, bonus chest, simulation distance and
hardcore stay locked; cheat rules wait for cheats, and turning cheats off keeps them. Each change
saves at once and reaches the running server through its console (`gamerule`, `difficulty`,
`defaultgamemode`, `alwaysday`, `changesetting allow-cheats`); container BDS takes the line
through the image's `send-command`. Default player permission applies on the next start. Rules
applied this way stay pending until a clean stop, when BDS has saved them itself.

Resource packs, behaviour packs and experiments remain unavailable. Xbox Live, Realms and
friends-broadcast settings are not exposed on the local BDS multiplayer page. Hardcore remains
unavailable, as do custom flat presets and Education Edition. Runtime acceptance of every option across supported BDS platforms remains open.

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
- **server.properties / env:** name, gamemode, difficulty, seed, level type, `online-mode=false`,
  the multiplayer player limit, `view-distance` at the client's largest render distance (each
  RequestChunkRadius picks the radius), independent simulation distance (4–12), `transport=nethernet`
  and `enable-lan-visibility=false`. Seed and level type apply at creation only.
- **Dragonfly terrain:** the core forwards the saved generator and signed seed to the local server. Normal uses
  the pinned `bedrock-mc/vanilla-gen` generators for Overworld, Nether and End. Its initial spawn comes from the
  generator; reopening preserves the saved spawn and chunks. Flat retains Dragonfly's default generators.
- **Dragonfly animals:** Passive animal simulation and natural population are unsupported. Previously saved
  unsupported actors retain their stored data when their chunks are saved, but are not loaded as live entities.
- **Downloads** name the agent `Cinnabar-local-worlds`; minecraft.net resets unnamed downloaders.
- **Exposure:** native BDS listens on a random port with offline authentication and the configured player limit.
  The container publishes its transport and enabled discovery ports on `127.0.0.1` only.
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
  `/flyspeed <blocks/s>` sets direct motion flight for chunk-streaming tests, for example `/flyspeed 500`.
  Use `/fly` if the current game mode does not allow flight, then double-tap jump to start flying.
  Movement follows yaw and raw forward/strafe input; jump and sneak move vertically, and releasing input stops motion.
  `/flyspeed off` restores the previous flight abilities; `/speed` also switches back to ability flight.
  Direct flight has no speed multiplier cap, but motion must fit the protocol's floating-point values.
  Cinnabar's existing collision-query extent also limits accepted motion; acceptance beyond that bound remains incomplete.
- **Login:** signed in, the core presents the account's identity. Signed out, it presents a self-signed NetherNet
  identity (BDS refuses anonymous HTTP offers) and an offline login from the client's identity, both admitted
  because `online-mode=false`. Player-data persistence needs a stable client identity.
- **Offline:** native BDS gathers ICE candidates only from non-loopback interfaces and has no other
  transport, so on a device with no network its SDP answer carries no candidate. The core fails such
  a join at once with `cinnabar.disconnect.localWorldOffline` rather than waiting for a candidate.
  Joining a BDS world offline remains unsupported.

## Core route

The launcher reports the open world to the core with `local_world.v1`: `state` (`idle`, `starting`,
`running`, `stopping`, `failed`), `world` (`id`, `name`, `game_mode`, `backend`), `target` while running
(`address`, `transport` = `raknet` | `nethernet` | `nethernet-lan`, `lan_address`, `level_name`) and a path-free
`error` while failed. The menu treats a world as ready only once the core has acknowledged the running server's exact route. A session waits while the world starts, fails while it stops or has failed, and falls
through to other routes only when idle. A world that begins opening clears any `connect.v1` selection and
pending transfer. When multiplayer is enabled, the core hosts a running BDS world to Xbox friends
and serves `world_invite.v1` for it.

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
can be slow. A killed client can leave a container running; the next start of that world removes it
before reserving ports.
