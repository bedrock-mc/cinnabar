# Experimental Cinnabar extensions

This is a developer spike, off by default. It proves a typed WASM component
boundary, retained JSON-UI output, one local input action, bounded execution,
quarantine and hot reload. It is not a supported marketplace or a vanilla feature.

## Build and run the sample

From the repository root, with the Rust toolchain pinned by the repository:

```sh
rustup target add wasm32-unknown-unknown
cargo build -p hello-mod --target wasm32-unknown-unknown --locked
cargo run -p mod-host --bin mod-host --locked -- pack \
  target/wasm32-unknown-unknown/debug/hello_mod.wasm /tmp/cinnabar-hello.wasm
cargo run -p mod-host --bin mod-host --locked -- probe /tmp/cinnabar-hello.wasm
cargo run -p mod-host --bin mod-host --locked -- bench /tmp/cinnabar-hello.wasm
CINNABAR_MOD_COMPONENT=/tmp/cinnabar-hello.wasm cargo run -p bedrock-client --features local-mods --locked
```

Route local builds through the shared limiter as required by
`docs/agents/multi-agent-workflow.md`. No Go core or server is needed for `probe`,
`bench` or the tests. Launching the client still requires its normal pinned carriers.
No mod is loaded without an environment selection or enabled local registration; builds without `local-mods`
do not compile Wasmtime and ignore it with a warning.

The last command opens the launcher. Select a server only in a separately
authorized live session. To exercise the same startup switch, scheduled adapter,
window-focus check and F8 action entirely offline, run:

```sh
CINNABAR_MOD_COMPONENT=/tmp/cinnabar-hello.wasm \
  cargo test -p bedrock-client --features local-mods --lib configured_sample_drives_the_app_adapter_offline --locked
```

During gameplay, F8 changes the label. UI focus and window focus suppress the
action. Without the additional gameplay grants below, the mod cannot generate
camera input, chat, packets or world mutations. Rebuild
the guest and package to a temporary sibling file, then atomically rename it over
the selected component to reload. Reload resets guest state and commits the new
label only after initialization succeeds; a broken replacement keeps the previous
instance. A runtime trap removes its label and disables callbacks until a changed,
valid component is loaded.

## Visual time changer

`environment.set-time-override(option<u32>)` accepts a fixed tick within one
Bedrock day; `none` immediately restores tracked server time. Access is denied by
default; `ModGrants.environment` grants it per instance. The developer component
switch grants it explicitly. Writes are bounded and transactional; quarantine
clears the override and successful reload starts fresh. Only atmosphere and light
inputs consume it: server clock updates and simulation continue, with no packets.
The override stays fixed regardless of the server daylight-cycle rule.

`examples/mods/time-changer` starts at `Time: Server`; F8 cycles Day, Sunset,
Night, Midnight, then Server. Night uses the vanilla night preset, distinct from
midnight. Build with the already installed `wasm32-unknown-unknown` target (otherwise
install it separately with `rustup target add wasm32-unknown-unknown`):

```sh
cargo build -p time-changer-mod --target wasm32-unknown-unknown --locked
cargo run -p mod-host --bin mod-host --locked -- pack \
  target/wasm32-unknown-unknown/debug/time_changer_mod.wasm /tmp/cinnabar-time-changer.wasm
cargo run -p mod-host --bin mod-host --locked -- probe-environment /tmp/cinnabar-time-changer.wasm
CINNABAR_MOD_COMPONENT=/tmp/cinnabar-time-changer.wasm cargo run -p bedrock-client --features local-mods --locked
```

Use the shared build limiter for every cargo command, as for hello. The last
command opens the launcher; the existing offline adapter and snapshot commands
also accept this component. The dedicated offline cycle/no-packets test is
`configured_time_changer_is_visual_only_offline` with `CINNABAR_MOD_COMPONENT` set and
`--features local-mods`.

Vanilla time uses six presets, preset-table lookup and modulo 24000, including
celestial time wrapping. The vanilla 1.26.50.4 pack's
`texts/en_US.lang:3665–3671` identifies the time preset labels;
`:1993–1995` describes freezing the daylight cycle. Existing atmosphere math remains
subject to its current parity limits; this mod adds no native acceptance claim.

## Contract and implementation

### Read-only local player state

`player-state.read-snapshot()` exposes the local player's current presented
inventory, worn armor, offhand and active status effects. It requires the separate
default-denied `ModGrants.player_state` grant (`CINNABAR_MOD_PLAYER_STATE=1` for
the explicitly selected developer component). It returns `ok(none)` without a
connected world or when the network, player and UI session owners disagree.
Screens may retain this read capability while they own input; the grant carries
no camera, interaction, remote-player or outbound packet authority.

The snapshot contains the network session generation, current dimension and
selected hotbar cell. Inventory has exactly 36 cells, hotbar first, and armor has
four cells in helmet-to-boots order. Every inventory/gear cell preserves unknown,
empty and present states with `known` and an optional item. Items carry their
negotiated identifier when available, wire metadata/count and canonical block
classification. Unknown identifiers remain unknown and still carry their counts.
Durability damage uses accepted response corrections before the retained Damage
tag; missing damage remains absent. Durability maxima use the existing Bedrock
vanilla table only for non-component items. Custom component maxima are unknown,
without a guessed vanilla fallback.

Effects use the existing authoritative UI effect store and estimated server
clock. Remaining duration is in 20 Hz ticks, `none` means infinite, and expired
effects are omitted even before the ordinary UI expiry pass. Effects are sorted
by ID; wire amplifiers remain zero-based. Reads are limited to eight per callback.

`player-state.read-revision()` shares that grant and read limit. It returns an
opaque `u64` token only while the current callback has a valid snapshot. Equal
tokens within one component instance mean every snapshot field is unchanged,
including identifiers, session, dimension, exact effect ticks and flags. A guest
may compare the token before importing the full snapshot and must release cached
facts when it observes `none`. Tokens do not survive reloads. The host retains one
exact comparison snapshot only when the guest imports it with `read-snapshot`.
Current contents equal to that full read reuse its token, including after
unavailable callbacks or unimported transient changes. Other contents receive a
fresh token; returned tokens are opaque and need not increase. Comparison facts
are never exposed without a current snapshot and are cleared on validation
failure, revocation or callback failure. The allocation counter rejects
theoretical exhaustion instead of wrapping.
Current snapshots are cleared before and after callbacks and on quarantine; a reload
starts without previous session data. This experimental API closes no vanilla
parity or native acceptance gate.

### Opt-in gameplay API

Personal developer components may request `gameplay.read-frame()` and
`gameplay.rotate(yaw-delta, pitch-delta)`. Both capabilities are denied by
default, independently of the visual time grant. Explicitly opt in at startup:

```sh
CINNABAR_MOD_COMPONENT=/tmp/my-mod.wasm \
CINNABAR_MOD_PLAYERS=1 CINNABAR_MOD_CAMERA=1 \
  cargo run -p bedrock-client --features local-mods --locked
```

On PowerShell, set the corresponding `$env:CINNABAR_MOD_*` variables before
launching the client. Only the exact value `1` grants access. These grants apply
only to the explicitly selected personal component, not to server bundles.

`read-frame` returns an error without the players grant, or `ok(none)` outside
active gameplay. A snapshot contains the actor session, dimension, local subject
eye position, actor yaw/pitch, frame duration in seconds, held semantic attack
action and up to `mod_api::MAX_GAMEPLAY_PLAYERS` remote player feet positions.
Positions are world block coordinates; angles and rotation deltas are radians
in the actor's YXZ convention: positive yaw turns left, positive pitch turns up.
Nearest players come first, with runtime ID breaking distance ties. The local
player, mobs, non-finite positions and player-list entries without loaded actors
are excluded. Runtime IDs are session-scoped. These are committed actor positions,
not interpolated render poses; the list does **not** assert line of sight,
on-screen visibility, friendship or server permission.

`rotate` requires the separate camera grant and a current gameplay snapshot.
It adds to physical look in the same frame, before movement, physics and camera
publication. The sum of accepted writes per axis must remain within
`mod_api::MAX_CAMERA_DELTA_RADIANS`; non-finite or excessive deltas are rejected.
The app retains its existing pitch limit and preserves subject position and roll.
There are at most eight read calls and eight rotation calls per callback; exceeding
either budget traps the guest. Output is committed only after a successful
callback and consumed once. Initialization, loss of gameplay authority, a trap or
successful reload cannot leave a rotation queued for a later frame.

The app provides no gameplay frame when the window is unfocused, the cursor is
released, a screen owns input, no world is connected, an acceptance camera is
running, or a server camera is active. The default client still installs no
extension systems. A camera write changes the local actor's look; the normal
movement/network path may report that look to the server. This is not a
presentation-only override and carries no server approval claim.

A guest can use `attack-held` to choose between continuous assistance and
assistance only during the attack action, and `frame-seconds` to scale a strength
setting independently of rendering speed. Target selection and strength remain
guest policy; this API does not install an aim-assist algorithm. No process memory,
OS input synthesis or vision model is needed.

`crates/mod-api/wit/extension.wit` (`cinnabar:extension@0.1.0`) is the single interface
definition of player mods. It grows additively: a component built against an earlier 0.1
links as long as every import it names still exists. Its world `extension` is the bare
component's; world `player-mod` includes it and adds package screens, session data and event
callbacks (see "Mod packages and screens" below). One host linker serves both. (A
server Experience's client part is another world, `server-bundle`, which `experience-sdk`'s
`client` feature builds; see [server-experiences.md](server-experiences.md).) The guest
SDK uses `wit-bindgen`; the host independently generates Wasmtime bindings from
those same files. Copy `examples/mods/hello` to start a bare mod (`mod_api::bindings`) and
implement its generated `Guest` trait, or `examples/mods/screen-probe` for a package: implement
`mod_api::PlayerMod`, overriding only the events the mod uses, and export it with
`mod_api::export_player_mod!`. Adjust the copy's dependency path. `pack` converts the core WASM
module and embedded WIT metadata to a component. The guest's actual imports
declare its requirements; unknown imports fail linking. The prototype's
grant is HUD, the demo action and environment for the developer-selected mod,
with separate explicit opt-ins for gameplay reads and camera writes. It has
no marketplace permission prompt or signed package format.

The host admits no WASI, filesystem, network, Bevy or GPU import. Gameplay data
arrives only as a bounded app-owned snapshot, not a world handle. Each
callback receives a fresh fuel budget and publishes at most one validated label
and one visual time override, plus one current-frame camera delta when granted.
Resource limits are defined in `crates/mod-host/src/lib.rs` and `runtime.rs`.
Invalid text is rejected; labels are plain text and cannot carry formatting codes.
The label's original host-owned JSON template goes through the existing JSON-UI
engine and compiled carrier. Guest strings never become JSON or binding expressions.

The app integration sits after semantic input finalization and before UI
publication, between physical look and movement. Without an active selection it
installs no guest runtime or gameplay output. A `local-mods` build watches its
local registration on a background worker; default builds have no watcher.
Required vanilla carriers remain required. The extension adds no protocol types
or dependencies on gameplay state to the component host.

## Personal controls and interaction

Three additional per-component grants are opt-in: `CINNABAR_MOD_CONTROLS=1`,
`CINNABAR_MOD_INTERACTION=1` and `CINNABAR_MOD_SETTINGS=1`.
They are developer extension capabilities and do not change the vanilla client.

`panel.set-content` retains a bounded JSON panel of toggles, sliders, buttons and
choices. It uses the host's JSON-UI engine. Optional `style: "compact"` renders a unified menu with up
to three equal-height cards. Optional `theme: "monochrome"` selects an opaque neutral
palette for either layout; omitting it preserves the existing dark/light theme.
Sections can select a bounded `icon` (`pointer`,
`crosshair`, `ruler`, `settings` or `none`). A `keybind` control has `id`, `label`,
`key` and optional `capturing` fields; pressing its keycap emits a button event,
and the guest owns key capture and reservations. Key changes retain geometry.
Choices open a host-owned option list. Slider numbers open a bounded text editor;
Enter applies finite values within the declared range and normalizes the step.
Escape or an outside click cancels an editor before closing the panel. Keyboard
input belongs to the editor while it is open; reserved emergency and panel keys
retain priority. The existing choice/slider event payloads are unchanged.
Optional sections organize controls into category tabs and
cards; omitting them keeps a flat panel. `input.read-controls` supplies current-window physical key
edges and panel events. `input.reserve-keys` prevents selected bindings reaching
gameplay. The panel's `toggle_key` opens or closes it before the ordinary input
sample; Escape closes it. Other absorbing screens and lost focus close it, release
input, and suppress gameplay output. Removing or quarantining a guest releases
the panel and its reservations.

`input.read-selected-controls(selection)` reads the same current frame with the
same controls grant and shared read limit. Each optional pressed/held key list
uses `none` for all keys, an empty list for none, or up to 64 physical names for
exact matches in native order. Names follow the existing 32-byte alphanumeric
key rule; repeated requested names do not duplicate observations. The `events`
flag selects whether panel events are included; scalar flags are unchanged.
Selection neither consumes input nor changes reservations or native sampling.

An optional `surface` replaces the built-in panel presentation with extension-owned
JSON-UI. It has `screen` (`namespace.name`), `document` (a JSON string containing
that namespace and one root definition), and `bindings` (a map of `#name` to a
boolean, finite number, bounded text or 2–4-number array). The private catalog
accepts bounded screen, panel, button, label, stack-panel and rectangle/vector
custom nodes; factories, inheritance and dynamic expansion are rejected. The
whole panel is capped at 128 KiB; its document at 96 KiB, 1,024 nodes and depth 32.
Bindings update without rebuilding unchanged catalog geometry. Host bindings
`#surface_width` and `#surface_height` report the current logical viewport.
With a surface, the optional panel `reference_size` pair declares positive logical
dimensions up to 1,000,000. Smaller viewports proportionally reduce rendering and
input geometry to fit that extent; larger viewports keep the normal scale.

Authored buttons route `mod.control:N` to declared control index N, `mod.edit:N`
to a declared slider's native number editor, or `mod.close` to dismiss the panel.
Choice lists, keybind events, slider normalization and input ownership retain
their native behavior. A guest can declare Button controls for navigation or
search and process physical key edges while its `capture_key` flag is active;
layout, branding and navigation state remain in that guest.

`gameplay.set-attack-reach` requests a current-frame actor selection/admission
range up to `mod_api::MAX_ENTITY_REACH_BLOCKS`. It preserves obstruction checks
and normal attack transactions; the server still decides whether a hit is valid.
`gameplay.pulse-attack` requests one press only while the captured semantic Attack
action is physically held. Neither operation synthesizes OS input. These requests
commit after a successful guest callback and expire each frame.

`settings.load/save` reads or atomically replaces only the selected component's
`.settings.json` companion. JSON objects and file reads are bounded by
`mod_api::MAX_SETTINGS_BYTES`; there is no guest-selected filesystem path.
Successful callbacks commit settings in memory immediately. One worker coalesces
atomic disk writes, reports failures separately, and flushes the last value on exit.

With an explicit component and controls grant, `CINNABAR_MOD_FONT` may select a
bounded local outline font for the personal panel. It is rasterized once per
selected font into an isolated atlas alias with filtered sampling. Panel sizing follows display DPI
independently of the game GUI scale; vanilla and server glyph ownership are preserved.

## Custom rendering

`render` is a separate local grant (`CINNABAR_MOD_RENDER=1`, or `"render": true` in
`local-mod.json`); depth reads also need `render_depth`. Budgets live in `mod_api`.

- **Post passes.** `register-pass` takes WGSL that defines
  `fn effect(uv: vec2<f32>) -> vec3<f32>` against a host prelude (`scene`, `param`,
  `blur`, `bloom`, `world_to_uv`, and `depth` or `world_position` with depth). naga
  validates the composed module. The guest may not declare resources, overrides or entry
  points, and may not loop. Worst-case texture reads and expressions per pixel, with call
  sites expanded, must fit the budget, as must source size, tokens per statement (which
  bounds nesting) and the size of every type. Validation runs on its own thread, and a frame
  callback may compile one shader. A rejection returns the reason to the guest. Passes
  run by `(order, name)` after post-processing and before the HUD, each reading the
  previous colour. `update-pass` retains an enable flag and 16 floats, and disabled passes
  cost nothing, and replaced or reloaded passes release their pipelines. Each slot is
  timed as `gpu_mod_pass_N`.
- **World primitives.** `draw` appends decals, ribbons, beams and billboards for the
  current callback. Each successful callback replaces the drawn set; an identical set
  rebuilds and uploads nothing. One premultiplied,
  depth-tested draw without depth writes runs in the transparent phase, timed as
  `gpu_mod_primitives`.
- Both commit only after a successful callback. A trap, reload, revocation or unload
  clears them.

### Render sample

`examples/mods/render-sample` registers a vignette pass, which F8 toggles, and draws a
pulsing ring at the player's feet:

```sh
cargo build -p render-sample-mod --target wasm32-unknown-unknown --locked
cargo run -p mod-host --bin mod-host --locked -- pack \
  target/wasm32-unknown-unknown/debug/render_sample_mod.wasm /tmp/cinnabar-render.wasm
cargo run -p mod-host --bin mod-host --locked -- probe-render /tmp/cinnabar-render.wasm
CINNABAR_MOD_COMPONENT=/tmp/cinnabar-render.wasm CINNABAR_MOD_RENDER=1 CINNABAR_MOD_PLAYERS=1 \
  cargo run -p bedrock-client --features local-mods --locked
```

Replacing the component reloads it as for other mods. A rejected shader shows its error as
the mod's label.

## Mobs, camera rig, commands and cues

`CINNABAR_MOD_ENTITIES=1` grants `gameplay.read-mobs`: up to
`mod_api::MAX_GAMEPLAY_MOBS` non-player actors within `MAX_MOB_RANGE_BLOCKS` of the
eye, nearest first, with type ID and replicated health. `gameplay.set-camera-rig`
(camera grant) retains a third-person boom in camera-local blocks plus roll and FOV
change, swept against blocks like the vanilla boom; it presents third-person-back
until `none`, a trap or a reload. `CINNABAR_MOD_COMMANDS=ability` (comma-separated)
lets `gameplay.request-command` send `/ability ...` as a vanilla player command
request; any other command is refused, and requests are capped by
`MAX_COMMANDS_PER_FRAME` and `MAX_COMMANDS_PER_SECOND`. `events.emit` publishes bounded
cues in the app's `ModCueFeed`; `events.poll` returns last frame's cues, at most
`MAX_INCOMING_CUES`. `input.read-controls` also reports held keys. All output commits
only after a successful callback and is dropped on a trap or reload.

`camera.set-view-scale` (camera grant) renews independent FOV and look multipliers
each callback. FOV accepts `MIN_VIEW_FOV_SCALE..=1` and look accepts
`MIN_VIEW_LOOK_SCALE..=1`; both must be finite. It applies only to focused,
input-owned gameplay with no open extension panel. FOV updates in the current
camera frame; look gain applies on the next look update. Neutral `1/1`, omission,
focus loss, UI input ownership, traps and unload restore the player's view without
changing saved FOV, sensitivity or perspective. In a mod set the earliest
non-neutral publisher wins.

## Several mods at once

`CINNABAR_MOD_SET=/abs/mods.json` loads up to `mod_api::MAX_LOADED_MODS` components,
each with its own grants (the `local-mod.json` names), budgets, trap quarantine and hot
reload. A component that fails to load is skipped:

```json
{"version": 1, "mods": [
  {"component": "/abs/camera.wasm", "grants": {"players": true, "camera": true, "controls": true}},
  {"component": "/abs/hud.wasm", "grants": {"environment": true}}
]}
```

File order settles conflicts: the earliest camera rig, rotation, time override, attack
reach and non-zero packet delay win; a key reserved by an earlier mod never reaches a later
one; the first mod with a panel owns it; labels join with ` | `; commands and cues keep load
order. Render passes merge by name with the earliest mod keeping a contested name, and passes
and each primitive kind fill the single-mod budgets in load order. Each mod polls
every mod's previous-frame cues. The set takes precedence over `CINNABAR_MOD_COMPONENT`
and the registration watcher, which still load a single mod.

## Attach a local component to a running client

A `local-mods` build watches `local-mod.json` in `InstallLayout.user_config_root`
(on an installed Windows client, `%LOCALAPPDATA%/Cinnabar/`). Write it atomically:

```json
{
  "version": 1,
  "request_id": "local-request-1",
  "enabled": true,
  "component": "C:/Local/mod.component.wasm",
  "font": "C:/Local/panel.ttf",
  "grants": {
    "environment": false,
    "players": true,
    "camera": true,
    "controls": true,
    "interaction": true,
    "settings": true,
    "render": false,
    "render_depth": false,
    "entities": false,
    "commands": []
  }
}
```

The registration is bounded to 16 KiB, paths must be absolute, and grants default
to false. Component compilation, source polling, font rasterization and status
writes run on the worker. The current generation is installed before physical
input sampling; deleting, disabling or invalidating the registration revokes its
runtime and restores input ownership. A request-ID-only change preserves guest
state. An environment-selected component takes precedence over registration.

`local-mod.status.json` reports the request ID, client PID and `loaded`, `error`
or `disabled` state. `loaded` acknowledges actual installation, rather than only
successful compilation. The guest still has no filesystem or process access.
This loader cannot be added to an executable that is already running without it;
older clients require one update and restart.

## Verification and limits

```sh
cargo test -p mod-host --locked
cargo test -p bedrock-client --features local-mods --lib modding --locked
cargo test -p bedrock-client --lib mod_hud --locked
```

To capture the real compiled sample through the existing offline renderer:

```sh
mkdir -p /tmp/cinnabar-mod-frames
CINNABAR_FORM_SNAPSHOT_DIR=/tmp/cinnabar-mod-frames \
CINNABAR_MOD_SNAPSHOT_COMPONENT=/tmp/cinnabar-hello.wasm \
  cargo test -p bedrock-client --lib mod_spike_snapshot_with_real_carrier --locked -- --nocapture
```

This writes before, initial-label and keybind PNGs without starting a network
session. The test requires the real carrier when a snapshot directory is set.

Measure the real CPU UI build with zero mods and with the sample loaded:

```sh
CINNABAR_MOD_SNAPSHOT_COMPONENT=/tmp/cinnabar-hello.wasm \
  cargo test -p bedrock-client --lib mod_spike_offline_frame_overhead --locked -- --ignored --nocapture
```

This alternates warmed baseline/sample batches at the same viewport and reports
both totals and paired differences. It includes the guest callback, label adapter
and JSON-UI build; it excludes Bevy scheduling, reload polling, rasterization and
GPU work. Default builds install zero extension systems. These development
profile measurements are diagnostic, not a release frame-budget acceptance test.

The host tests execute actual components, including traps, endless loops,
memory growth, missing authority and failed/successful reloads. The Rust sample
is independently built to WASM and exercised with `probe` and the snapshot test; native workspace tests
alone do not prove guest code generation. `bench` reports warmed batch per-frame
idle and action crossing costs with fuel checks, excluding compile, disk polling, JSON-UI and GPU
costs. Its development-profile figures are spike evidence, not a release frame
budget or vanilla performance acceptance.

This in-process spike cannot contain compiler OOM, host defects or runtime native
crashes. Environment-selected reload compilation and file reads remain synchronous;
registration loading uses its bounded worker. Before admitting downloaded
mods, move compilation/execution to a restricted helper with process memory/time
limits, bounded IPC and watchdog restart. Recheck the runtime's security support
and advisories before release. The design also requires signed packages, explicit
permission grants, revocation, a server policy protocol and cross-platform tests.
There is no claim of server approval or native visual acceptance.

HUD visibility remains incomplete: the existing vanilla data source hardcodes
HUD-visible bindings and alpha. The spike suppresses its label for focus, menus,
loading and a statically hidden underlying HUD, but does not yet follow vanilla
hide-GUI, partial server HUD visibility or animated opacity. See `plan.md`; the
hidden-HUD test is not full visibility parity evidence.

## Mod packages and screens (world `player-mod`)

A package is a directory: `mod.toml`, `mod.wasm` (a `player-mod` component), `ui/<name>.json`
templates and `textures/`. `CINNABAR_MOD_PACKAGE=<dir>` loads it (with `--features
local-mods`) and takes precedence over `CINNABAR_MOD_COMPONENT`. A set entry
`{"package": "/abs/dir", "grants": {...}}` loads one beside other mods. Its settings companion
sits beside the directory, `<dir>.settings.json`, outside the hashed files.
`examples/mods/screen-probe` is a complete package; the host tests build it.

Templates are the mod's own files, hashed in `mod.toml` and resolved against the vanilla catalog
only, as a client part's modal templates are; guest strings still never become JSON. The panel
above stays host-templated.

```toml
id = "bei"                  # lowercase name; also the templates' JSON-UI namespace
version = "0.1.0"
api = "0.1"                  # the cinnabar:extension version
permissions = ["screen", "items", "recipes", "keys"]
templates = ["ui/overlay.json", "ui/recipes.json"]
textures = ["textures/arrow.png"]
actions = ["bei.next_page"]  # control ids and edit box names, in the mod's namespace

[[keys]]
id = "bei.show_recipes"
key = "r"                    # a-z, 0-9, f1-f12, backspace, page_up, page_down
modifiers = []               # any of "ctrl", "shift", "alt"; must match exactly
label = "key.bei.show_recipes"

[files]                      # lowercase SHA-256 of mod.wasm, every template and texture
"mod.wasm" = "<sha256>"
```

`experience_sdk::mod_manifest` is the one parser. The host refuses a package whose files miss
their hashes or differ from them, that hashes a file it does not declare, that puts an action
or key outside its namespace, repeats a key or binding, or ships a template of another
namespace. A build script calls `experience_sdk::declarations::generate_mod("mod.toml")`
(which ignores `[files]`, filled after the build) and the guest includes `actions::*`,
`templates::*` and `keys::*` with `experience_sdk::include_declarations!()`.
`CINNABAR_MOD_PACKAGE` grants what the manifest asks for, plus the environment opt-ins a bare
component gets; in a set, each package permission needs both the manifest's ask and the entry's
grant (`screen`, `items`, `recipes`, `keys`). `inventory` has no import yet. On reload, the new
manifest narrows the original loader authorization again; an entry cannot gain an ungranted
permission by changing its manifest.

**Screens.** `screen.set-overlay(template)` draws one template beside every container screen.
It is laid out over the whole root after the container screen. The host drops every node and
control of it that meets the GUI rect (the union of vanilla's laid-out panels, an open recipe
book included) or an exclusion area, so a mod cannot cover or intercept a vanilla slot, and
vanilla's held stack and tooltips draw above it. A press on an overlay control is the mod's
and never drops the held stack. `screen.open-view(template)` draws a template over the
still-open container (no `ContainerClose`) and hides vanilla's screen and its keys. Escape
(when no edit box is selected) or `open-view(none)` returns; the inventory key or closing the
container closes both. When the host closes the view (Escape, the container closing) the mod
gets `view-closed`. While the view is up the overlay is clipped against the view's drawn
bounds (`screen-layout.view`) instead of the hidden screen's GUI rect, so it sits beside the
view as JEI's list sits beside its recipes screen, and takes the pointer, wheel, edit boxes
and key rows wherever it draws there. `screen.layout()` and `screen-changed`
report the vanilla screen name, the root size and GUI scale, the GUI rect, the exclusions and
the view's bounds, in GUI units; opening, closing or resizing the view delivers
`screen-changed`. Templates,
data binding (`set-collection`, `set-value`, `set-text`) and their limits are the client part
modal's (server-experiences.md, Modal screens); `focus-text` selects an edit box. Rows bind
`#item_id_aux` (network id << 16 | aux) and `item_renderer` draws the item. The mod's atlas
has its own four dynamic texture pages. With several mods loaded, one owns the screens: the
earliest in load order with an overlay or view, else the earliest granted `screen`. Only it
draws and receives screen input and keys; every package granted `items` or `recipes` reads
the session.

**Session data.** `cinnabar:session@0.1.0` (`crates/experience-sdk/wit/session/session.wit`)
is read-only. `items` gives the creative content in its order, then the registry's other
items: identifier, aux, icon key, localized name, creative group and category, maximum stack
and tags, plus `lookup` and `tag-members`. `recipes` gives shaped and shapeless crafting,
stonecutter, cartography, smithing transform and trim, with ingredients as an item, any aux
of an item, or a tag. Both page at `MAX_PAGE` (256) per call and carry a revision; a changed
revision delivers `data-changed`. They are the facts vanilla's recipe book and creative
screen show, with no packet, world or account access. Known gaps: CraftingData carries no
smelting at the 1.26.x target, brewing is skipped, and a recipe with a Molang, complex,
deferred or int-id ingredient is dropped by the decoder.

This package is the one definition client parts use too. SP5's planned `items.lookup` for
client WIT 1.2 is `cinnabar:session/items.lookup`: the `server-bundle` world imports
`cinnabar:session/items@0.1.0` from this same file (as a further WIT path, the way mod-api
reads `client/deps/server-experience`) instead of restating it in `capabilities.wit`.

**Keys.** While a container screen or the view is up and no edit box is selected, a press of
a declared key that vanilla's container screen does not use (the inventory and drop
bindings, Escape, Q, 1 to 9, arrows, Page Up and Page Down; over the view only the inventory
key and Escape) delivers `key(id, hovered, row)`. `hovered` is the vanilla slot's stack under
the pointer; `row` is the collection name and index of the mod's control under it, by the
rule `action` uses. Modifiers must match exactly, so `o` with `["ctrl"]` is Ctrl+O. No key
reaches the mod during gameplay.

**Callbacks.** `player-mod` exports `screen-changed`, `action`, `secondary-action`,
`scrolled`, `text-changed`, `key`, `data-changed` and `view-closed` beside `init` and `frame`.
Each event export is optional: the host type-checks every one a component exports (refusing
one of another signature) and never calls one it lacks, so new events arrive as new exports
without breaking built mods. `data-changed(sources)` lists what changed (`items`, `recipes`);
that enum is closed, and a new kind of change arrives as a new export.
`scrolled` reports wheel notches (a pixel wheel's pixels / 16), positive scrolling down, and
the Ctrl, Shift and Alt held. An event callback commits the label, visual time, fullbright, panel,
settings and screens; render, camera and command output is `frame`'s alone.

`data-changed`, whose sources a mod copies whole across the ABI, and the `init` of a component
exporting it get `LOAD_FUEL` (100,000,000; twice
vanilla's session measured 27.7M in the probe), after the Experience runtime's
`REGISTER_FUEL`. Every other event callback gets `CALLBACK_FUEL` (10,000,000) and `frame`
keeps 100,000; any other component's instantiation and `init` keep the frame budget. A
session that outgrows the load budget calls for a host-side query API
(items and recipes on demand) rather than a larger copy. One frame's events are
coalesced: the latest layout first, one data change naming every source, then the rest in order with the latest
text per edit box. A trap or exhausted fuel quarantines the mod and removes its overlay and
view; a refused template does too. An undeclared action or key is refused without
quarantine. Reload re-reads the whole package and delivers the current layout and data to
the new instance.

Verification: `cargo test -p mod-host --locked --lib` (real components for every callback,
fuel, traps, caps, revisions, permissions and reload), `cargo test -p experience-sdk --lib`
(the manifest) and `cargo test -p client-ui --lib mod_screens session_data` (clipping, the
lifted held stack, hit filtering, session data). The overlay has not been checked on a
rendered frame yet; see `plan.md`.

## Loaded block highlights

The separate `block_highlights` grant (`CINNABAR_MOD_BLOCK_HIGHLIGHTS=1`) permits
`render.set-block-highlights`. A retained specification names up to
`mod_api::MAX_BLOCK_HIGHLIGHT_IDENTIFIERS` canonical block identifiers, a bounded
camera-relative range, and linear RGBA colour. The host scans only loaded primary
block layers, caches palettes and subchunk identities, and draws full unit cubes
through terrain without changing world or packet state. Results share the
`mod_api::MAX_BLOCK_HIGHLIGHTS` nearest-block budget; the earliest active mod wins.
`none`, unload, reload, or a trap clears the overlay. Output commits only after a
successful callback; repeated unchanged input rebuilds no geometry.
## Fullbright

The separate `fullbright` grant (`CINNABAR_MOD_FULLBRIGHT=1`) permits
`environment.set-fullbright`. Enabling it replaces the shared world light table
with full illumination without changing time, stored lighting or server state.
Disabling it restores the current environment. The flag is retained after
successful callbacks and clears on traps, unload and reload. Unchanged input
uploads no new table; inactive world sessions suppress the override.

Block highlights inspect received primary block layers even while collision
readiness is incomplete. Missing subchunks and unloaded data remain excluded.

## Embedding an isolated client

`CINNABAR_USER_ROOT` optionally selects an absolute profile directory on each desktop
platform, including development binaries. Configuration goes under `config`, data
and caches under `data`, and runtime files under `run`. Bundled resources stay at
the executable's normal installation location. Relative and empty overrides fail
at startup. This does not require changing HOME, LOCALAPPDATA or XDG variables.
`CINNABAR_WINDOW_TITLE` optionally changes the game window title; absent, empty or
whitespace-only values retain the product name.

### Cosmetic HUD cards and crosshairs

The separate `hud` grant permits `hud.set-content(json)` and
`hud.set-crosshair(json)`. Select it with `CINNABAR_MOD_HUD=1`, or `"hud": true`
in the component's registration/set grants. Neither operation reads player data,
changes input, nor sends packets. Existing `set-label` remains available without
this grant. All HUD writes share an eight-call callback budget.

Card data contains `cards`, each with a unique `id`, `rows`, and optional
`title`, `anchor`, `offset`, `scale`, `position`, and `background_opacity`. Anchors are `top_left`, `top_right`, `bottom_left`,
and `bottom_right`; offsets are GUI pixels from that corner. Negative offsets
move inward from right/bottom corners. Scale is 0.5–2 and scales card text, icons,
and geometry together. Each row has `label`, `value`, and optional `item`
(resource identifier), `metadata`, `effect_id` (canonical Bedrock effect icon),
`progress` (0–1), and `color` (RGBA 0–1). Item and effect icons are mutually exclusive;
unknown effect IDs leave the icon empty rather than guessing.
Item art follows the current session's resource-pack icons and normal item atlas;
unknown item identities draw no substituted item. Optional `row_layout` selects
`standard` (default, inline text with a left icon), `stacked_text` (label above
value beside a left icon), or `icon_right` (inline text before a right icon).
`width`, `row_height`, and `icon_size` use unscaled GUI pixels and default to
148, 20, and 16. Width is 48–512, row height is 12–64, and icon size is 4–48;
icons must fit the row height and the width minus 12 pixels of padding.
Optional `text_scale` (default 1, bounded to 0.5–2) multiplies the row label and
value fonts independently of icons and card dimensions. Inline text bands expand
within the row padding and progress-bar space; stacked rows retain separate line
bands. Text stays clipped to its available band when the chosen row is too small.
Card height is `(22 + row_height * rows) * scale` with a title, or
`(4 + row_height * rows) * scale` without one. Gameplay rendering and layout
editor bounds use the same dimensions. Background opacity
is 0–1 (default 0.82) and affects only the card surface, preserving foreground
text, icons and progress bars. An optional normalized `position: [x, y]` in
0–1 overrides corner placement: each axis is a fraction of available travel
(viewport minus scaled card size), keeping moved cards visible across resizing.
Legacy anchor/offset placement remains exact until a card is moved.

Crosshair data supports `shape` (`cross`, `dot`, `circle`), `size` (cross arm
length or circle/dot radius), `gap` (cross center clearance), `thickness`, `color`,
`outline`, and `outline_color`. Dimensions use GUI pixels. The host renders the
replacement inside the ordinary JSON-UI cursor renderer, preserving its camera,
spectator, pack, focus, and HUD visibility gates. Clearing the spec restores the
player's existing cursor texture and blending preference.

Both operations retain validated data only after a successful callback. Empty
JSON clears that surface. A trap or unload revokes it; a rejected replacement
keeps the old instance, and a successful reload starts with the new instance's
published surfaces. Cards hide while a screen or personal panel owns input,
during loading, and when the player or server hides the HUD. Text/value updates
reuse the retained host JSON-UI template while its geometry is unchanged.
The settings panel now accepts at most 64 controls; existing category and page
navigation keeps controls reachable when they exceed the viewport.

With both `hud` and `controls` grants, `hud.open-editor(json)` opens a native
JSON-UI layout editor from the current focused personal panel. The request is
bounded HUD preview data and can include cards disabled during gameplay.
`editor_label` names a preview independently of its optional gameplay title.
Set `hide_editor_labels: true` on the editor request to hide those labels while
keeping draggable outlines. An empty card list can still open the editor and
its optional extension surface. While dragging, cards snap within six GUI pixels
of the screen edges or center; a temporary guide shows the aligned axis.
Each card edge and its center can align to a guide, including placement directly
above or below the horizontal center line. Optional `reset_anchor`, `reset_offset`,
and `reset_scale` supply factory layout without changing the current preview.
These fields affect only the layout editor.

The editor captures native pointer dragging, clamps cards to the available
viewport, supports one-pixel arrow nudges, and offers optional eight-GUI-pixel
grid snapping. Save (or Enter) returns all card IDs and their optional normalized
positions through `hud.read-editor-result()`. The typed result has `saved`,
`reset`, and `placements`; `reset` indicates that Reset was used in this draft.
Reset clears positions and uses the supplied factory anchors/offsets while
preserving opacity and restoring scale only when `reset_scale` is supplied. Cancel (or Escape) returns `saved: false` and no
placements, so the guest leaves its preferences untouched. Save and Cancel return
to the personal panel, which retains exclusive gameplay input ownership.

An editor request may opt into `resizable: true` to expose four native corner
handles. Dragging a handle scales the card uniformly from 0.5 to 2, keeps its
opposite corner fixed when space permits, and clamps its placement to the viewport.
Use `hud.read-editor-layout()` to read `saved`, `reset`, and `placements` containing
ID, normalized position, and scale. It shares result consumption with the existing
`hud.read-editor-result()`; the legacy position-only API remains unchanged.

An editor request may opt into `autosave: true`. Completed pointer drags and resizes, arrow
nudges, and Reset then return incremental saved results while the editor stays
open. A held or interrupted drag remains a draft. Escape closes the personal
panel in this mode; completed placements have already been delivered, and the
unfinished gesture is discarded. Omitting `autosave` preserves Save/Cancel.

The optional editor `surface` has the same bounded document and bindings as a
personal panel surface. Its client-authored chrome replaces the built-in shade,
grid, toolbar, and help while native previews and drag bounds remain. Editor
actions accept `hud.save`, `hud.cancel`, `hud.close`, `hud.reset`, `hud.grid`, and
bounded `hud.card:N`. `hud.close` dismisses the whole panel, preserving completed
autosave placements and discarding an unfinished drag. `hud.done:N` ends the editor and emits the declared Button
control at panel index N through the ordinary control event queue. It retains panel
input ownership so the component can choose its next surface. Other control
types and undeclared indices cannot emit this event.

A result stays stable for the callback and is consumed only after a successful
callback reads it. The app delivers it only to the requesting component, even
when another component publishes the gameplay HUD. Focus loss, a different live
session, another screen, a guest trap, reload, or unload cancels pointer capture
and the draft. The host has no preference-file authority through this API;
components may persist accepted placements with the existing settings grant.

## Local movement input

The separate `movement` grant (`CINNABAR_MOD_MOVEMENT=1`, or `"movement": true`
in a component registration) exposes `gameplay.read-movement` and `pulse-jump`.
The snapshot belongs to captured gameplay and contains its session, dimension,
simulation tick, velocity in blocks per tick, ground contact, physical held jump,
ordinary walking eligibility and local knockback sequence. No snapshot is retained
across callbacks. Snapshot validation rejects mismatched authority and non-finite velocity.

`pulse-jump` stages a single ordinary jump input only while walking is eligible and
physical jump is released. The request commits after a successful callback and is
consumed once by the next physics tick. Focus loss, menus and authority changes
revoke pending input; no velocity or movement rule is changed. Jumping remains
subject to the simulator's ground contact and jump cooldown rules.

`cancel-jump` commits an explicit cancellation even outside gameplay, allowing a
component to revoke a sub-tick pulse as soon as its module is disabled. Cancellation
wins over jump pulses in the same callback and never clears physical jump input.
