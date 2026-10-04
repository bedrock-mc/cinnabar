# Experimental Cinnabar extensions

This is a developer spike, off by default. It proves a typed WASM component
boundary, retained JSON-UI output, one local input action, bounded execution,
quarantine and hot reload. It is not a supported marketplace or a vanilla feature.

## Build and run the sample

From the repository root, with the Rust toolchain pinned by the repository:

```sh
rustup target add wasm32-unknown-unknown
cargo build -p hello-mod --target wasm32-unknown-unknown --locked
cargo run -p mod-host --locked -- pack \
  target/wasm32-unknown-unknown/debug/hello_mod.wasm /tmp/cinnabar-hello.wasm
cargo run -p mod-host --locked -- probe /tmp/cinnabar-hello.wasm
cargo run -p mod-host --locked -- bench /tmp/cinnabar-hello.wasm
CINNABAR_MOD_COMPONENT=/tmp/cinnabar-hello.wasm cargo run -p bedrock-client --features local-mods --locked
```

Route local builds through the shared limiter as required by
`docs/agents/multi-agent-workflow.md`. No Go core or server is needed for `probe`,
`bench` or the tests. Launching the client still requires its normal pinned carriers.
No mod is loaded when the environment variable is absent; builds without `local-mods`
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
cargo run -p mod-host --locked -- pack \
  target/wasm32-unknown-unknown/debug/time_changer_mod.wasm /tmp/cinnabar-time-changer.wasm
cargo run -p mod-host --locked -- probe-environment /tmp/cinnabar-time-changer.wasm
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

`crates/mod-api/wit/extension.wit` is the single interface definition. The guest
SDK uses `wit-bindgen`; the host independently generates Wasmtime bindings from
that same file. Copy `examples/mods/hello` to start a mod, adjust its dependency
path, and implement its generated `Guest` trait. `pack` converts the core WASM
module and embedded WIT metadata to a component. The guest's actual imports
declare its requirements; unknown imports fail linking. The prototype's
grant is HUD, the demo action and environment for the developer-selected mod,
with separate explicit opt-ins for gameplay reads and camera writes. It has
no permission prompt or install manifest yet.

The host admits no WASI, filesystem, network, Bevy or GPU import. Gameplay data
arrives only as a bounded app-owned snapshot, not a world handle. Each
callback receives a fresh fuel budget and publishes at most one validated label
and one visual time override, plus one current-frame camera delta when granted.
Resource limits are defined in `crates/mod-host/src/lib.rs` and `runtime.rs`.
Invalid text is rejected; labels are plain text and cannot carry formatting codes.
The label's original host-owned JSON template goes through the existing JSON-UI
engine and compiled carrier. Guest strings never become JSON or binding expressions.

The app integration sits after semantic input finalization and before UI
publication, between physical look and movement. With the switch absent it
installs no mod resource or update system.
Required vanilla carriers remain required. The extension adds no protocol types
or dependencies on gameplay state to the component host.

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
GPU work. No-mod scheduling installs zero extension systems. These development
profile measurements are diagnostic, not a release frame-budget acceptance test.

The host tests execute actual components, including traps, endless loops,
memory growth, missing authority and failed/successful reloads. The Rust sample
is independently built to WASM and exercised with `probe` and the snapshot test; native workspace tests
alone do not prove guest code generation. `bench` reports warmed batch per-frame
idle and action crossing costs with fuel checks, excluding compile, disk polling, JSON-UI and GPU
costs. Its development-profile figures are spike evidence, not a release frame
budget or vanilla performance acceptance.

This in-process spike cannot contain compiler OOM, host defects or runtime native
crashes. Compilation and file reads are synchronous. Before admitting downloaded
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
