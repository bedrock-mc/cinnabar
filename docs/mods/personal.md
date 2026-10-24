# Personal client mods

Personal mods run under the user's grants in local and remote worlds. Server
consent does not control these grants. For server-delivered client parts, use
[paired mods](README.md). Personal mods use [`mod-api`](../../crates/mod-api),
not the Experience SDK's client world.

## Build and load

The [hello example](../../examples/mods/hello) uses the `extension` world:

```sh
rustup target add wasm32-unknown-unknown
cargo build -p hello-mod --target wasm32-unknown-unknown --locked
cargo run -p mod-host --bin mod-host --locked -- pack \
  target/wasm32-unknown-unknown/debug/hello_mod.wasm /tmp/cinnabar-hello.wasm
cargo run -p mod-host --bin mod-host --locked -- probe /tmp/cinnabar-hello.wasm
```

Import the component in Settings → Mods, review permissions and Enable it.
Reload rereads the installed path; atomically replace that file to update the same
registration. Invalid replacements retain the previous instance. Successful reloads
reset guest memory and preserve companion settings. A guest trap quarantines that
mod and clears its outputs. Reload a valid changed component to recover.

For explicit development selection, set `CINNABAR_MOD_COMPONENT` to an absolute
component path. `CINNABAR_MOD_PACKAGE` takes precedence, and `CINNABAR_MOD_SET` can
select an ordered JSON list. Registered enabled mods run before environment mods.
The client still needs its normal runtime carriers.

## API and grants

[`extension.wit`](../../crates/mod-api/wit/extension.wit) defines the low-level
world. The `player-mod` world supports package screens and session catalogs;
implement [`PlayerMod`](../../crates/mod-api/src/guest.rs) and call
`export_player_mod!`. Init and frame are required; event callbacks default to no-ops.
Input edges expire, while latest layout/session changes wait for a callback lane.
Frame, event and load work have separate budgets.

Optional grants default off in [`ModGrants`](../../crates/mod-host/src/grants.rs).
A component may request them in top-level `cinnabar:grants` JSON; requests grant
nothing by themselves. Commands authorize only the requested names. Settings can
revoke individual grants or all grants; permission changes reload that mod.
Explicit environment-selected components get the environment grant and opt into
other grants with the [development switches](../../crates/client-extensions/src/modding/grants.rs).

| Capability group | Contract source |
| --- | --- |
| Read-only inventory/effects, poses and physics | `player-state` and `gameplay` in extension WIT; unavailable data is optional, not inferred |
| Camera, item use, movement and attack requests | WIT `gameplay`; frame-local policies expire even when a callback is deferred |
| Controls, settings and personal panels | WIT `input`, `settings`, `panel`; settings use a bounded companion file |
| HUD cards and crosshair | WIT `hud`; retained output clears on quarantine/reload/unload |
| Time/weather and local menu branding | WIT `environment`; changes presentation, not server state |
| World primitives and post passes | WIT `render`; [render example](../../examples/mods/render-sample) |
| Retained world drawing | [drawing WIT](../../crates/mod-api/wit/deps/world-draw/drawing.wit); separate material/depth/post/through-wall grants |
| Entity masks | [Mask reference](../mod-entity-masks.md) |
| Nearby terrain | [Terrain reference](../mod-terrain-api.md) |
| Block highlights and fullbright | `render` highlights and `environment.set-fullbright`; local retained presentation |

Callbacks publish only after success. Session changes, reload, revocation and traps
invalidate scoped handles and outputs. Check typed errors rather than silently
assuming an import succeeded. Shared values and ceilings live in
[`mod-api`](../../crates/mod-api/src/lib.rs),
[drawing limits](../../crates/mod-api/src/drawing.rs) and
[host limits](../../crates/mod-host/src/lib.rs).

## Packages and screens

A package directory contains `mod.toml`, `mod.wasm`, `ui/` templates and `textures/`.
Use [`screen-probe`](../../examples/mods/screen-probe) as the complete example.
The manifest indexes declared files with SHA-256 and names permissions, templates,
actions and keys. IDs and templates must belong to its namespace. The build script
uses `experience_sdk::declarations::generate_mod`, then the guest includes generated
constants with `experience_sdk::include_declarations!()`.

The manifest request and loader grant must both allow each package capability.
Inventory requests also need the explicit inventory development switch. Reload
narrows against the original authorization, so editing a manifest cannot add grants.
The settings companion is `<package>.settings.json`, outside the indexed package.

An overlay draws beside the active container, clipped away from its panels and
excluded areas. A view hides the container's presentation without closing its
server container. Escape returns to the container; closing the container closes
both. `screen_changed` reports layout in GUI units, and `view_closed` reports a
host-closed view. Declared actions, keys, text, wheel events and data revisions
arrive through `PlayerMod` callbacks. UI values and collections use the same
[tagged bindings](../server-experiences.md#modal-screens) as client parts.

## Single-file resources

Only binary components support embedded resources. Put each in one top-level
custom section, not inside a nested core module. Resource budgets are separate
from executable bytes; directory packages retain their own lifecycle.

| Section | Payload/grant |
| --- | --- |
| `cinnabar:extension/panel-bundle@1` | Panel JSON; `controls` |
| `cinnabar:extension/font@1` | Outline font; `controls` |
| `cinnabar:extension/menu-logo@1` | PNG; `environment` |
| `cinnabar:extension/menu-title@1` | Single-line UTF-8 heading; `environment` |
| `cinnabar:extension/notices@1` | UTF-8 copyright/license text |
| `cinnabar:extension/icon@1` | PNG Mods-tab icon |

[SDK constants](../../crates/mod-api/src/lib.rs) define byte/image limits.
Duplicate, nested, malformed or oversized presentation resources are rejected;
invalid icon/metadata affects only the card. `CINNABAR_MOD_FONT` and
`CINNABAR_MOD_UI` sidecars override embedded font/panel data. Mod cards read standard
component name, description, version, authors and licenses metadata.

## Optional GitHub update checks

A component can append top-level UTF-8 `cinnabar:extension/update-repository@1`
containing its public GitHub repository URL for release notifications. For automatic
installation, use `cinnabar:extension/update-url@1` containing an exact latest-release
WASM asset URL of the form
`https://github.com/owner/repo/releases/latest/download/mod.wasm`.
A download URL takes precedence over a repository-only declaration.

Updates require standard SemVer version metadata and the same declaration in the
replacement. Drafts, prereleases and older/equal versions are skipped. Downloads
require GitHub's SHA-256 digest, compatible components and matching versions.
Only unchanged files in the managed mods directory are replaced automatically;
external files can receive notifications. Companion settings and grants survive.
The Mods tab's Update action works when automatic installation is disabled.
Settings → Updates controls mod checks and automatic installation separately.
Mods without either declaration make no update requests.

## Trusted packet mods

`CINNABAR_PACKET_MOD` selects a packet mod on its own; the `packets` grant exposes
session packets and pack insertion through the private core relay. A packet mod
cannot share a registration with gameplay/package mod authority. Grant changes
revoke current authority; reload selects the next session's source. Optional
`CINNABAR_PACKET_MOD_WASI=1` adds clocks, randomness and captured output, never
files or network. See [packet WIT](../../crates/mod-api/wit/extension.wit) for callbacks.
