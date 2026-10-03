# OreUI screens

26.30 draws some screens with OreUI (a web UI bundle, `data/gui/dist/hbui`) instead of JSON-UI.
Cinnabar matches the screens 26.30 uses **by default** and keeps JSON-UI where OreUI is opt-in
or flag-gated. OreUI is drawn in our own code: colours, borders, spacing, type sizes, states and
icons are re-created from reference facts. Nothing from a Minecraft install (images, fonts, CSS,
JS) is packed, bundled or committed; the open font stands in for the Minecraft fonts (accepted
deviation). A dev-only mode can load the install's originals for comparison (below).

## Which screens are OreUI by default (26.30)

The client picks a tech stack per screen (`ScreenTechStackSelector::getTechStackForScreen`): a
non-zero dev override wins (1 OreUI, 2 JSON-UI), then a preference option, then the screen's
`isSelected() && isSupported()`. Treatment toggles are true only when the service's treatment list
names them, so they default off. Evidence: the 26.30 reconstruction
(`ScreenTechStackSelectorInitializer`, `TreatmentFlightingToggles`, `DisconnectionRequestHandler`,
`OreUIGameplayUtils`) and the local install's `routes.json`.

| Screen | Route | Default | Cinnabar |
| --- | --- | --- | --- |
| Main menu | `/main-menu` | JSON-UI (dev override only) | `start_screen.json` |
| Play | `/play/:tab` | **OreUI** (selected and supported) | OreUI, drawn natively |
| Create / edit world, templates | `/create-new-world`, `/edit-world`, `/start-from-template` | **OreUI** (1.26.50: Create has no JSON-UI path) | OreUI; General and Advanced tabs only |
| Death | `/gameplay/death` | **OreUI** | OreUI |
| Bed | `/gameplay/bedtime` | **OreUI** | OreUI |
| Settings | `/oreui-settings` | JSON-UI unless the `mc-new-settings-screen` treatment (default off; preference default unrecovered) | `settings_screen.json` |
| Disconnected | `/disconnected` | JSON-UI unless treatment toggle 0x42 (default off) | `disconnect_screen.json` |
| Send invites | invite screen | JSON-UI (dev override only) | not built |
| Inventory, trade, containers | `/gameplay/inventory` ... | JSON-UI (dev option only) | Java-styled path (owner exception) |
| Profile, inbox, message, friends drawer, add friend, screenshots, storage, report | various | **OreUI-only** (no JSON-UI switch) | OreUI for profile, inbox, friends drawer |
| Achievements | `/achievements` | OreUI, needs Xbox Live | not built |

The local install used as a visual reference is 1.26.50 (its `Info.plist`), one version newer than
the target; layout facts taken from it need a 26.30 screenshot check.

## Two looks

- **Drawn (default, and anything shipped):** every OreUI surface is our code — the theme's role
  fills per state, one-texel borders, speculars and bevels, the elevated solid buttons that drop
  0.4rem when pressed, the 12/8-column grid, and our own pixel-art icons at the originals' texel
  sizes. One rem is five GUI pixels; one texel is 0.2rem.
- **Local originals (dev only):** with `CINNABAR_OREUI_LOCAL_ASSETS=<install root or its
  data/gui/dist/hbui>`, the app reads the install's sprite atlases at runtime and draws their icons
  where the drawn look approximates them. `CINNABAR_OREUI_LOOK=drawn` keeps the drawn look while
  the originals are loaded, so two runs compare side by side. Nothing is copied or packed.

Code: `app/src/ui_runtime/presentation/forms/oreui/` (theme, paint, grid, icons, widgets, one file
per screen) and `app/src/ui_runtime/oreui_assets.rs` (the dev-mode loader).

## Screenshot checks still needed

- Bed: text colour and secondary-button theme colours (unrecovered).
- Death: the radial vignette (drawn as nested bands), title and button placement, the missing
  death message and hardcore variant.
- Profile: the source-backed card, Overview and Stats layout is implemented. Matched vanilla
  captures, full navigation, screenshot persistence, privacy/offline distinctions and achievement
  reward/progress metadata remain incomplete; see `profile-parity.md` and `../plan.md`.
- Inbox: category menu, card layout, the Recent/History split.
- Friends drawer: search field (not interactive), tab icons, the People list (only friends
  currently in worlds are known).
- Create / edit world and templates: text field and segmented control art (approximated
  from `baseTextField*` facts), the side-menu tab icons, the world preview image, the missing
  Multiplayer, Cheats, pack and experiment tabs, Hardcore, and the "Leave Create New World?" prompt.
- Play: tab bar bevels, world rows (thumbnails are placeholders; no pager or grid mode), the
  Servers tab's classic layout (the `servers_tab_v2` flag's default is unknown), the Realms tab
  details (shows the first Realm; no selection), and Add server still opening the JSON-UI form.
- Every screen: text sizes, since the open font replaces the Minecraft fonts.

## Reference screenshots

The PlayCover install can be launched normally from PlayCover to capture reference screenshots
by hand; do not automate its UI, and keep captures out of git (see `AGENTS.md`).
