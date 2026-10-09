# Importing custom skins

Open the Dressing Room and choose Import skin. You can select a PNG, its matching
JSON geometry file, or a `.mcpack` skin pack. A loose pair uses the same name, such
as `robot.png` with `robot.json` or `robot.geo.json`. A loose JSON file must contain
one model. A skin pack names each model and texture in `skins.json`, includes a
`skin_pack` module in `manifest.json`, and stores custom models in `geometry.json`.
Modern `minecraft:geometry` and legacy named geometry are supported.

Use a 64×32, 64×64 or 128×128 PNG. The importer accepts free static entries.
Animated and paid entries are rejected. Custom shapes keep their authored geometry;
you can rename or delete them. Imported files are copied into your private library,
so moving or deleting the originals does not change the selected skin.

Imports are bounded by the existing archive, image and geometry resource limits.
A failed import leaves the previous selection intact. The library retains at most
256 imports. Restarting restores the selected model and its minimum engine version.
Servers receive that model during login and later skin changes; their skin policies
can affect what other players see.

This custom import feature does not establish complete native skin-pack, persona,
Marketplace trust or animation parity.
