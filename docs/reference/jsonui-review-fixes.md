# JSON-UI review corrections

The supplied JSON-UI findings 1–4 and UI/app findings 1, 2 and 4 are reproducible.
Tests first observed the animation panic, missed component writes, absent title
labels, missing Escape event, stale recipe icon and camera reset. The input test
captures the real Drop setting and reopens the inventory before checking both Q
and R, including the Ctrl modifier.

The corrections reject authored internal animation programs and validate their
indices, share instance keys between binding and layout, seed native creation
bags with form titles, restore the saved screen cancel route after unconsumed
Escape, suppress the unbound physical Q shortcut, explicitly clear recipe icons,
and apply perspective only when its configured option changes.

## Behavior references

The vanilla pack is the read-only pinned `v1.26.50.4/full/resource_pack` under the
installed bedrock-samples tree. References below identify behavior rather than
copied implementation source.

- Animation construction: `R:u/UIAnimationComponent.cpp:1142` reads resolved
  animation definitions. Vanilla `ui/server_form.json` and `ui/ui_common.json`
  use authored animation references, not the engine's serialized `anim_graph`.
  Lens current-client source search for `wait_until_rendered_to_play` found
  `FUN_144d2e9f0`, RVA `0x4d2e9f0` (1.26.50.26).
- Instance bags: `R:u/UIControlFactory.cpp:7251` reads `collection_index` into the
  instance component; `R:u/UIControl.cpp:2379` processes control property bags.
  Vanilla `ui/server_form.json:118` creates dynamic buttons through a factory.
- Title creation: `R:s/ServerFormScreenController.cpp:975` and `:1022` set
  `#title_text` in custom/action factory creation bags. Vanilla
  `ui/server_form.json:41` and `:258` select `#title_text` with a `none` binding.
  Lens current-client source search for `title_text` found the same token in
  `FUN_1455005d0`, RVA `0x55005d0` (1.26.50.26).
- Cancel routing: vanilla `ui/server_form.json:11` maps `button.menu_cancel` to
  `button.menu_exit` globally on the enclosing screen. Lens current-client
  source search found `button.menu_cancel` in `FUN_1473abae0`, RVA `0x73abae0`
  (1.26.50.26). A consuming inner control retains priority over screen cancel.
- Drop remapping: `R:k/KeyboardRemappingLayout.cpp:85` replaces the key vector
  with the newly captured key; `R:v/VanillaClientInputMappingFactory.cpp:2506`
  routes the inventory Drop action. Lens current-client source search for
  `menu_inventory_drop` found `FUN_147395f10`, RVA `0x7395f10` (1.26.50.26).
- Recipe cells: vanilla `ui/ui_common.json:3946` binds `#item_renderer_data`.
  `R:i/InventoryItemRenderer.cpp:2829` reads the current control property bag.
  Retaining a previous row's renderer index contradicts that row's item data.
- Perspective: `R:c/ClientInstance.cpp:4226` registers the perspective option's
  callback; `R:c/ClientInstance.cpp:33243` implements `_perspectiveOptionChanged`.
  An unrelated settings snapshot is not a perspective option change.
- Font measurement: `R:t/TextComponent.cpp:1832` obtains the current FontHandle
  while validating measured text; `R:t/TextComponent.cpp:867` uses it to draw.
  Vanilla `ui/ui_template_dialogs.json:9` defines the standard title label.
  The accepted open-font deviation still requires layout and drawing to use
  the same installed metrics.

Lens indexed current-client snippets were available. Requested detailed function
reads returned an unavailable analysis service; the named mcsrc owners and real
vanilla pack provide the detailed corroboration. The references do not establish
a native frame-time budget for the captured shop.

## Live form investigation (incomplete)

An independently reproduced defect keeps an open form's cached geometry after a
session font is installed or removed. Its layout cache omitted font identity.
The late-font test observes one new layout per font change and no further layouts
while that font is stable. Offline PNGs compare the same open form before and
after installing deliberately different glyph metrics. This defect is fixed;
its connection to the owner's black rectangle or FPS report is unproven.

The exact captured Spirit Bundle request is also replayed before installing its
UI pack. The test waits for the button/backdrop textures to become resident,
finishes artwork preparation, checks that purple bundle pixels reach the published
pages, then compares all published page identities and layout pass counts across
later frames. These witnesses exercise the real carrier and captured pack. They
do not connect to a server or exercise GPU submission.

The existing byte-artwork cache already keys decoded images by payload hash.
Pack replacement clears the form binding/layout cache; decoded atlas textures
have header dimensions before pixel residency, and URL completion changes the
form image model. No additional texture churn or per-frame relayout defect is
established by source inspection alone. The captured request uses pack paths;
it does not establish a failure of the live URL downloader.

The reported black artwork, floating labels and 110-to-41 FPS loss remain open.
No live visual or performance parity gate is closed. Captures and before/after
PNGs remain outside git. The focused tests and final local gate are reported
with the final committed HEAD.
