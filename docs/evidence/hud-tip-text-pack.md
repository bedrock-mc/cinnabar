# HUD tip text under the Lumine UI pack

Offline frame evidence for the tip-text slot fix. Frames are produced by
`ui_runtime::presentation::tests::hud_server_pack_tests::server_pack_stack_hud_dump`,
which rasterizes a real presentation frame at 1920x1080 on the CPU.

## Reproducing

```sh
unzip ~/GolandProjects/proxy/internal/handler/resource/lumineui.mcpack \
  -d /tmp/opencode/lumine-pack
mkdir -p /tmp/opencode/shots
CINNABAR_FORM_SNAPSHOT_DIR=/tmp/opencode/shots \
  CINNABAR_HUD_PACK_STACK=/tmp/opencode/lumine-pack \
  cargo test -p client-ui --lib server_pack_stack_hud_dump -- --nocapture
```

Build, version, scenario and scale: `cargo test -p client-ui` in this
checkout, 1920x1080 at `DpiScale 1.0`, a populated session (stats, hotbar,
sidebar, boss bar, title, chat) plus a four-line tip text.

The dump builds its presentation through `pack_harness::font()`, which is the
installed Cinnangles Sans carrier when `.local/assets/compiled` has been built
(`make assets`). The stub font every other engine fixture uses fills each glyph
with a solid white texel, so a frame drawn on it shows text as flat blocks and
carries no ink information.

## Without the pack

The tip text's own `hud_tip_text` template draws it just above the status
row, with the last line holding the tip position and the earlier lines
stacked above it:

```
popup_tip_text  en_panel/root_panel/popup_tip_text  189.0 192.0 102.0 36.0
  text "LUMINE PROXY\nAnti Forced Packs\nJava\nJava"
```

The block's bottom edge (228) sits above the status row (231) and well
clear of the hotbar (248).

## With the Lumine UI pack

The pack overrides `hud_tip_text` with `anchor_from`/`anchor_to`
`top_right`, `offset ["0px", "24px"]` and `text_alignment: right`. The same
four-line block follows it, right-aligned to the right edge:

```
popup_tip_text  en_panel/root_panel/popup_tip_text  378.0 26.0 102.0 36.0
  text "LUMINE PROXY\nAnti Forced Packs\nJava\nJava"
```

378 + 102 = 480, the right edge of the 480x270 GUI-px space, with the first
line at the pack's 24px offset — matching where vanilla draws the proxy's
array list.

## Limitations

The frames come from the offline CPU raster of the real pack, not the GPU
path on the target platform. Geometry, anchoring, layering and glyph ink are
what they verify; no native parity or frame-budget gate closes.
