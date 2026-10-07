# Global resource packs: references and runtime inventory

Sources used on 2026-10-01:

- `V:` is the read-only installed vanilla pack at
  `~/Downloads/cinnabar/.local/assets/bedrock-samples/v1.26.50.4/full/resource_pack/`.

## Verified user-facing rules

The effective order, low to high, is vanilla base, global resources, then packs
from the world or server. The top pack wins overlapping assets. This is explicit
in `V:texts/en_US.lang:8330`, `:8333`, and `:8335`; the global description explicitly
includes worlds the player joins. Required server packs retain this precedence.
The required setting controls admission/download consent: `:8245` says the world
owner requires downloading all packs to join, and `:8338` labels the switch as
requiring players to download optional resource packs. No source found here
establishes that this setting removes nonconflicting global resources.

The global page labels are “Global Resources” (`V:texts/en_US.lang:6428`), “My
Packs”/“Activate” (`:8299`–`:8300`), and “Active”/“Deactivate” (`:8315`–`:8316`).
Its descriptions explain priority instead of assigning invented categories.
The actual selected and available collection factories are
`V:ui/resource_packs_screen.json:3824` and `:3842`. Pack resolution and insufficient
memory messages are `V:texts/en_US.lang:8375`–`:8377`.

Vanilla disallows changing resource packs while playing a world:
`V:texts/en_US.lang:8321`. Cinnabar's live apply is an intentional extension, not a
closed vanilla parity gate. Imports contain user assets stored in the install
layout, never committed assets.

## Vanilla rules

| Rule | Behaviour |
| --- | --- |
| Composite stack | Compose separate stacks, a conditional stack and lower fallbacks. |
| Resource lookup | Resolve the first matching resource, checking its selected subpack before the pack root. |
| Unavailable subpack | Use root resources when the server's selected name has no declared subpack; retain the selection as metadata. |
| Unchanged global stack | Compare pack identities and avoid applying an unchanged stack. |
| Manifest | Read UUID, minimum engine version, subpacks and `memory_tier`; serialize and deserialize subpack counts. |

Internal vector direction must not be confused with the user-visible bottom-to-top
labels. Cinnabar's `LayeredPackView` stores low-to-high and looks up in reverse.
Current force-pack branch behavior remains incompletely verified. The shipped
pack's explicit UI descriptions establish the current user-visible stack order.

## Existing subscribers and reload dependencies

| Subscriber | Existing path and behavior | Replacement dependencies |
| --- | --- | --- |
| Block atlas/materials/models | `app/src/runtime/network/block_overlay.rs`; `resource_packs.rs::session_runtime_assets`; base is the required/diagnostic carrier | Stack textures, terrain catalog, block and model definitions; preserve network IDs; change `ChunkTextureAssets` revision and dirty resident meshes |
| Item icons | `network/item_icons.rs::compile_session_icons` | Item texture catalog/raster, StartGame icon keys, custom block thumbnails; replace `UiRuntime` session icons and held-item snapshot |
| Entity/attachable/animation/controllers | `network/entity_pack.rs`, `entity_pack/collect.rs` | Actor definitions, referenced textures/models/controllers/animations; vanilla reference sidecar only includes geometry/controllers/animations, not vanilla client entity declarations |
| Entity renderer | `network/actor_publication.rs::configure_session_artwork` | Scene pack geometry plus pack textures; its original session-only refresh condition must also recognize live pack identity changes |
| JSON-UI and textures | `resource_packs.rs::collect_server_ui`, `ui_runtime/presentation/forms.rs::observe_server_ui`, `forms/server_pack.rs` | Merge layer definitions over original carrier; replace atlas and invalidate resolved screens when Arc changes |
| Sounds | `audio/server.rs::ServerSoundPack::from_view`, generation mailbox, `audio/engine.rs::poll_server` | `sounds/sound_definitions.json`, `sounds.json`, audio bytes; publish new optional sound snapshot |
| Glyph sheets | `network/glyph_sheets.rs` | `font/glyph_XX.png`, clear absent overrides to base; general font configuration/TTF replacement is not implemented by this existing reader |
| Language | `resource_packs.rs::merged_server_lang` | `texts/en_US.lang` plus startup-selected locale |
| Particles | Startup particle carrier and `app/src/particles` | Initially no server-stack reader; requires an added compiler adapter and render atlas invalidation |
| Biome colors/colormaps | Runtime world assets and stream biome tint resolution | Initially carrier-only; requires base-preserving biome overlay and tint revision refresh |
| Fog/sky | Startup atmosphere carrier, atmosphere texture/render resources | Initially carrier-only; requires fallback-preserving atmosphere overlay, detached compilation, and GPU identity refresh |

The required base carrier admission still fails closed. Optional layer failure
must retain a usable prior/base presentation and must not mutate connection or
world sequencing state. A request revision and session generation together reject
stale worker completions after edits, disconnects, or transfers.

## Font reload evidence

The default bitmap sheet is `font/default8`, with an explicit texture reload path.
Divide sheet dimensions by 16 and iterate 256 cells using row `index >> 4` and
column `index & 15`. Scan alpha for glyph widths and treat space specially.
ASCII cells can be supported directly; the extended range also requires font
remapping rules.

## Implemented reload boundaries

The worker retains the server's admitted stack and StartGame-derived compilation
inputs; global layers are composed below it. Per-family fingerprints include
file paths, bytes and precedence. They conservatively treat textures as an input
to multiple consumers because JSON definitions can reference arbitrary texture
paths. Unchanged families keep their compiled Arc identities and block assets.
They do not promise individual-texture GPU uploads.

`pack_reload` publishes revision- and session-stamped replacements, with old
requests discarded. `WorldStream::reload_resource_assets` preserves network
sequence/session state and dirties resident meshes. Existing actors are rebound
when their entity catalog changes. `actor_publication` observes pack/item and
artwork changes as well as session changes. Vanilla source texture overrides use
carrier source paths, preserve raster resolution within existing renderer safety
limits, and restore original carrier pages on removal. Their immutable base must
be captured after vanilla equipment pages are appended.

The offline world witness uses real carrier texture pages, a real WorldStream
asset swap, and the production greedy chunk mesher. Its camera projection and
rasterization are software test tooling, not a native GPU or vanilla visual
parity measurement. The large-pack timing test reports worker duration and peak
CPU update time in an offline test app; these are not native full-frame timings.

The live font subscriber now reads `font/default8.png` through the same bounded
worker decoder as Unicode sheets. Its verified printable ASCII range, U+0020
through U+007E, takes precedence over `glyph_00.png` in the ordinary bitmap-font
path. Cropping retains left padding in the bearing and advance; space advances
four GUI pixels. A malformed sheet falls through to a valid lower layer. The
extended-byte remapping of `default8` remains unimplemented; non-ASCII continues
to use Unicode sheets and the base font. This does not claim force-Unicode-mode
parity or arbitrary TrueType/OpenType pack support.

Focused tests cover highest-layer ASCII replacement, Unicode preservation,
restoration after removal, no duplicate codepoints, right-edge advance with
leading transparent columns, and malformed-image fallback. Space emits no geometry.

## Settings host integration

The host reads `MenuView.global_resources` (`Snapshot.available`, `active`,
`selection`, `busy`, and `message`) and sends
`MenuRuntime::activate(MenuAction::GlobalResources(Action::...))`. Actions include
Import, Activate, Deactivate, MoveUp, MoveDown, Settings, Subpack, and Apply.
Indexed commands carry the snapshot revision, so delayed clicks cannot operate
on a different row. The complete binding module is
`app/src/ui_runtime/presentation/forms/global_resources.rs`; the section host only
calls `bind`, `overlay`, `action`, and `slider_actions` through its existing hooks.

The underlying `GlobalPackLibrary` exposes `available`, `active`, `import`,
`activate`, `deactivate`, `move_pack`, `select_subpack`, `preview`, and
`commit_selection`. `preview` does worker validation; persistence follows successful
runtime publication. Replacing an active or outstanding-preview UUID requires
Deactivate and Apply first, to prevent replacing the archive underneath an
unacknowledged selection. This restriction is not asserted as vanilla parity.

Vanilla declares dotted controller variables such as `$button.remove` and
`$button.move_left` at `V:ui/settings_sections/general_section.json:1568` and
`:1607`; their consumers include `V:ui/resource_packs_screen.json:181` and `:2272`.
The JSON-UI variable lexer now recognizes the dot. Explicit collection defaults
supply the empty lists' section visibility and counts without fabricating rows;
the vanilla section and title controls bind these through their collections.
