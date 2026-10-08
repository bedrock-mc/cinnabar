# OreUI screens

Vanilla draws some screens with OreUI (a web UI bundle, `data/gui/dist/hbui`) instead of JSON-UI.
Cinnabar uses the target's OreUI Settings layout, including its category sidebar and grouped
controls. The other routes below retain their documented selection policy. OreUI is drawn in our
own code with vanilla component geometry, states, spacing and type metrics. Installed runtime
assets supply native images and font faces; Minecraft content is never bundled or committed.

Native text preserves its primary face and uses the installed locale-specific Noto chain for
missing glyphs, including CJK language names and server MOTDs. Fallback pages are shared across
families and raster sizes. Additional characters rasterize on a worker into a bounded cache;
unchanged text retains its layouts and textures. Full script shaping remains an open parity gate.

## Screen routes

The client picks a tech stack per screen: a non-zero dev override wins (1 OreUI, 2 JSON-UI),
then a preference option, then OreUI only when the screen's OreUI version is both selected
and supported. Treatment toggles are true only when the service's treatment list names them, so they default off. The local install's `routes.json` lists screen routes.

| Screen | Route | Default | Cinnabar |
| --- | --- | --- | --- |
| Main menu | `/main-menu` | Vanilla: JSON-UI; owner-requested design extension | Shared title above OreUI action and character panels; compact utility buttons |
| Play | `/play/:tab` | **OreUI** (selected and supported) | OreUI, drawn natively |
| Add / edit server | server draft | Owner-requested design extension | OreUI modal, scrollable fields and fixed Save / Join actions |
| Pause | game menu | Owner-requested design extension | Uniform world overlay, OreUI action panel and interactive character card with Dressing Room access |
| Connecting / loading | world-entry progress | Owner-requested design extension | OreUI progress card; existing stage, cancellation and download fraction semantics |
| Create / edit world, templates | `/create-new-world`, `/edit-world`, `/start-from-template` | **OreUI** (1.26.50: Create has no JSON-UI path) | OreUI; General and Advanced tabs only |
| Death | `/gameplay/death` | JSON-UI through the OreUI entry point | Carrier `death.death_screen` |
| Bed | `/gameplay/bedtime` | **OreUI** | OreUI |
| Settings | `/oreui-settings` | OreUI in the supplied vanilla witness | Native OreUI; shared persisted values, input bindings and OreUI pack variants |
| Dressing Room | classic skin selector | Owner-approved custom layout | OreUI character preview and skin cards; installed starter skins and imported PNGs |
| Disconnected | `/disconnected` | JSON-UI unless treatment toggle 0x42 (default off) | `disconnect_screen.json` |
| Send invites | invite screen | JSON-UI (dev override only) | not built |
| Inventory, trade, containers | `/gameplay/inventory` ... | JSON-UI (dev option only) | Java-styled path (owner exception) |
| Profile, inbox, message, friends drawer, add friend, screenshots, storage, report | various | **OreUI-only** (no JSON-UI switch) | OreUI for profile, inbox, friends drawer |
| Achievements | `/achievements` | OreUI, needs Xbox Live | not built |

The route policy for other screens was established against 26.30. Settings follows the
current target's registered controls and the supplied vanilla witness. Its local component
witness is 1.26.51.01; a version-matched native capture at the same resolution and scale
remains necessary to close the parity gate.

Global Resources is an owner-requested design exception: expandable OreUI pack cards,
an immutable Vanilla Textures base card, staged priority controls, and a variant picker.
Apply compares the staged stack with the last runtime acknowledgement. Imported artwork
uses the existing bounded artwork loader; vanilla artwork is read from the installed bundle.
Expanded card details share equal interior gutters. Settings opened from a game keeps the
world beneath its dimming overlay; launcher Settings keeps the title background.

Dressing Room uses a custom OreUI wardrobe with Steve, Alex and imported skins. Skin and cape
selection are independent and persist across launches; both reach the local character and network
skin. Official Java cape textures download into the private runtime cache. Custom PNG skins and
capes support rename and removal. The fitted preview has idle arm motion, drag rotation and mouse
head tracking. Home and Pause fit the interactive character inside their own cards. Profile remains
a separate screen.

Pause, server drafts and loading use an owner-requested OreUI design rather than vanilla's legacy
dialogs. Server drafts close from the corner X; shared fields center visible glyphs inside an even
inset border. Pause uses the current account name and server-pack title. Its caption and actions
form a bottom-aligned group, with extra breathing room above the logo. Loading keeps
the installed dimension backdrop beneath a dark tint and groups server branding with a compact
status card. Terrain keeps its grey tint; dimension travel names the destination over its own
full-resolution scenery, center-cropped to fill the viewport beneath a dark tint. The installed
grass block and existing netherrack/End-stone item icons identify each destination. Optional scenery
loads once from the user-data `assets/dimension-loading/{overworld,nether,end}.png` files. Missing
scenery keeps the charcoal backdrop; missing icon art leaves the destination text. Unknown dimensions
use a generic label. Terrain and dimension cards use a neutral edge and shadow. Connecting keeps its panorama
treatment. Unknown progress uses the OreUI
cube loader; pack downloads show their actual completion fraction. Pause has no decorative green
stripe above its logo.
Interface click sounds are disabled at the owner's request. Other authored interface feedback and
gameplay sounds keep their existing playback path.

The owner-requested home layout places the shipped logo above the Play and interactive character
panels. Servers and Settings are secondary; Realms and Marketplace are quiet text actions without
promotional art. Compact utility buttons group account, friends, inbox and quit below the panels.
The footer names the app version, Minecraft version and protocol number from their shared sources.
Compact screens retain Dressing Room access and scroll the main actions.

The owner-requested motion extension uses finite cubic easing: 85 ms for highlights,
45 ms for depression, 80 ms for release/dialog exits and 120 ms for entrances. Segmented
choices retain independent selection state and stay lowered while selected. Switch travel takes
100 ms, radio diamonds ease their selection and settings/Play/Inbox icon glimmers complete in 200 ms.
Text fields use the shared caret blink. Newly inserted characters reveal in 75 ms with a small
rise, sharing the final line layout; edit caret travel uses the same duration. Pointer and keyboard
caret placement stays immediate. Play tabs load their native border images and category icons.
Inbox uses native category faces, independent sidebar/message scrolling and illustrated,
category-specific empty cards. Its category changes animate only the message pane; message views,
maintenance pages and deletion dialogs use the same finite transitions. Background dimming eases in 100 ms and out in 80 ms. World tabs animate their content;
joining stages animate on semantic changes without replaying for download progress updates.
Click targets stay fixed and input remains immediate. Settings → Video → Screen animations
disables these transitions; toggling it settles active motion immediately. Settled controls keep
their native artwork. Live animation review remains open.

## Runtime artwork

- An available installed bundle supplies its atlases, standalone artwork and animations at
  runtime. `CINNABAR_OREUI_LOCAL_ASSETS=<install root or its data/gui/dist/hbui>` selects a bundle
  explicitly. Native button faces use the original state-specific nine-slice images. Switches,
  sliders, segmented controls and radio diamonds use their native role fills and bevel geometry.
- Minecraft Seven supplies body text and Minecraft Ten supplies headings, with native em metrics,
  line heights and letter spacing. Minecraft Five variants are available for their native roles.
  Native outline text uses CPU distance fields and the native coverage shader; small text
  uses pixel-size raster variants. The default HUD and JSON-UI font remains independently selectable.
- Without a native bundle, drawn artwork and the open font remain a diagnostic fallback.
  The renderer uses that artwork only when a runtime bundle is unavailable. This fallback does not
  satisfy the native artwork parity gate. One rem is five GUI pixels; one texel is 0.2rem.

Code: `crates/client-ui/src/ui_runtime/presentation/forms/oreui/` (theme, paint, grid, icons,
widgets, one file per screen) and `crates/client-ui/src/ui_runtime/oreui_assets.rs`
(the runtime loader). The saved-accounts picker, which vanilla lacks, is an OreUI modal menu. Profile models, feed projections, and value formatting live in
`crates/launcher/src/menu/`; app owns service polling and command dispatch.

## Screenshot checks still needed

- Settings: full native option coverage, controller binding glyphs, account management,
  Touch/Party/subscription services, and storage subroutes remain incomplete. Runtime
  artwork, core fonts and CPU distance fields are integrated; locale shaping, hidden-tab
  animation resumption and matched captures remain open; see `../plan.md`.
- Bed: text colour and secondary-button theme colours (unrecovered).
- Death: the carrier owns its gradient, title, reason and button layout. Controller
  timing, death camera, respawn transitions and hardcore variants still need matched
  native captures.
- Profile: the vanilla card, Overview and Stats layout is implemented. Matched vanilla
  captures, full navigation, screenshot persistence, privacy/offline distinctions and achievement
  reward/progress metadata remain incomplete; see `profile-parity.md` and `../plan.md`.
- Inbox: matched captures, preference toggles, subscriber variants and rich message templates.
- Friends drawer: search field (not interactive), tab icons, the People list (only friends
  currently in worlds are known).
- Create / edit world and templates: matched captures, compact choice dropdowns, functional
  Multiplayer, Cheats, pack and experiment tabs, Hardcore, Realm creation, and the
  "Leave Create New World?" prompt.
- Play: tab bar bevels, world rows (thumbnails are placeholders; no pager or grid mode), the
  Servers tab's classic layout (the `servers_tab_v2` flag's default is unknown), the Realms tab
  details (shows the first Realm; no selection).
- Add server, Pause and loading: owner live review of the custom layouts, focus and scaling.
- Every screen: native text metrics and runtime artwork at matched resolution and scale.

## Reference screenshots

The PlayCover install can be launched normally from PlayCover to capture reference screenshots
by hand; do not automate its UI, and keep captures out of git (see `AGENTS.md`).

Create New World uses the installed preview and category art, solid form rows and the same native
choice faces as Settings. Sidebar/form tabs scroll independently and retain their surrounding
screen during quick category changes. The world backend's unsupported categories, Hardcore and
Realm creation remain disabled; the Realms action keeps the owner's quieter commerce treatment.
Advanced separates Backend (Dragonfly/BDS) from World generator (Normal/Flat). Dragonfly is the
default; selecting unavailable BDS shows its detected Docker warning without changing the generator.
