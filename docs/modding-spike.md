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
CINNABAR_MOD_COMPONENT=/tmp/cinnabar-hello.wasm cargo run -p cinnabar --locked
```

Route local builds through the shared limiter as required by
`docs/agents/multi-agent-workflow.md`. No Go core or server is needed for `probe`,
`bench` or the tests. Launching the client still requires its normal pinned carriers.
Every native build includes the mod host and Wasmtime, including builds with
`--no-default-features`. The host has no optional execution feature. No guest is loaded
without an environment selection or an enabled local registration.

The last command opens the launcher. Select a server only in a separately
authorized live session. To exercise the same startup switch, scheduled adapter,
window-focus check and F8 action entirely offline, run:

```sh
CINNABAR_MOD_COMPONENT=/tmp/cinnabar-hello.wasm \
  cargo test -p cinnabar --lib configured_sample_drives_the_app_adapter_offline --locked
```

During gameplay, F8 changes the label. UI focus and window focus suppress the
action. Without the additional gameplay grants below, the mod cannot generate
camera input, chat, packets or world mutations. Rebuild
the guest and package to a temporary sibling file, then atomically rename it over
the selected component to reload. Reload resets guest state and commits the new
label only after initialization succeeds; a broken replacement keeps the previous
instance. A runtime trap removes its label and disables callbacks until a changed,
valid component is loaded.

## Single-file presentation resources

An installed binary component may carry optional resources in these singleton
top-level WebAssembly custom sections. No filesystem extraction or additional
guest imports are required.

| Section | Contents | Source limit |
| --- | --- | --- |
| `cinnabar:extension/panel-bundle@1` | Existing UTF-8 panel bundle JSON | 8 MiB |
| `cinnabar:extension/font@1` | Outline font bytes | 2 MiB |
| `cinnabar:extension/menu-logo@1` | PNG main-menu logo | 2 MiB |
| `cinnabar:extension/menu-title@1` | Nonempty UTF-8 single-line heading | 64 bytes |
| `cinnabar:extension/notices@1` | UTF-8 copyright and license text | 4 MiB |
| `cinnabar:extension/icon@1` | PNG Mods-tab icon, at most 256 by 256 pixels | 256 KiB |

Embedded resources are supported only in the single-file component form, not in
directory packages. Directory packages retain their existing asset lifecycle and
4 MiB component ceiling. Resources require a binary component and are rejected when duplicated, nested,
truncated or over their limits; an icon's size or content only affects its card.
Recognized resources are removed before compiling
the component; executable bytes and unrelated custom metadata retain the existing
4 MiB limit. License text has no required product schema and grants no authority.
The PNG must be at most 4096 by 2048 pixels and 4,194,304 pixels in total. Its
original source bytes remain intact; the existing artwork decoder fits the image
into its bounded atlas and native main-menu layout preserves its aspect ratio.

Panel bundles are available before guest initialization. Panel and font use require
the controls grant; local main-menu branding requires the environment grant.
Declaring requested grants in component metadata never grants them automatically.
Enabled instances use their existing registration order for branding precedence.
Disabling, removing or quarantining the selected instance restores the next active
instance or Cinnabar's shipped branding. Server-defined pause and loading logos
keep their existing priority.

Install through Settings, Mods, Import, review the requested grants, then Enable.
To update while preserving the same registration and preferences, atomically
replace the installed component at its current path and select Reload. Importing
a differently named version creates another registration.

`CINNABAR_MOD_COMPONENT=/absolute/path/component.wasm` also accepts embedded
resources with the existing explicit capability switches. Optional
`CINNABAR_MOD_FONT` and `CINNABAR_MOD_UI` sidecars override the embedded sources.
Changed environment components prepare the guest, panel, font and branding on a
bounded worker, then publish them together. Invalid replacements retain the
accepted instance; preferences changed during preparation cause a fresh retry.

## Mods-tab name, version and icon

Mod cards read the WebAssembly Component Model's standard metadata sections, the
ones `wasm-tools metadata add` writes: the component name, `description`,
`version`, `authors` and `licenses`. The file name is the fallback name. For example:

```sh
wasm-tools metadata add --name "Zoom Plus" --description "Smooth zoom" \
  --version 1.2.0 --authors "Example Author" --licenses MIT mod.wasm -o mod.wasm
```

Recent `cargo component` releases write the same sections from `Cargo.toml` package
metadata. Each field displays as one line. Name is limited to 64 bytes,
description 512, version 32, authors 256 and licenses 128. A field that is too
long, not UTF-8 or has control characters is ignored.

Append an icon to the finished component as the top-level custom section
`cinnabar:extension/icon@1`, for example with `wasm-encoder`'s `CustomSection`.
A section inside the core module is rejected like other embedded resources. The
icon must be one PNG within the limits above that decodes completely; otherwise
the card shows the placeholder. Metadata and icons are read by parsing the file
only, and their content never causes a load to fail.

### Optional GitHub mod updates

Authors can opt into automatic updates by appending one top-level UTF-8 custom
section `cinnabar:extension/update-url@1` (`mod_api::UPDATE_URL_SECTION`). Its
contents identify the exact WASM asset on the latest stable GitHub release:

```text
https://github.com/example/zoom-plus/releases/latest/download/zoom-plus.wasm
```

The HTTPS URL must use that path, a public GitHub repository, and an ASCII asset
name ending in `.wasm`. URL queries, credentials, fragments and nested paths are
rejected. The URL is bounded to 512 bytes, with an asset name up to 128 bytes.
Missing, duplicate, nested or invalid sections disable automatic updates without
affecting the mod's loading. The component also needs a standard `version`
section containing SemVer, and each release must retain the same update URL.

For notifications alone, authors can instead append
`cinnabar:extension/update-repository@1`
(`mod_api::UPDATE_REPOSITORY_SECTION`) containing the public repository URL,
such as `https://github.com/example/zoom-plus`. Repository URLs up to 256 bytes
accept an optional `.git` suffix or trailing slash. When both sections are
present, the download URL selects the repository.

On launch, Cinnabar checks opted-in repositories in the background and compares
the latest release tag, such as `v1.3.0`, with the installed version. Drafts,
prereleases, equal/older versions and non-SemVer tags are skipped. Each repository
is checked once per session, including newly imported mods; network work is
limited to 32 repositories and 32 automatic download attempts per launch.
Missing releases, network failures and API limits leave the installed mod usable.
These public requests use no GitHub login or token.

With automatic installation enabled, Cinnabar selects the exact named release
asset and downloads its immutable GitHub asset ID. The asset must have GitHub's
SHA-256 digest and fit the host's component size limit. Cinnabar checks the hash,
component compatibility, declared source and embedded version before atomically
replacing the unchanged file in the managed mods folder. Settings companions and
registration, including permission grants, are preserved. Enabled mods reload
through the existing source watcher, which retains the running instance if
activation fails. Disabled mods stay disabled. Components registered outside the
managed mods folder can show notifications but are not replaced automatically.

A newer release shows the launcher's corner update card. **View mod updates**
opens **Settings → Mods**. For mods with a WASM download URL, **Update** downloads,
validates, replaces and reloads the component inside Cinnabar. It works even when
automatic installation is off. Its live progress uses the pack downloader's byte
units, rolling speed estimate and ETA, with a percentage and byte progress bar.
The stages distinguish **Downloading**, **Verifying**, **Installing** and
**Reloading**. Installation and reload use an indeterminate indicator rather than
a fabricated percentage; enabled mods stay busy until the runtime acknowledges
the new instance. Duplicate presses are blocked until completion or failure. Download or installation failures leave the
existing file in place, show an error and allow another attempt. Repository-only
notification sources retain **View release**, which opens GitHub because they do
not identify an installable file. Successful installs report their version on the
Mods tab and clear the available-update notice. App updates take priority in the corner.

Players can disable **Install mod updates automatically** in **Settings →
Updates**, under **Mods**, while retaining notifications, or disable **Check for mod updates**
to stop both checks and downloads. Both mod settings work independently of the
app updater, including development builds. Mods without either declaration make
no update requests.

## Generic action filtering

A player mod can explicitly export `filter-actions(action-frame) -> action-verdict`
under the `controls` grant. Use `mod_api::filtered_player_mod` bindings to opt in;
ordinary `PlayerMod` exports do not claim ownership. Only the earliest active,
granted exporter runs. It receives resolved Attack/Use transitions, physical and
logical hold state, a continuity epoch, and absolute monotonic microseconds.
Ordered desktop transitions preserve source order. Phase-only devices use a
canonical channel order and cannot recover sub-frame ordering.

The callback runs after ordinary frames and before gameplay, with independent
fuel and no host-import calls. It returns one accept/suppress decision per observed
transition and may finish retained releases. Suppressing a release retains its
hold; suppressing the following press joins it. Accepting that press completes the
outstanding release first. Suppressing a fresh press blocks its gesture until
physical release. Authorized attack pulses appear as `repeat`; rejecting one leaves
the hold unchanged. Every emitted press has an observed source in the same frame.

Callbacks run on transitions and every frame with a retained hold. Idle physical
holds need no callback. Traps, invalid verdicts, owner replacement, loss of capture,
input-absorbing UI, suspension, session/dimension changes and authority gaps clear
continuity. Retained releases complete; suppressed gestures remain suppressed until
their physical release. A changed epoch tells the module to discard policy state.
Native code owns projection and lifecycle only. Modules own all filtering rules,
deadlines and rates; the host imposes no hold-duration policy.

## Click limiting and local weather

`input.set-click-limits(attack, item-use)` is a frozen legacy compatibility API.
New modules should implement policy through `filter-actions`. It retains separate optional press caps.
Each cap accepts 1 through `mod_api::MAX_CLICK_RATE` (100) presses per second;
`none` restores ordinary input for that channel. It requires `controls` authority
and cannot generate clicks. Excess gestures are discarded through their release;
accepted held mining and item-use gestures keep ordinary behavior. Ordered desktop
bursts and aggregate input phases share the filter. Caps combine by taking the
strictest active cap per channel. Cooldowns reset at gameplay, input-authority,
session and dimension boundaries, with no delayed replay.

`environment.set-weather-override(option<weather>)` selects `clear`, `rain` or
`thunder` under the existing `environment` grant and output precedence. It affects
local sky, fog, light and precipitation inputs. Authoritative weather and its
normal display interpolation keep advancing independently, so `none` restores the
current server presentation. Local presets suppress server lightning flashes;
they do not create lightning entities or change server simulation or audio.

Both policies publish only after successful callbacks, retain across deferred
callbacks, clear on quarantine, and start fresh after a successful reload.
No default setting changes. A component must opt into these APIs. Headless
populated-world rendering and interaction acceptance remain to be completed.

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
CINNABAR_MOD_COMPONENT=/tmp/cinnabar-time-changer.wasm cargo run -p cinnabar --locked
```

Use the shared build limiter for every cargo command, as for hello. The last
command opens the launcher; the existing offline adapter and snapshot commands
also accept this component. The dedicated offline cycle/no-packets test is
`configured_time_changer_is_visual_only_offline` with `CINNABAR_MOD_COMPONENT` set.

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
  cargo run -p cinnabar --locked
```

On PowerShell, set the corresponding `$env:CINNABAR_MOD_*` variables before
launching the client. Only the exact value `1` grants access. These grants apply
only to the explicitly selected personal component, not to server bundles.

Personal-mod capabilities use explicit user consent in both local and remote worlds.
Servers do not grant or restrict those permissions. Drawing, entity observation,
terrain reads, block highlights, materials and post effects each retain their own
permission controls and component-declared requests. Drawing resources and observations
remain scoped to the live connection and dimension; disconnects, transfers, reloads,
traps and revocation clear the previous instance's output. Mod updates retain this
user-consent policy independently of server negotiation, mod digest allowlists and
server-grant expiry.

`read-frame` returns an error without the players or terrain grant, or `ok(none)` outside
active gameplay; the terrain grant alone omits remote players. Terrain reads
(`CINNABAR_MOD_TERRAIN=1`) are described in [mod-terrain-api.md](mod-terrain-api.md). A snapshot contains the actor session, dimension, local subject
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
installs no guest runtime or gameplay output. Every native build watches its local registration on a background worker.
Required vanilla carriers remain required. The extension adds no protocol types
or dependencies on gameplay state to the component host.

## Personal controls and interaction

Three additional per-component grants are opt-in: `CINNABAR_MOD_CONTROLS=1`,
`CINNABAR_MOD_INTERACTION=1` and `CINNABAR_MOD_SETTINGS=1`.
They are developer extension capabilities and do not change the vanilla client.

`panel.set-content` retains a bounded JSON panel of toggles, sliders, buttons,
choices and text fields. It uses the host's JSON-UI engine. Optional
`style: "compact"` renders a unified menu with up to three equal-height cards. Optional `theme: "monochrome"` selects an opaque neutral
palette for either layout; omitting it preserves the existing dark/light theme.
An open personal settings panel hides the normal gameplay HUD. The layout editor
keeps it visible so users can align their widgets with the hotbar and crosshair.
Sections can select a bounded `icon` (`pointer`,
`crosshair`, `ruler`, `settings` or `none`). A `keybind` control has `id`, `label`,
`key` and optional `capturing` fields; pressing its keycap emits a button event,
and the guest owns key capture and reservations. Key changes retain geometry.
A `text` control has `id`, `label`, `value`, optional `min_chars` (default 0) and
`max_chars` (default 256). Bounds count Unicode characters; values stay single-line.
Its native editor supports selection, character-safe navigation and cancellation.
Command/Ctrl-A, C, X and V select, copy, cut and paste only while a native field
has focus. Paste remains subject to the field's single-line and character bounds.
Optional `live: true` emits the complete value on every edit, coalesced per field
between frame callbacks. Enter submits and keeps a live field focused. The additive
`input.read-field-events()` API returns `change({id, value})`, `submit({id, value})`
and `key({id, key, shift, control, alt, super-key})` variants. Changes may temporarily
fall below `min_chars`; submitting still requires the full bounds. Omitting `live`
preserves Enter-to-commit behavior. The legacy text-event API receives commits only.

Optional `navigation_keys: true` forwards Up/Down, Left at the start, Right at the
end, and Enter/Shift+Enter through field key events while the field has focus.
Horizontal arrows first collapse any selection. Other horizontal movement,
Home/End, Shift selection, and Ctrl/Option word movement and deletion stay native.
IME preedit appears inline with an underline and remains local until commit.
Its transient spelling may exceed the field's committed character limit.
The caret uses the painted glyph positions and the caller's animation time.

Committed text arrives through `input.read-text-events()` as `{id, value}` entries,
separately from the unchanged numeric events in `input.read-controls()`.
The batch is visible only during its regular frame callback, remains stable across
reads and stays queued when the callback is deferred. It shares the controls grant
and read budget. Before running a frame, native integrations first drain
`ModUi::take_mod_panel_text_events(owner)` into `ModHost::queue_text_events`, then
`ModUi::take_mod_panel_field_events(owner)` into `ModHost::queue_field_events`.
Draining field events retains the same valid, coalesced values that the host
admits. Below-minimum drafts remain editable without changing the retained value.
Admission validates each complete batch before updating any retained value.
Optional `show_gameplay_hud: true` retains the resource-pack gameplay HUD behind an
open panel, for example while placing HUD elements. It defaults to false. Hide HUD
and death-screen suppression still apply, and the crosshair stays hidden while a
panel owns input. This does not enable other extensions' overlay cards. The value
can change with a full panel update without replacing the retained document.

Optional `motion: true` enables authored surface transitions, short native hover
and toggle easing, and smooth discrete wheel scrolling. Pixel scrolling and
scrollbar dragging remain immediate. Authored surfaces can bind `#row_N_hover` and
`#row_N_toggle_progress` and `#row_N_toggle_inverse_progress` (complementary numbers
from 0 to 1), `#row_N_toggle_color` and
`#row_N_knob_color` (RGBA arrays), and `#row_N_knob_offset` (logical coordinates
from `[2, 2]` to `[12, 2]`). These values follow native input without waiting for a
component update. Mod feedback is independent of the built-in Screen animations
setting; the mod chooses whether to enable `motion`. `ModUi::mod_panel_motion_active` reports unsettled feedback for
callers that suspend idle drawing. Arbitrary authored animation graphs remain
unsupported.

Native module collections expose the same live feedback as `#item_ROLE_hover`,
`#item_ROLE_toggle_progress`, and `#item_ROLE_toggle_inverse_progress`. They read
the current native values without republishing immutable tile artwork.

`#row_N_toggle_enabled` and `#row_N_toggle_disabled` are complementary boolean
bindings for custom enabled and disabled artwork. They follow the current toggle
value immediately, independently of any animated progress value.

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
key rule; repeated requested names do not duplicate observations. The retained panel catalog accepts up to 512 controls, while each input batch
still accepts at most 64 events. The `events`
flag selects whether panel events are included; scalar flags are unchanged.
Selection neither consumes input nor changes reservations or native sampling.
The observed key lists also include `MouseLeft`, `MouseRight`, and `MouseMiddle`.
Pressed entries are physical down edges rather than held repeats; held entries
track the current window and clear on focus loss. Observing buttons does not
cancel ordinary gameplay clicks.

An optional `surface` replaces the built-in panel presentation with extension-owned
JSON-UI. It has `screen` (`namespace.name`), `document` (a JSON string containing
that namespace and one root definition), and `bindings` (a map of `#name` to a
boolean, finite number, bounded text or 2–4-number array). The private catalog
accepts bounded screen, panel, button, label, stack-panel and rectangle/vector
custom nodes, scroll views and their track/box controls; factories, inheritance
and dynamic expansion are rejected. The whole panel is capped at 128 KiB; its
document at 96 KiB, 1,024 nodes and depth 32.
Bindings update without rebuilding unchanged catalog geometry.

An optional `transitions` map eases numeric bindings instead of replacing them at
once, like CSS transitions. Each key names a binding; its value has `duration`
(seconds, 0–5), optional `easing`, optional `from` and optional `exit`:

```json
"transitions": {
  "#drawer_offset": { "duration": 0.25, "easing": [0.16, 1, 0.3, 1], "from": [0, 6], "exit": [0, 6] },
  "#drawer_alpha": { "duration": 0.18, "easing": "out_cubic", "from": 0, "exit": 0 }
}
```

`easing` is a JSON-UI curve name (default `linear`) or CSS cubic-bezier points
`[x1, y1, x2, y2]` with both x values in 0–1 and y values within ±4. When a
binding changes, the host eases from the value currently on screen, so a reversed
hover or toggle continues smoothly rather than restarting. Arrays ease each
component. `from`, a number or 2–4-number array, is shown when the binding
first appears, so the surface eases in each time the panel opens or its surface
route changes; without it the first value appears at once. Text, Boolean values
and a change of array length apply at once. A transition may name a binding that
a later update supplies. Bind eased values to any numeric property, such as
`#offset`, `#alpha`, `#color` or `#size_binding_x_absolute`.

`exit`, a number or 2–4-number array of the binding's length, is where the
binding eases when the personal panel closes. Input, pointer capture and
gameplay focus are released at once, while the surface keeps drawing with the
same duration and easing until every transition settles, then disappears.
Bindings without `exit` keep their values. Reopening during an exit eases back
from the values on screen. A panel with no `exit` values, or whose exit would
not move anything, closes at once as before. Exits play only for the panel's
own close, not when the HUD editor owned the surface, when the extension clears
its panel, or when rendering fails. An update with the same route continues the
exit with its new bindings; a route change while closing ends it at once, and a
route change while open swaps surfaces at once, easing the new route in from its
`from` values. `ModUi::mod_panel_motion_active` reports transitions and exits
still in progress, and `ModUi::mod_panel_painting` reports a closed panel that is
still drawing.

Surface transitions and exits require `motion: true` and remain independent of
the built-in Screen animations setting. Setting `motion: false` settles active
feedback, wheel scrolling and surface transitions immediately and removes a
pending exit. Re-enabling motion affects future changes without replaying the
entrance. The host can also disable all panel motion with
`ModUi::configure_motion(false)`; a panel cannot override that preference.

A control may declare `cinnabar_transform` to scale and rotate how it and all its
descendants are drawn, like CSS `transform`, without changing layout:

```json
"cinnabar_transform": { "scale": 1.15, "rotation": 45, "origin": [0.5, 0.5] }
```

`scale` is a number or `[x, y]` pair within 0–4 (default 1), `rotation` is
clockwise degrees within ±360 (default 0), and `origin` is an `[x, y]` pair of
fractions of the control's box within −1–2 (default the centre). Every field is
optional, so `{}` declares a control whose transform only bindings set. Nested
transforms compose like CSS. The binding targets `#cinnabar_scale`,
`#cinnabar_rotation` and `#cinnabar_origin` replace one field on a control that
declares the property, so `transitions` can animate a press scale of `0.975`, a
hover scale of `1.15`, a panel entrance from `0.97` or a chevron turning to `45`:

```json
"bindings": [{ "binding_name": "#chevron_turn", "binding_name_override": "#cinnabar_rotation" }]
```

Bound values outside the bounds are clamped. Presses, hover, slider and scroll
drags use the drawn position, so a scaled or rotated button is hit where it
appears. A scaled control's `clips_children` and scroll views clip to their
scaled boxes. Clips are axis-aligned, so a control that may rotate (a non-zero
`rotation` or a `#cinnabar_rotation` binding) cannot be or contain a clipping
control or scroll view; validation rejects the surface instead. Rounded
rectangles and vector art are rebuilt at their drawn size, keeping a one-pixel
antialiased edge; other art and text scale their vertices. Text keeps
sampling its glyph atlas as at any other size, and distance-field faces keep
one-pixel edges; pixel faces sample nearest texels. Plain solid quads have no
antialiasing, so rotate a `cinnabar_rounded_rectangle` rather than a filled
image. Only extension surfaces honour the property; resource packs and server
UI ignore it.

A control may declare `cinnabar_filter` to blur how it and all its descendants are
drawn, as one image, like CSS `filter: blur()`:

```json
"cinnabar_filter": { "blur": 5 },
"bindings": [{ "binding_name": "#menu_blur", "binding_name_override": "#cinnabar_blur" }]
```

`blur` is a Gaussian standard deviation in GUI pixels within 0–16 (default 0); the
`#cinnabar_blur` binding target replaces it on a control that declares the property, so a
transition with `"from": 5` blurs a menu in as it opens. Bound values are clamped. Filters
cannot nest, and a blur binding without the property is rejected. A zero blur draws the
subtree directly at no extra cost. Layout and hits are unchanged, and the blur spreads up
to three standard deviations beyond what the subtree draws. While the blurred subtree's
layer redraws, the client draws the subtree offscreen, downsamples it within those bounds
and composites it blurred in its place; an unchanged layer reuses the result. Controls in
other layers that interleave with the subtree split it into separately blurred groups.
Glass inside a filtered subtree blurs its tint, border and highlight with the subtree,
while its backdrop keeps sampling the world.

For larger local menus, `CINNABAR_MOD_UI` selects a bounded JSON bundle, or a
running-client registration can supply its absolute path in `ui`. The bundle has
an initial ordinary `panel` and a `surfaces` object mapping route identifiers to
`{ "screen": "namespace.name", "document": "..." }`, optionally with
`transitions`. The loading worker checks
every document and action against the shared control schema before
publication. Bundles accept up to 128 surfaces and 8 MiB of source data.
`ModHost::set_panel_bundle` prepares the same contract for host-owned integrations.
The registration worker and direct environment startup also call
`PreparedPanelUi::prepare(host.panel_bundle().unwrap())` and pass its result to
`ModUi::set_prepared_panel_ui(owner, Some(prepared))` before the first frame.
Use `PanelUiOwner::Environment(index)` for each startup component or
`PanelUiOwner::Registration` for the independently registered component. Remove
that owner's entry with `None` when it unloads. Standalone integrations prepare
and install their owner entry outside rendering, then publish accepted values
with `ModUi::set_mod_panel(owner, host.panel())` using that same owner. Passing
`None` clears that owner's active panel without clearing another owner's panel.
Prepared routes retain their catalogs and
resolved roots; returning to a route reuses its binding and measurement caches.
Route changes continue to release pointer capture and reset the scroll position.

`panel.update-content(json)` then applies a bounded 32 KiB patch:

A bundle may declare `control_schemas`, mapping a surface route to its own control
array. Each route is validated against that array during loading. Selecting a route
with a different schema starts from its declared values; the same patch can set the
saved values. Repeated updates to the same schema retain omitted values. Routes
without an override use the bundle's initial controls. Per-route limits still apply.

Use `binding_changes` for sparse scalar edits that preserve other retained
bindings. The existing `bindings` field still replaces the complete map. Both
forms validate the final binding count and value domains before publishing.

Large tile menus can declare `collections` on a personal-panel surface. Each entry
has an immutable `definition` and an `order` containing unique item indexes. The
definition supplies a small static `template` surface, `columns`, a fixed logical
`viewport`, and up to 256 `items`. An empty panel with
`"cinnabar_collection": "modules"` places the native scrollable collection named
`modules`; its authored offset places the viewport. The host owns its grid and
scrollbar and creates tiles around the visible region. Native cell clipping and
the existing surface node budget bound live work.

Each item supplies scalar `values` named `#item_*`, a `controls` map from local
action roles to admitted control indexes, and optional finite vector `art`.
Templates read ordinary global `#item_*` bindings; the host scopes these to their
tile. A role named `toggle` supplies `#item_toggle_value` and, for a toggle control,
`#item_toggle_toggle_enabled`, `#item_toggle_toggle_disabled`, and
`#item_toggle_toggle_progress`. Vector art supplies `#item_art_paths`,
`#item_art_view_box`, `#item_art_stroke_width` and `#item_art_closed`.
`mod.item:toggle` routes through that item's admitted control. Targets are resolved
when hits are painted, so a subsequent filter cannot redirect an old hit.

Use `"collection_order": { "modules": [8, 2, 5] }` in a panel patch to filter or
sort retained metadata. An empty list shows no tiles. Unknown indexes, duplicates,
unbounded artwork, nested collections and tiles escaping their cell clip are
rejected before publication. Templates cannot contain surface bindings or
transitions of their own. Collections are currently supported on personal panels.

Use `"collection_reveal": { "collection": "modules", "item": 8 }` to reveal an
immutable item index through the current filtered order. The host scrolls by the
minimum distance needed to show its whole cell, including the template's card
padding, before preparing the visible grid window. Reveals are instant, including
under reduced motion, and cancel pending wheel easing. Each request is consumed
once; wheel scrolling then stays under pointer control. Sending the same request
again reveals the item again. Unknown collections and out-of-range item indexes
reject the patch atomically. An admitted item excluded by the current filter is
ignored and counted in one warning for that request.

```json
{
  "controls": [{ "id": "enabled", "value": true }],
  "surface": "main",
  "bindings": { "#filter": "" },
  "capture_key": false,
  "motion": true
}
```

All fields are optional. `controls` updates only existing identifiers: toggles
take a Boolean `value`, sliders a finite in-range `value`, keybinds `key` and/or
`capturing`, and choices a valid `index`. Any control can update its `label`.
`control_values` provides compact `[control_index, value]` pairs: Boolean toggles,
numeric sliders, unsigned choice indexes and string text values. Indexes refer to the selected route's
validated control order. A control cannot appear twice across either edit list.
Omitted controls retain their values. `motion` changes the retained animation
preference without replacing the prepared route or resetting scroll and input.
Omitting it, or supplying `null`, preserves the preference across route changes.
`surface` selects a prepared route;
`bindings`, when supplied, fully replaces custom scalars and starts any declared
transitions toward the new values, while the host continues
to generate `#row_N` facts from the controls. Invalid batches change nothing, and
valid batches commit only after a successful callback. Same-route updates retain
the catalog, scroll offset and captured drag. The legacy `set-content` contract
remains available without a bundle. A replacement schema must match the bundle's
declared control identities and kinds before any prepared route can be selected.

For local probe diagnostics, `CINNABAR_MOD_BACKTRACE=1` enables guest backtraces.
Normal execution keeps them disabled and uses the same fuel and epoch limits.

`cinnabar_rounded_rectangle` nodes take `radius` (GUI pixels, default 6) and `color`.
Optional `color2` fades the fill linearly from `color` along `gradient_direction`
(`vertical`, the default, or `horizontal`). Optional `border_color` draws an inside
border `border_width` GUI pixels wide (default 1). The fill stops at the border, so
translucent fills do not paint underneath it. Edge coverage spans one physical pixel at
every display density, including subpixel borders and square inner corners.
Coverage is centered on the authored outline; layout and hit bounds stay fixed.

`cinnabar_glass` draws a frosted panel: the world behind its rounded outline blurred and
filtered like CSS `backdrop-filter`, then a tint, border and top highlight above it:

```json
{"type":"custom","renderer":"cinnabar_glass","size":[300,200],"radius":13,
 "blur":13,"saturation":1.12,"color":[0.04,0.07,0.11,0.78],
 "border_color":[0.8,0.88,1,0.1],"highlight_color":[0.94,0.96,1,0.14]}
```

`blur` is a Gaussian standard deviation in GUI pixels within 0–32 (default 0), and
`saturation` and `brightness` are CSS `saturate()` and `brightness()` factors within
0–2 (default 1), applied to sRGB-encoded values. `radius`, `color`, `border_color` and
`border_width` match `cinnabar_rounded_rectangle`; `color` defaults to transparent.
`highlight_color` draws a `highlight_width` GUI-pixel line (default 1) along the inside of
the top edge that thins out down each top corner, like CSS `inset 0 1px 0`. Literal values
outside their bounds are rejected; values bound through `#blur`, `#saturation`,
`#brightness`, `#color`, `#border_color`, `#highlight_color` or `#radius` win over the
authored ones and are clamped, so transitions can animate them. The node's `#alpha` fades
the backdrop with the tint. Under `cinnabar_transform` the panel, its blur and its
highlight scale and rotate with it, and the backdrop is sampled where the panel is drawn.

Surface bindings named `#world_blur` (GUI pixels, 0–32), `#world_brightness` and
`#world_saturation` (0–2) blur and filter the whole world behind the open personal panel or
HUD editor; other values are rejected. Declare transitions on them to ease the world in
and out, as the panel opens and, with `exit`, as it closes:

```json
"bindings": { "#world_blur": 3, "#world_brightness": 0.62 },
"transitions": {
  "#world_blur": { "duration": 0.25, "easing": [0.16, 1, 0.3, 1], "from": 0, "exit": 0 },
  "#world_brightness": { "duration": 0.25, "from": 1, "exit": 1 }
}
```

Glass sees the world through this backdrop: its blur adds in quadrature and its factors
multiply. Both act on the 3-D scene only. HUD elements, other UI beneath a panel and
nested glass are not blurred and stay sharp through the tint, as the mockup's HUD layer
does. Only extension surfaces draw glass or a world backdrop; resource packs and server
UI cannot name the renderer.

The client blurs the world once per frame, and only while a glass panel or world
backdrop is published: it downsamples the post-processed scene into a half-resolution
dual-Kawase pyramid of up to eight levels, then copies the scene into the next frame
target while each backdrop shape samples the level its blur needs. Retained UI layers
never hold world pixels, so a moving world redraws no UI. Without glass no pass runs and
no pyramid stays allocated. `CINNABAR_GPU_UI=1` reports the passes as `gpu_ui_backdrop`.

`cinnabar_vector_path` draws caller-authored geometry without registering product
icons in the runtime. `view_box` is `[x, y, width, height]`; `paths` contains arrays
of finite `[x, y]` points in that coordinate space. Without `stroke_width`, closed
contours use even-odd filling, including holes; fills require `closed: true`.
With a positive `stroke_width` in view-box units,
paths form continuous strokes with rounded joins and caps; `closed` defaults to
false for strokes and true for fills. For example:

```json
{"type":"custom","renderer":"cinnabar_vector_path","size":[16,16],
 "view_box":[0,0,16,16],"paths":[[[3,3],[8,13],[13,3]]],
 "stroke_width":1.5,"color":[1,1,1,1]}
```

The renderer accepts at most 64 contours and 2,048 points. It covers round caps
and joins beyond the view box, keeping the edge band one physical pixel wide.
The half-pixel mesh grid is bounded to 256 cells per axis, eight million
sample/segment operations, and 32,768 expanded indices per path. Over-budget
paths draw nothing; they are never silently reduced to a coarser grid. An
eight-MiB cache retains hot geometry. Path data belongs to the caller's surface.
Coplanar coverage cells share a quad, preserving the antialiased edge samples.
Rectangle searches have a separate work bound. Fully clipped vector artwork
skips tessellation; stroke caps and artwork outside its layout box keep their coverage.

Personal-panel surfaces support JSON-UI scroll views, including wheel input, track
presses and scrollbar-box dragging. Wheel input stays within the view under the
pointer and uses that view's `scroll_speed`, independently of vanilla's shared
sensitivity latch. Closing the panel or losing focus releases scrollbar capture. Changes
to content bounds clamp the scroll offset to the visible range. Track mappings
use `mod.scroll_track` with the view's `scrollbar_track_button` set to that action;
these presses are consumed by the host without invoking an extension control.

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
and normal attack transactions. The client caps the request at the current game
mode's attack reach and the input mode's pick range, so an override can only
shorten either limit. The server still decides whether a hit is valid.
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
  cargo run -p cinnabar --locked
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

`camera.set-hurt-camera-style` (camera grant) renews a temporary damage-camera
style after each successful callback. Intensity must be finite and within
`0..=render_api::MAX_HURT_CAMERA_INTENSITY`; zero disables damage shake. `old-tilt`
uses a fixed tilt axis instead of the observed damage direction. Death rotation,
view bob, aim and saved preferences stay independent. This presentation override
also applies while menus own input. `none`, omission, a trap or unload restores the
player's current preferences. In a mod set the earliest publisher wins.

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
one; each panel can be selected with its own toggle key (load order resolves shared keys);
labels join with ` | `; commands and cues keep load order. Render passes merge by name with
the earliest mod keeping a contested name, and passes
and each primitive kind fill the single-mod budgets in load order. Each mod polls
every mod's previous-frame cues. The set takes precedence over `CINNABAR_MOD_COMPONENT`;
the registration watcher manages independently enabled components ahead of the environment set.

## Attach a local component to a running client

Every native build watches `local-mod.json` in `InstallLayout.user_config_root`
(on an installed Windows client, `%LOCALAPPDATA%/Cinnabar/`). Settings › Mods owns an
ordered list and writes it atomically:

```json
{
  "version": 2,
  "request_id": "local-request-1",
  "mods": [
    {"component": "C:/Local/camera.wasm", "enabled": true,
     "font": "C:/Local/panel.ttf", "ui": "C:/Local/panel-ui.json",
     "grants": {"camera": true, "controls": true}, "reload": 0},
    {"component": "C:/Local/hud.wasm", "enabled": true,
     "grants": {"hud": true}, "reload": 0}
  ]
}
```

The original version-1 single selection is read losslessly and written as version 2
on the next Settings command. Disabled entries retain their permissions until Remove.
The file is bounded to 64 KiB, paths must be absolute, duplicate identities are rejected,
and entries are bounded by `mod_api::MAX_LOADED_MODS`. Settings companions derive from
component paths using the host's existing companion naming rule.

Import validates a `.wasm` component without executing it and never overwrites an
existing filename. Enable adds a card to Active; Disable and Remove target that card's
identity. Move up/down changes precedence without restarting guests. Registered mods
run before environment-selected mods. Higher Active entries publishing camera or time output own that authority, and contested HUD card IDs refuse lower publications and count each
refused import. Distinct HUD slots are merged within the shared presentation limit.
Per-instance fuel, epoch deadlines, memory, host-copy and source limits remain in force.
Regular frame callbacks share `mod_api::TOTAL_MOD_FRAME_FUEL`. Events retain their
existing `CALLBACK_FUEL` allowance, and session-loading callbacks retain `LOAD_FUEL`.
Separate bounded lanes admit one event and one loading callback per frame across all mods.
A callback needing an unavailable lane defers rather than receiving a smaller guest limit.
Latest layout, changed session sources and view-closure notifications remain queued and
coalesced until delivery. Each declared edit control also retains one latest bounded text value
for its current screen and view. Screen changes, view closure and host replacement discard
old edits; key, click and wheel edges are not replayed. Frame-local camera and item-use
policies reset even when a callback defers; retained presentation follows its existing contract.

An authority revocation is distinct from a guest quarantine: restoring superseded grants
rebuilds a revoked guest, while unrelated Settings changes preserve a trapped guest's
quarantine and installed identity.

Reload and Reload all work from Settings while in a world. Candidates re-read sources
through the bounded admission path and initialize off-frame. An invalid reload keeps
the previous instance, with its failure displayed on that card. A successful reload
resets guest state and preserves current companion preferences. Hot replacement uses
the same path. Permission changes reload only the affected mod; revoked authority is
released before admission completes. A guest trap quarantines only that guest.

Cards show how many requested permissions are allowed and warn about allowed
capabilities that can change gameplay. Customize opens a compact requested-first
grid; Show all permissions reveals the remaining capabilities. Hover or focus a
permission to read its description. Allow requested grants exactly the declared
requests, while Revoke all clears the card's grants. Both save once through the
same path as individual toggles. Undeclared components offer Customize only.
Permission names and gameplay warnings come from `ModGrants`. Optional grants
default off. A binary component may declare its requested grants as JSON in the
optional top-level custom section `cinnabar:grants`; declarations never grant authority.
For example, `{"camera":true,"controls":true,"commands":["ability"]}` marks those
requests. Command permissions enable only the declared command names; undeclared
components cannot gain unrestricted command authority through the toggle.

`local-mod.status.json` uses version 2, reports the client PID and the
`independent_registration` capability, and records per-component `loaded`, `error` or `reload-error` outcomes
for the list's request ID. Source polling, compilation, font rasterization, UI preparation,
status writes and final settings retirement run off-frame. Remove first clears the entry's
grants and metadata, waits for its detached host's final settings writes, then deletes its
component and unshared companion. Environment-loaded settings writers also protect their companions. A changed list or watcher generation cancels deletion.
Resolved directory aliases agree on selection and companion ownership. The guest has
no filesystem or process access.

## Verification and limits

```sh
cargo test -p mod-host --locked
cargo test -p cinnabar --lib modding --locked
cargo test -p cinnabar --lib mod_hud --locked
```

To capture the real compiled sample through the existing offline renderer:

```sh
mkdir -p /tmp/cinnabar-mod-frames
CINNABAR_FORM_SNAPSHOT_DIR=/tmp/cinnabar-mod-frames \
CINNABAR_MOD_SNAPSHOT_COMPONENT=/tmp/cinnabar-hello.wasm \
  cargo test -p cinnabar --lib mod_spike_snapshot_with_real_carrier --locked -- --nocapture
```

This writes before, initial-label and keybind PNGs without starting a network
session. The test requires the real carrier when a snapshot directory is set.

Measure the real CPU UI build with zero mods and with the sample loaded:

```sh
CINNABAR_MOD_SNAPSHOT_COMPONENT=/tmp/cinnabar-hello.wasm \
  cargo test -p cinnabar --lib mod_spike_offline_frame_overhead --locked -- --ignored --nocapture
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
templates and `textures/`. `CINNABAR_MOD_PACKAGE=<dir>` loads it and takes precedence over `CINNABAR_MOD_COMPONENT`. A set entry
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
grant (`screen`, `items`, `recipes`, `keys`, `settings`, `inventory`). `settings` gives the
`settings` interface its companion file. `inventory` also needs `CINNABAR_MOD_INVENTORY=1` and
grants the `inventory` interface: `creative-take` (Creative only, an item of the creative
content) and `fill-recipe` (a crafting recipe, once, onto the cursor), each the request the
creative catalog or the recipe book makes, at most four per callback. On reload, the new
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
modal's (server-experiences.md, Modal screens); `focus-text` selects an edit box,
`held-modifiers` reports the modifiers held with the event being delivered and
`elapsed-seconds` is a monotonic clock to animate by. Rows bind
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

This package is the one definition client parts use too: the `server-bundle` world imports
`cinnabar:session/items@0.1.0` from this same file (as a further WIT path, the way mod-api
reads `client/deps/server-experience`) under the `items` permission.

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
(the manifest), `cargo test -p mod-ui --lib mod_screens` (clipping, the lifted held stack, hit
filtering) and `cargo test -p client-ui --lib mod_surfaces session_data` (session data). The overlay has not been checked on a
rendered frame yet; see `plan.md`.

## Loaded block highlights

The separate `block_highlights` grant (`CINNABAR_MOD_BLOCK_HIGHLIGHTS=1`) permits
`render.set-block-highlights`. User consent is sufficient in local and remote
sessions, including servers without drawing negotiation.
A retained specification names up to
`mod_api::MAX_BLOCK_HIGHLIGHT_IDENTIFIERS` canonical block identifiers, a bounded
camera-relative range, and linear RGBA colour. The host scans only loaded primary
block layers, caches palettes and subchunk identities, and draws full unit cubes
through terrain without changing world or packet state. Results share the
`mod_api::MAX_BLOCK_HIGHLIGHTS` nearest-block budget; the earliest active mod wins.
`none`, unload, reload, or a trap clears the overlay. Output commits only after a
successful callback; repeated unchanged input rebuilds no geometry.
Packet `render-frame` scenes also accept an optional `outline` on each block:
`{"position":[1,64,2],"colour":[0,1,1,0.3],"outline":{"colour":[0,1,1,0.85],"width":1.5}}`.
The width is in physical pixels, greater than zero and at most 8. Both colours use
independent linear RGBA; a zero fill alpha gives an outline-only block. These
edges share the block-highlight budget and do not consume retained box slots.
Guests must negotiate `native_render::BLOCK_OUTLINES` before sending `outline`;
older hosts accept only the existing position and colour fields. The optional
bit requires the same `block_highlights` consent, not another permission.
Face-adjacent highlights form a continuous surface. Shared faces and coplanar
outline seams are removed; isolated blocks and exterior creases retain their edges.

Packet guests negotiating `native_render::ACTOR_ANCHORS` may set a line's
`camera_start: true` and `entity_end: {"runtime_id":42,"offset":[0,1.62,0]}`.
The host resolves these from the same interpolated actor body and camera used
for rendering, dropping the line when its target is unavailable. Offsets use
world axes. This bit requires entity observation and highlight consent in
addition to the existing world drawing grants.
An entity group selecting exactly one actor may also supply `position:[x,y,z]`
to place its current animated mesh at those feet. `style.fill:[r,g,b,a]` adds
translucent silhouette coverage independently of the outline and halo. A hidden
local body is prepared only for requested coverage and stays out of main passes.
Omit all these optional fields when the host does not advertise the bit.

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
in the component's registration/set grants. Plain cards and crosshairs do not read player
data, change input, or send packets. Native source bindings additionally require their
observation grants. Existing `set-label` remains available without
this grant. All HUD writes share an eight-call callback budget.

`hud.update-content(json)` retains unchanged cards. Its bounded `HudUpdate` object
accepts `cards` to insert or replace by ID, `remove` with card IDs to delete, and
optional `presentation` using the ordinary HUD schema with an empty `cards` list.
Presentation replaces metadata while retaining cards. Duplicate IDs, invalid
cards, or a combined result over the HUD limits reject the entire update. Updates
compose within a callback and publish only after it returns successfully.

`cell_changes` accepts sparse paint/text edits to an existing rectangular cell:
`{"card":"keys","index":0,"color":[1,0,0,1],"background":[0,0,0,0.5]}`.
Optional `label` and `value` replace its text. Geometry stays unchanged. Unknown or
duplicate targets and invalid resulting colors/text reject the whole transaction.

A card may bind native rows with `source`, for example
`{"kind":{"kind":"inventory","hotbar":true},"limit":36,"color":[1,1,1,1],"brackets":false}`.
Other kinds are `armor` (`held`, `percent`, `empty`), `effects` (`icons`), and
`sidebar` (`scores`), and `item_count` (`label`, up to sixteen `items` identifiers,
and `blocks`). An optional `potion_effect` names a known effect and additionally
counts native potion forms whose auxiliary type grants it. This uses the native
item and potion facts, including negotiated identifiers, rather than guessed
network IDs or auxiliary values. Counts include known inventory and offhand stacks
matching an identifier, the block-item flag, or the selected potion effect, and
display `--` without a snapshot.
These require `player-state`. Their positive `limit` reserves
rows against the combined HUD budget before any facts arrive. Native bindings update
from current admitted session facts even when a guest frame is deferred, and clear
unavailable observations. They retain ordinary card geometry and editor behavior.
An editor preview gives an empty bound card its `editor_label` (or ID) as a title,
so widgets without current items or effects remain selectable. Gameplay cards keep
their actual empty content.
An effect label uses the native language table; a row's optional `localized: true`
also treats its label as a native language key, while its value remains literal.

The `clock` source takes `military` and `am_pm`, reserves exactly one row, and
uses the OS local wall clock. It requires HUD authority and does not expose the
clock to WASM or depend on Minecraft's day/night cycle. It updates natively when
the local minute changes, including when the guest callback is deferred.

The `totem_uses` source takes `flip` and `count_color`, reserves exactly one row,
and requires `player-state`. It displays confirmed local activations from the
current native counting interval, independently of inventory quantities. An
unavailable interval displays `--`; death, retired player lifetimes, dimensions
and disabled intervals cannot contribute stale counts.
Set `actor_style.totems` to start the interval and annotate eligible native player
names. This additionally requires `players`. Its `show_self`, `show_tab`, and
`show_nametag` flags select surfaces; `separate_nametag` selects a separate world
name line. `render_first` places the annotation before the name, `show_prefix`
adds a separately tinted separator, and `prefix_color` accepts straight RGBA.
`flip` displays positive instead of negative counts, and `count_color` selects
the native count palette. These presentation conventions remain provisional;
they do not establish reference-client parity. Annotations retain native
visibility, depth, font-atlas and record limits and leave server name text intact.

A single plain-value card may set `text_plate` with `brackets`, `bracket_color`,
`background`, `border_color`, `border_width` (0–8), and `radius` (0–16, default 0). Colors are straight RGBA.
Optional `width` (1–512) and `height` (1–128) fix the background's size; omitting
either fits that dimension to native font measurements and padding. Text stays
centered and retains its row color and the card's shadow. Brackets have their
own tint. The editor uses the same measured bounds as gameplay. Text plates
cannot mix titles, icons, cells or other row layouts; their rows contain one
plain value with an empty label. They may use the native Clock source or an
`item_count` or `totem_uses` source with an empty label, `limit: 1`, and `brackets: false`.

Presentation may set `actor_health` with `players`, `mobs`, `range` (0–128), and
`color`. Players require `players` and mobs require `entities`. This reserves sixteen
world-label slots and projects only observed current-session health, within range.
It never estimates missing health or reveals actors marked invisible.
`actor_bounds` accepts the same selection, range, and color fields. It requires
`render` plus the selected actor grants and draws native, depth-tested outlines
only during focused gameplay. Outlines share the existing ribbon budget; authored
guest primitives keep their slots when that budget is full.

HUD data may set `hide_effect_icons: true` to replace the ordinary status-effect
icons. This hides their visibility binding and native renderer without changing
the player's effects. It remains active with empty `cards`; clearing the HUD,
omitting the flag or setting it to false restores the ordinary icons.

Card data contains `cards`, each with a unique `id`, `rows`, and optional
`title`, `anchor`, `offset`, `scale`, `position`, `background_opacity`, and
`background_radius` (0–16 GUI pixels, default 3). Background radii scale with the card. Anchors are
`top_left`, `top_center`, `top_right`, `left`, `center`, `right`, `bottom_left`,
`bottom_center`, and `bottom_right`. Offsets are finite GUI pixels measured inward
from anchored edges; centered axes use signed offsets toward the right or bottom.
Anchored cards clamp to the available viewport without changing their saved offsets.
An explicit `position` keeps its existing normalized-travel behavior and takes precedence.
Scale is 0.25–5 and scales card text, icons,
and geometry together. Each row has `label`, `value`, and optional `item`
(resource identifier), `metadata`, `effect_id` (canonical Bedrock effect icon),
`progress` (0–1), and `color` (RGBA 0–1). Item and effect icons are mutually exclusive;
unknown effect IDs leave the icon empty rather than guessing.

A card may instead supply `compass` with `rows: []`, width 168–448, a current
`session` and `dimension`, RGBA `color` and `marker_color`, `show_marker`,
`show_value`, and up to sixteen `waypoints` containing `position` and RGBA `color`.
This requires `player-state`; it cannot be mixed with rows, cells, or a row source.
The native host supplies the pose only while both observations match that stamp.
Turns update retained tick and marker bindings even when the guest callback is deferred.
Missing observations hide bearings and show `--` instead of a guessed heading.
At most four compass cards may share a HUD. `top_center` keeps their center aligned
through viewport or scale changes; a saved drag position still takes precedence.
Item art follows the current session's resource-pack icons and normal item atlas;
unknown item identities draw no substituted item. Optional `row_layout` selects
`standard` (default, inline text with a left icon), `stacked_text` (label above
value beside a left icon), `icon_right` (inline text before a right icon), or
`grid` (square inventory cells, with columns derived from width and row height).
`width`, `row_height`, and `icon_size` use unscaled GUI pixels and default to
148, 20, and 16. Width is 48–512, row height is 12–64, and icon size is 4–48;
icons must fit the row height and the width minus 12 pixels of padding.
Optional `text_scale` (default 1, bounded to 0.5–2) multiplies the row label and
value fonts independently of icons and card dimensions. Optional `shadow` defaults
to true and controls row text shadows. Inline text bands expand
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

Instead of rows, a card may provide `cells` with `rows: []`. Each cell has
`rect: [x, y, width, height]` in unscaled GUI pixels, `label`, an optional smaller
second-line `value`, RGBA `background` and `color`, and `shadow` (default false).
Cell cards allow widths of 1–512 pixels and do not reserve icon padding.
Rectangles must have positive finite sizes, stay within the card width, and end
within 1024 pixels vertically. Cells share the row count and text limits; rows
and cells cannot be mixed. Cell-card height is the largest rectangle bottom plus any title header;
card scale, placement, editor dragging, resizing and snapping use that geometry.
Cells have square corners, centered text and independent background colors;
label/color updates retain the catalog while geometry changes rebuild it.

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
The settings panel accepts at most 128 controls; existing category and page
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
Optional `editor_style` styles the host-owned chrome: `outline` and `selected`
card outlines, resize `handle`, `handle_hover` and `handle_pressed` colors, the
`handle_radius` of the five-pixel handle mark (0–4), alignment `guide`, the
built-in `grid`, and the distance `measurement` pills. Colors are RGBA from zero to one; omitted fields keep the
built-in appearance. `font` selects `native` or `sans`, and `text_scale` (0.25–0.8,
default 0.8) controls editor labels independently of widget scale. Distance
pills retain their compact 0.45 text scale in the selected font. Distance
captions move clear of visible widgets and hide when there is no free space.
`selection_caption: true` draws one measured name-and-scale tag for the selected
widget, using `selected` as its background and `caption_text` for its RGBA text.
It stays the same size as the widget is resized; the inspector and distance
captions avoid its bounds. The option defaults to false for existing surfaces.
`top_clearance` (0–128 GUI pixels, default zero) reserves an authored toolbar band
from selection and distance captions. Measurements hide if no nearby free space remains.

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
handles. Dragging a handle scales the card uniformly from 0.25 to 5, keeps its
opposite corner fixed when space permits, and clamps its placement to the viewport.
Use `hud.read-editor-layout()` to read `saved`, `reset`, and `placements` containing
ID, normalized position, and scale. It shares result consumption with the existing
`hud.read-editor-result()`; the legacy position-only API remains unchanged.

Completed moves, resizes, and arrow nudges choose one of nine anchors from the
viewport third containing the card's center. The stored GUI-pixel offset preserves
the current origin and its distance from that anchor on later viewports and GUI scales.
Use `hud.read-editor-anchored-layout()` for `saved`, `reset`, and placements with
`id`, `anchor`, `offset`, and `scale`. Republish these cards without `position` to
retain anchoring. Both older reads still return normalized positions at the current
viewport and share the same callback lifetime and result consumption.

An editor request may opt into `autosave: true`. Completed pointer drags and resizes, arrow
nudges, and Reset then return incremental saved results while the editor stays
open. A held or interrupted drag remains a draft. Escape closes the personal
panel in this mode; completed placements have already been delivered, and the
unfinished gesture is discarded. Omitting `autosave` preserves Save/Cancel.
Built-in autosave chrome shows Done and describes immediate saving. Its widget name
appears in one contextual footer caption when hovered or selected, so nearby widgets
do not overlap each other's labels. Custom editor surfaces retain authored chrome
and their optional labels above cards.

The optional editor `surface` has the same bounded document, bindings and
transitions as a personal panel surface. Its client-authored chrome replaces the built-in shade,
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

### Bounded actor reads and observed maximum health

`player-state.read-actors-limited` returns a caller-bounded prefix of the same
authorized observations (1 through `MAX_OBSERVED_ACTORS`). It preserves existing
filters and session fences, allowing guests to bound canonical-ABI copying work.

Each bounded result contains the existing `actor` record and optional `max-health`
from the replicated health attribute. Missing attributes remain absent; no default
maximum is inferred. The original `read-actors` import and actor record stay compatible
with previously built components.

## Trusted packet mods

Settings → Mods → Import mod accepts raw WASI packet reactors up to the packet
host's 256 MiB source limit. Ordinary component mods retain their existing limits.
Import validates the packet ABI without starting the guest; permissions stay off
by default. The `packet-mod` world in `crates/mod-api/wit/extension.wit` owns the
session identity, startup packs, batch replacement and timed output contract.

Grant Session packets and, for WASI reactors, WASI runtime before enabling the
mod. One packet mod can be selected at a time. The prepared source starts in a
fresh instance on the next server join. Disabling it or revoking grants retires
the current authority immediately; reloads select the next session's source.
Compilation and guest execution run off the frame thread. Traps, invalid output
and expired deadlines quarantine the instance into ordinary packet forwarding.

The host retains one admitted compiled program in memory, keyed by the source
and exact grants. Its native image is bounded by the source byte limit; compiled
native artifacts are never read from disk. WASI provides clocks, randomness and
bounded captured output, without files, inherited environment or network access.

Server batches through spawn use the join deadline, including startup registries
that arrive after the StartGame handoff. The login owner switches later batches
to the shorter play deadline after spawn. Ticks always use the play deadline.

Developer launches may select a reactor with `CINNABAR_PACKET_MOD` and add WASI
with `CINNABAR_PACKET_MOD_WASI=1`, or select a packet entry from
`CINNABAR_MOD_SET`. Settings uses the same prepared-source and relay path.

Native menus for trusted packet reactors use the optional callback and explicit
Controls grant described in [the native menu contract](../crates/mod-api/native-menu.md).
