# UI and screen status

[Roadmap](../../plan.md)

These topic notes retain the recorded implementation status and limitations.
An implementation entry does not close a parity or performance gate. Earlier
test results describe the recorded revision; revalidate changed code. Update the
owning section when its remaining work changes instead of appending a PR diary.

## Server-authored chat content

Server message subtrees retain their parser bindings, custom definitions and hover controls
inside the built-in chat placement. View-bound text survives retained factory refreshes.
Open chat supplies hover feedback through non-consuming input panels and positions native
tooltips at the pointer; zero-sized tooltip controls paint outside their layout rectangles.
Regression fixtures cover refreshes, changed captions, hover entry/exit and display scaling.
The reported pack was exercised offline with anonymous message text. Native version-matched
input and command-edit acceptance and broader JSON-UI parity remain incomplete; no parity
or performance gate closes here.


## Held input across HUD toggles

Display-setting replacements retain the controls revision when controls are unchanged.
Gameplay input uses that revision, so hiding and showing the HUD or paper doll preserves
held movement, jump and attack. Changed controls still release held input until it is
pressed again. Regression tests are written but unrun; compilation and live acceptance
remain pending. No parity gate closes here.

## Creative search under server UI packs

Unchanged inventory screens keep recipe-grid virtualization even when a server supplies
unrelated UI definitions. Scoped grid and collection-factory templates are compared to the
base catalog; modified screens and runtime factory inputs retain the conservative path.
Incomplete: remaining placeholder layout work still costs about 21 ms per scroll in the
2,048-item Windows offline fixture, down from about 203 ms before this change. Live typing,
icons and hardware frame pacing remain unverified; no high-refresh performance gate closes.


## Settings disk writes

Settings edits apply to runtime authorities immediately. A single background writer keeps
only the newest pending snapshot. It starts a 100 ms batching window with the first edit,
without extending that window during a drag. Storage can delay completion. Failed writes
retain the latest edits and retry after one second. Orderly shutdown flushes pending edits before releasing the writer.
A crash can lose edits still in the batching window or waiting on storage. Native slider-drag
frame timings remain unverified; this change does not close a hardware performance gate.


## UI text and presentation failure recovery

- Wrapped presentation text keeps an indivisible glyph wider than its control, with
  existing line breaks and clipping. Newly shaped overflow layouts are counted and
  logged with bounded frequency. Strict measurement requests still reject oversized ink.
- Rejected runtime presentation frames count, log and skip publication without ending
  the network session; a later valid frame can publish. Startup asset validation remains
  fail-fast. Fitting status labels keep their width checks and ellipsis policy.
- Exact-version native overflow behavior remains incomplete pending a matching native
  control witness. The original reported text is unknown. No parity gate closes.


## Anvil screen

- The anvil predicts its result and cost: repair materials restore a
  quarter of the durability each for a level each, a second damaged copy adds its remaining
  durability plus 12% for two levels, and enchantments merge by the native registry's slots,
  caps, rarity multipliers and exclusivity. A second item that changes nothing clears the
  result; renaming adds a level, and a rename alone stays below 40 levels and keeps the
  prior-work penalty, which other takes double plus one.
- The name box prefills from each new target. The result slot and its tooltip show the
  server's output when supplied, otherwise the prediction. For predictions, the cost
  line turns green or red by the player's levels or shows "Too Expensive!", and the cross
  marks a target that makes nothing. Taking a prediction sends that result and consumes the
  materials it uses; server outputs keep their authoritative take path. Predictions retain
  complete enchantment levels and lists and decline unsupported enchantment fields.
- Predictions apply retained server damage and name corrections to both inputs and the
  resulting item data. Unchanged prefilled names retain whitespace. Server outputs suppress
  local cost and invalid-result indicators. Damage-only corrections preserve edits in progress;
  a changed authoritative name refreshes the prefill, while acknowledgements that echo its
  effective name preserve edits regardless of whether the name comes from NBT or a response.
- Provisional: repair materials and item enchantment slots follow vanilla item names rather
  than each item's native registration, and data-driven anvil recipes are not predicted.
  Current-version visual and behavior parity remains incomplete; no anvil parity gate closes.


## OreUI menus, appearance and motion

- Keyboard binding labels use unmodified layout characters observed during the session while saved
  bindings keep their physical codes. Provisional: keys not yet observed use physical names;
  startup layout lookup and live AZERTY acceptance remain incomplete.
- Home, Pause, Play, Inbox, world creation, Accounts and confirmation dialogs use shared OreUI
  panels and controls. Home keeps the shipped title above translucent, top-aligned Play and character
  cards. Account access stays in the footer, Realms keeps its original text link, and the utility
  buttons retain their original widths, account icon and red Quit action. Compact menus use a centred column whose
  width stays proportional to its controls. The footer retains the original Minecraft version and
  protocol wording. Versions use their canonical sources.
  Home fits short windows without scrolling; Pause scales its controls when needed. Geometry
  regressions are written; CI and owner visual acceptance of the sizing change remain pending.
- Persistent Dark Mode defaults off, updates immediately and strengthens backdrop dimming.
  Extra large new UI applies an additional scale step to menu geometry, text and hit targets,
  independently of the ordinary GUI preference. Incomplete: exact-target sizing parity and
  rendered/input acceptance remain unverified.
  Optional Screen animations controls quick hover/press feedback, selection, glimmers, text edits,
  screen changes, modal exits, pack accordions, scrolling and interpolated loading progress. Selected
  segmented choices remain lowered; hit targets remain stable and input applies immediately.
- Server views preserve service catalog groups, artwork, descriptions, activities and available
  counts. Exact ping is optional. Collapse state and drag-reordered sections persist; custom servers
  use the owner's requested label. Resource-pack controls and loading stages retain surrounding UI.
- Dimension travel names the destination, uses its block icon and the owner's private runtime
  background beneath a strong tint. Ordinary terrain loading retains its approved neutral treatment.
- Create New World uses native panels, installed preview/category artwork and independent scrolling.
  General/Advanced changes animate their contents. Unsupported categories, Hardcore and Realm
  creation remain disabled. Backend defaults to Dragonfly or offers BDS with Docker detection;
  generator selection independently offers Normal (Vanilla) and Flat. Template navigation works.
- Dragonfly Normal uses the pinned owner-requested vanilla-gen dependency with the saved signed
  seed for all three dimensions. New worlds use its spawn; reopening retains saved spawn/chunks.
  Normal supports saved overworld pre-generation and four chunk workers by default; see
  [generation measurements](../../docs/evidence/local-world-generation.md). Bedrock generation parity
  and join/streaming budgets remain incomplete.
- Dragonfly passive animal simulation and natural population remain unsupported and incomplete.
  Stored unsupported actors are preserved without becoming live entities. Mob parity and
  performance gates remain open.
- Dressing Room persists classic/slim and static custom-geometry skins, independent capes, imports and item edits. Home
  and Pause previews support rotation and pointer tracking. Cape attachment uses its own shoulders.
  Standard fallback clothing uses quarter-texel expansion and the hat uses half-texel expansion;
  base and outer faces share the skin atlas orientation. Redeclared skin parts default to visible
  when they omit a hidden flag. Full second-layer preview parity remains incomplete.
- Cropped cape imports pad the 46×22 layout at supported texture scales with transparent pixels,
  preserving texels and private source files. This custom import extension leaves native cape
  gallery parity incomplete.
- These layouts, appearance and custom motion are owner-authorized extensions. Matching-version
  native UI parity remains open for responsive controls, rich Inbox templates and unsupported world
  settings. Preview drag gain is provisional. The chosen normal generator targets Java-style
  terrain; Bedrock terrain, structures and mobs remain unverified. No performance gate closes.

- Back navigation retains an available prior control and validates Home focus against painted
  enabled controls. Keyboard/gamepad return and pointer outline admission have regression coverage.
  Exact native Home restoration policy and full authored frontend parity remain incomplete.


## Shipped OreUI fonts

- Pack labels requesting `MinecraftTen` select Cinnangles Ten while retaining the authored text.
  Outline label sizing keeps bold and italic effects in display units, including caret advances.
  Server-form item controls resolve ordinary numeric item keys through the negotiated registry
  and existing icon sources. Capped collection factories retain a final Back button's response
  index. Target-platform shop artwork, title, controls and server-response acceptance remain
  incomplete; these changes close no shop-screen parity gate.
- Body text uses Cinnangles Seven and headings use Cinnangles Ten, derived from the shipped
  Cinnangles Sans. Optional face bundles compile before startup and use the default font when
  missing or invalid. OreUI reads no installed game fonts and rasterizes nothing at startup.
- Each OreUI face maps only its Latin, Greek, Cyrillic and symbol ranges. Its bundle holds
  unhinted FreeType rasters of them, tightly packed R8: a 52 px distance field and exact coverage
  for physical sizes 1-9 px. The shipped Sans carrier draws every other character. This restores
  the pre-Cinnangles OreUI text rendering. Full Unicode coverage, script substitution, bidirectional shaping and
  distance-field rendering of CJK remain incomplete.
- Game-bundle artwork remains a runtime input; this font change closes no native artwork,
  responsive-layout or font-parity gate.


## Shipped OreUI artwork

- OreUI icons, control faces, animations and illustrations ship as original pixel art at the
  native keys and dimensions, generated by `tools/oreui_icons` and compiled into the optional
  `oreui-icons` carrier. Screens draw them through the native-artwork path; a missing or stale
  carrier falls back to the drawn look. The installed bundle supplies only the profile banners and
  world preview; `CINNABAR_OREUI_LOCAL_ASSETS` loads its full art for comparison captures.
- Owner visual acceptance, removing the remaining install reads and matched native captures remain
  open; this closes no native artwork parity gate.


## Prior OreUI verification

- The original Japanese-label failure, texture-budget rejection and colliding rendering flags were
  reproduced before their fixes. Regression checks cover language names, small-cap MOTDs, uncached
  glyph publication, cache recovery, bounds, texture ownership, control input, focus and navigation.
- Touched Rust crates compile with tests and developer-control enabled. Focused UI, font, input,
  loading, carrier and world checks pass. Core and local-server Go suites pass, including generator
  selection and saved-world reopening. Incoming cursor-return behavior is preserved.
- Owner reviews accepted Home, Pause, Inbox, world creation, server layouts and animation direction.
  Prior macOS/Metal captures verify menu previews at 2048×1152 and Retina 2560×1440.
- Final macOS/Metal pass at 2560×1440 physical, 1280×720 logical, DPI 2: primary and CJK language
  labels, styled server MOTDs, dark Home and Quit legibility, geometry, clipping, layering and colors
  were inspected. Mouse Cancel, Escape and keyboard Cancel work; focus remains inside the modal.
  The final build also verifies restored native Play/settings/status icons, server symbols and the
  latest merged input. User language, appearance and section preferences were preserved.
- The final integrated rebuild repeats the native language/Home/Quit review with readable Japanese,
  Korean and Chinese labels, intact artwork, working modal Cancel and confirmed shutdown.


## Variable-named form buttons

- CubeCraft's Free For All cards use a variable child key containing both an
  instance name and a template reference. The resolver previously kept that
  string as a name, dropping the inherited button border, caption and input.
- Vanilla substitutes the key before splitting its name and template; the
  resolver now does so while preserving inline overrides. A regression covers
  the resulting instance name, button type, inherited children and overrides.
- The canonical client build passes. A minimal reproduction changes from an
  untyped child without descendants to the named button and its two children.
  Captured form definitions with representative menu contents emit all four
  borders, captions and button click regions.
- The owner accepted the actual form's rendered result in the live macOS Metal
  inspection build. The regression covers named inheritance and input ownership;
  this compatibility acceptance does not close the broader native parity gates.


## Cube overlay materials and scoreboard glyphs

- The live CubeCraft comparison shows floating models and the empty hand, but
  the central cube's overlays obscure its yellow base at distance and the VIP
  scoreboard icon falls back to a tiny glyph. After the requested relaunch,
  a fresh client capture shows the colored VIP icon. User acceptance of the
  cube and glyph rendering remains pending.
- Admitted reflection materials request inherited emissive shading and One/One
  additive blending. Those contracts now survive compilation and GPU submission;
  controller illumination multipliers also apply to unlit layers. Pipeline
  warming adds four additive raster contracts and reuses shader-only variants.
- The valid VIP icon U+E250 was dropped when earlier large duplicate glyphs
  exhausted the atlas. Exact raster reuse and height-first packing retain all
  768 captured cells within the existing eight pages, preserving each scalar's
  metrics, colors and lookup priority.
- Incomplete: sampler addressing, the sheen's mask behavior, wider blend factor
  pairs, and exact matching-version built-in material registration remain open.
  Regressions are authored but unrun at the owner's request. The canonical client
  build passes, and the inspection client is running after the requested
  relaunch. Changes are local and uncommitted; nothing is pushed.


## Empty boss-text HUD slots

- A captured server HUD uses eight fixed boss-text slots. Missing collection
  values left unused backgrounds visible, stacking into a black patch above XP.
  Vanilla supplies an empty name and false registration for absent entries;
  the HUD controller now supplies those defaults.
- The captured-pack offline draw reproduces all eight overlapping backgrounds
  before the fix. A retained-binding regression covers empty, unrelated, matching
  and cleared names. It is authored but unrun at the owner's request.
- The canonical client build passes. The captured-pack offline draw removes all
  eight unused backgrounds and preserves the XP number and outline.
  Incomplete: live acceptance remains pending. The updated inspection client is
  running after the requested restart. Changes are local and uncommitted;
  nothing is pushed.


## Potion tooltips

- Potion, splash and lingering potion stacks resolve their name from the
  auxiliary value (`potion.<type>[.splash|.linger].name`), shared by the
  inventory tooltip and the selected-item HUD. The tooltip lists each effect
  with its potency and `m:ss` duration, blue when beneficial and red when
  harmful. See docs/reference/potion-tooltips.md.
- Incomplete: effect amplifiers and durations come from the Minecraft wiki's
  Bedrock tables, not a native capture. Lingering durations are shown at a
  quarter of the drinkable duration, which matches the cloud's applied duration
  but is not confirmed as the native tooltip value. Base potions show no
  `No Effects` line, tipped arrows keep their generic name, and attribute
  modifier lines are not shown.


## Tip text slot

- `TextTypeTip` and its JSON variant used the action bar's slot, so a tip
  overwrote the action bar and server packs that retarget the tip text were
  ignored: the Lumine UI array list drew below the hotbar instead of the
  screen's top right.
- The tip text now has its own store slot, its `#tip_text` global and its own
  `hud_tip_text_factory` feed, with the `$destroy_id` instance and fade clock
  `popup_tip_text`. The built-in Java HUD pack defines the tip text's own
  template, and `clear_titles` clears it.
- Vanilla-template render tests cover the tip drawing beside, not over, the
  action bar; a multi-line tip keeping its last line on the tip position with
  earlier lines above it; and a pack retargeting `hud_tip_text` moving it to the
  top right. Text routing and localization tests assert the tip's own slot.
- A captured offline frame pass renders a real HUD frame at 1920x1080 under the
  Lumine UI pack and without it, on the installed Cinnangles Sans carrier so the
  glyph ink is legible. Without the pack the multi-line tip sits just above the
  status row, its last line on the tip position; with the pack the same block
  moves to the screen's top right, right-aligned, exactly where vanilla draws
  it. `server_pack_stack_hud_dump` builds its presentation through
  `pack_harness::font()`, which the fixture stub font cannot substitute for: the
  stub fills every glyph with a solid white texel, so frames drawn on it show
  text as flat blocks. See `docs/evidence/hud-tip-text-pack.md`.
- Incomplete: the frame pass uses the offline CPU raster of the real pack, not
  the GPU path on the target platform; no native parity or frame-budget gate
  closes. Two pre-existing failures are unrelated:
  `incremental_bind::hud_changes_rebuild_only_their_subtree`
  and `engine_hud_tests::java_pack_geometry_on_a_real_viewport`.


## Server-pack actors and conditional forms

- Captured CubeCraft definitions exposed rejected actor queries that discarded
  complete pre-animation scripts, leaving dynamic models at zero scale and player
  animation inactive. Camera-distance ranges, declared properties, armor slots
  and default swing duration now evaluate through the shared actor query context.
- The installed vanilla layer supplies actor rasters omitted by server packs,
  preserving server artwork precedence. Collection factories use their authored
  control-id arrays after view bindings settle, including nested factories.
- Incomplete: custom item swing-duration overrides and the vanilla guard for
  extremely narrow camera-distance ranges remain unimplemented or unverified.
  The integrated Rust client build passes; the user's live screenshots show
  floating models and the first-person hand. Selector and player animation
  acceptance remains pending. Regressions are
  authored but unrun at the owner's request. The inspection build is running
  after the requested relaunch. Changes are local and uncommitted; nothing is pushed.

## OreUI Settings

- Global Resources is an owner-requested design exception using expandable OreUI pack cards,
  imported thumbnails, priority actions and a native variant picker. Vanilla Textures is always
  the immutable base of the displayed stack. Apply tracks staged edits against the acknowledged
  running stack; finished work clears its transient status. Rendered verification is pending.
- Controller uses its native category image; category selection plays the 500ms highlight.
  Account uses the authenticated gamerpic or the signed-out silhouette. Sidebar bevels and
  detail-row top/bottom edges now retain their native state roles and group boundaries.
  The integrated Settings control regressions pass. OreUI now uses shipped Cinnangles carriers.
  The visible macOS client runs with rebuilt block assets; fresh local-world frames confirm textured terrain. These
  follow-up changes remain uncommitted, and matched Settings visual acceptance remains open.
- Settings now uses the native OreUI shell, category sidebar, grouped descriptions, right-side
  switches, filled sliders, inline choices, and independent panel scrollbars.
- Shared launcher options, key remapping, language, accounts, packs and storage retain their
  existing actions. Keyboard focus includes rows below the viewport and reveals them; resize
  clamps scroll positions before painting. Always Sprint and VSync remain user extensions.
- Regression tests reproduce legacy routing and unreachable offscreen focus. Rendered macOS
  acceptance is pending; this does not close the Settings parity gate.
- Incomplete: native options without a host model, controller binding glyphs, account management
  actions/confirmations, Touch/Party/subscription services, and native storage subroutes.
  Runtime vanilla artwork and shipped Cinnangles faces are integrated, with face metrics,
  letter spacing, coverage shading, control states and pointer/navigation timelines. Locale shaping
  and hidden-tab animation resumption still need native witnesses; disabled narration focus awaits a UI narration host. Matched visual
  acceptance remains open. Reference facts are tracked in
  [OreUI](../../docs/oreui.md).


## Original UI image animation

- Oversized resource-pack UI images retain original source pixels in immutable
  tiles. Authored UV animations can cross tile boundaries without downscaling
  their frame cells or changing the frame clock.
- UI images use the existing 64 MiB admitted-file envelope. Original tiles are
  bounded to 96 MiB across eight pages, decoded once on a shared worker and
  released on pack replacement. Large archive entries inflate and decrypt on a
  bounded worker, as do newly admitted large installed-pack images, with a
  shared 96 MiB encoded-source residency cap. Source metadata refreshes retained
  screen layouts and measurements without restarting bindings
  or animation clocks. Failed
  original-image decodes reveal valid carrier or icon artwork. Images beyond
  the resource limits remain unsupported.
  Consumers limited to one sprite retain fitted artwork for images spanning
  multiple tiles; exact multi-page icon rendering remains incomplete.
- Original pixel, loop, reload and residency-budget regressions pass. The retained
  pack inventory renders moving frames and repeats two loops in offline snapshots.
  Full-client server acceptance and hardware upload/frame budgets remain open;
  broader UI parity is incomplete.


## Compact font carriers and glyph residency

- Font carrier schema 2 stores independently compressed zlib pages, with coverage bytes for
  outline faces and RGBA retained for colored bitmap pages. Decoding checks stored and decoded
  hashes, exact stream lengths, dimensions, and the aggregate decoded-byte bound.
- Font textures use bounded demand atlases. A publication resolves all sampled glyph rectangles
  before uploading or drawing them; warm glyphs retain their placements. Full atlases repack the
  current frame's requests, and frames exceeding the bound are rejected before texture writes.
  New slots share row uploads to bound queue overhead. Source UVs remain unchanged during
  interpolation so relocation preserves SDF derivatives.
- Development-profile startup measurements reach the first completed menu GPU submission.
  Offline mixed-script menu/chat captures and native GPU pixel comparisons cover the carrier
  and atlas changes. Incomplete performance acceptance: release frame/hitch attribution and
  qualification on all reference hardware tiers remain open; these measurements do not close
  those gates.


## Java-style Tab player list

Keyboard & Mouse settings exposes Player List, with Tab as its default. Keyboard
and mouse remaps persist and reset with the existing mappings; menu Tab navigation
keeps its own binding.

- User-requested HUD extension: hold the Player List binding for the authoritative
  online roster.
- Compact centered columns, bounded to 80 players with an explicit overflow count.
- Roster changes refresh cached JSON-UI; binding release, focus loss and menus hide it.
- Each name has its skin's front face and hat layer, with the runtime Steve skin for
  missing or unsupported portraits. UUID-based skin updates refresh cached portraits;
  stock, legacy, HD, custom cube UVs and static first-frame face animations are supported.
  Rotated or otherwise unsupported head geometry uses the fallback. This is a
  user-requested extension; native Bedrock parity and performance acceptance remain incomplete.
- Input, rendered collection, focus/release, roster refresh and cache tests passed.
- Incomplete live Windows acceptance: installed-client Tab capture pending.


## Servers catalogue and classic OreUI view

The classic Servers feed now retains ranked featured and creator groups, localized descriptions,
tagged logos/banners/activity artwork and real pong MOTDs. The detail pane renders its populated
sections with independent scrolling and quick optional selection/glimmer motion. Regression
checks cover grouping, rank zero, image roles, selection across catalogue reorder, detail content
and bounded artwork packing. Integrated visible macOS review is in progress. Version-matched
native acceptance, narrow metadata layout, full-capacity badges and server notification panels
remain incomplete; this work does not close those parity gates.


## Desktop chat web links

- Requested desktop extension: recognize HTTP(S) links locally in displayed chat,
  including bare web domains, and require an in-game Open/Cancel prompt before
  handing a selected URL to the default browser. Chat messages and server packets
  are unchanged. This Java-style interaction is not a closed Bedrock parity gate.


## Absorption HUD limits

Absorption uses the local attribute's current points and the vanilla JSON-UI native
heart renderer. Incomplete: the retained HUD stat supports at most 65,535 current
points and health containers remain capped at six rows. Larger valid values are
skipped or bounded; this change does not close an unrestricted custom-health or
visual-comparison parity gate. See `docs/reference/absorption-hearts.md` for the rules.

Local swing publication: duration and progress tests cover both animation modes,
and Java torso turning uses matching committed local ticks. Incomplete: native
player body-turn timing remains on the provisional actor motion model. This work
does not close the broader native body-motion or live visual parity gate. See
`docs/reference/swing-duration.md` and `docs/reference/actor-animation-clocks.md`.


## Death-screen reasons

The dedicated server death-information packet supplies the localized reason to
the death screen independently of chat. Reasons survive either arrival order
around zero health and clear on authoritative health recovery or session replacement.
Recovery reads positive actor health independently of the rounded HUD values.
The default death screen uses the owned OreUI renderer with a radial world overlay,
centered title and literal wrapped reason, Respawn and Game menu actions, and the
HUD beneath it. Message, button, backdrop and loading animations have separate
clocks. The prompt still waits before accepting actions when animations are disabled.
Game menu returns to the same death presentation. Respawn retains progress until
authoritative recovery; pending requests survive outbound backpressure. Forced
death cancels hidden key capture, and formatted reason parameters remain literal.
The legacy JSON-UI renderer retains literal reason handling for fallback controls.
Ordinary first-person world damage rotation samples the actor's completed hurt
and death counters independently of hand animation. Scripted captures retain
their requested pose after those effects run.
Portal and fire overlays sample the final rendered camera pose and projection.
While the local player is dead, first person presents third-person-back with a
level heading fixed at the body's yaw, ignoring look and perspective input until
respawn (the native client activates its death camera component on death); the
Java 1.7 animation mode keeps first person.

Incomplete parity: exact target-version animation constants,
respawn retries, death camera framing/FOV, hurt and HUD flash timing, hardcore and secondary
client variants, and matched native frames remain open. The modern implementation
and its focused regressions do not close the full death-screen parity gate.

### Local custom models (incomplete parity acceptance, 2026-10-09)

The Dressing Room imports paired PNG/geometry JSON and free static skin packs.
It retains authored geometry and the minimum engine version across restarts and
sends them in login and skin updates. Custom models keep their authored shape;
rename and delete remain available. Preview preparation runs on a worker, while
local world presentation uses the shared actor preparation cache in both perspectives.

Starter skin compilation selects standalone model definitions above legacy definitions
at the geometry identifier boundary. This keeps classic skin previews from combining
an obsolete inherited body with the current model. The carrier emits classic and slim
models in one modern geometry schema, resolving legacy inheritance before encoding.
Authored coincident cubes and generic entity inheritance retain their existing
behavior. Reports involving other
custom entity models still need a reproducible model and pack stack; that parity
acceptance remains incomplete.

Imported raster dimensions follow the native 64×32, 64×64 and 128×128 contract.
Archive, geometry and library ceilings are Cinnabar resource limits. Pack minimum
engine versions remain separate from geometry schema versions. Full native skin-pack
import parity, animated imports, persona and Marketplace trust remain incomplete.
No visual parity or hardware performance gate closes with this extension.


## Outline label sizing and alignment

Named outline labels normalize their ascent and descent independently of the bitmap
font's ink grid. Measurement, painting, line capacity and edit metadata share the
resolved geometry, while text starts one GUI pixel below the authored top. Distance-field
labels retain fractional positions, including the origin read by caret metadata. Original
cap fixtures cover centered titles across raster sizes, GUI scales and DPI.
Incomplete: matched rendered title size and vertical alignment still require
verification in the reported server form; these CPU fixtures do not close that gate.


### Compiled block registry facts

The active block carrier stores traits and survival classifications by sequential
runtime id. State properties use generated keys and pinned scalar domains decoded
when the registry loads. Compiler selectors expand against the pinned palette and
produce a reviewed membership table; provisional rules remain explicitly listed.

Incomplete: native replacement and interaction membership, support-rule parity,
and complete survival coverage remain open.
Gameplay, shape, rendering and session consumers still require migration onto the
compiled facts. Adding this owner does not close those behavior gates.

## Server form screen variables

Action and custom forms resolve their content reference in the form screen's
variable scope, including defaults and selected conditional variables. An original
resource-pack fixture checks title-selected panels through indexed pack overlays.

Incomplete: the reported server form flash-close needs the affected pack and an
ordered packet/input trace. The separate data-driven screen packet family remains
unsupported. Live visual parity and performance acceptance remain open.


## Diagnostics settings and F3 log row

Settings → General → Advanced has two toggles, both off by default and kept in
`settings.json`. Verbose logging layers `net=debug,session=debug,core=debug,packs=debug`
over the defaults right after launch finds the install layout, and changes the filter
immediately when toggled; a non-empty `CINNABAR_LOG` or `RUST_LOG` wins and the setting
then changes nothing. Record network session asks each new session for a capture in the
log directory; its caption says the recording holds chat and player names and is never
uploaded. A non-empty `CINNABAR_PACKET_CAPTURE`, including `0`, decides instead. F3 shows
one row with the categories not at `info` (or `custom` for `RUST_LOG` or raw directives)
and the capture choice. The row is formatted again only when
`diagnostics::logging::filter_generation()` or the capture choice changes.

Verified: unit tests for setting persistence, environment precedence for both settings,
the F3 row's content and its formatting only on change, and the OreUI rows; ui-snapshot
captures of the section. Incomplete: no live client run has toggled either setting or shown
the F3 row on a real frame; the F3 row reports the choice for new sessions, not whether the
running session is recording; the toggle stays enabled when the environment overrides it.


## Retained HUD alpha segments

The UI renderer keeps bounded independent alpha rasters around invert draws.
Each invert still reads the current resolved world and earlier alpha segments;
the final segment keeps the existing output-composite policy. Removed segments,
resizes and device/view changes release their old storage. Large segment sets
use the original single-raster replay path.

Accepted publications prepare conservative glint coverage once. Clock-only
frames restore those pixels and replay overlapping translucent and model batches
in painter order. Shared depth scopes across invert boundaries and unsupported
projection or pipeline states use conservative full replay.

Incomplete validation: the focused segment/lifetime and shader pixel fixtures
are written but unrun. Matched crosshair, enchanted-item, resize and model-depth
captures and measured renderer work remain outstanding; no speedup is claimed.


## UI texture allocation recovery

UI texture resizes keep unchanged dimension buckets. If replacement storage fails,
superseded buckets are released before another attempt. Successfully allocated
and uploaded pages remain drawable; unavailable pages are absent. An unchanged
failed plan retries after 250 ms using time from the caller, while plan changes
retry immediately. Residency changes invalidate retained UI pixels even when the
publication revision stays the same.

Hardware memory-pressure captures remain outstanding. This recovery does not
close the texture streaming or hardware frame budget acceptance gates.


## Submitted server form JSON

Menu, modal and custom form submissions include the final newline inside the response
JSON string. Cancellation replies retain their absent response value. Encoded packet
regressions cover scalar answers, arrays and escaped text. Full numeric formatting,
string escaping and form admission/lifetime parity remain incomplete.
### Resolve NPC reply targets through their admitted actor lifetime

NPC dialogue carries a unique ID; action and closing replies use the runtime ID
of the actor resolved when the dialogue was admitted. Recheck that actor's full
lifetime before each enqueue, including retries, and retire replies locally when
it despawns or is replaced. Preserve the signed unique-ID bits while normalizing
the incoming dialogue. Full NPC component admission, opening-command dispatch,
server-close targeting and exact-current timing parity remain incomplete.


## Chat editor submission prefixes

Editor submissions remove leading ASCII spaces, tabs and line-control whitespace before
validation, history and slash-command routing. Trailing whitespace and non-ASCII spaces
remain intact. Explicit message-queue calls retain their exact-text contract.

Incomplete parity: native blank-line dismissal settings, account safety restrictions,
platform chat identities and the current command-version value remain unverified.

## Sign text line breaks

Sign edits retain existing trailing blank lines and line breaks entered before closing.
Unused editor rows do not add line breaks to the outbound text. The regression checks
encoded sign NBT, explicit Enter input and cursor-only navigation. Full sign text filtering,
length limits, structured text and complete editor behavior remain incomplete.
