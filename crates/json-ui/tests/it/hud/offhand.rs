//! The built-in offhand control draws one complete frame beside the hotbar.

use super::*;

/// Standalone sprite metadata keeps this layout regression independent of local carriers.
struct OffhandTextures;

impl TextureSource for OffhandTextures {
    fn texture(&self, _: &str) -> Option<TextureMeta> {
        Some(TextureMeta {
            base_size: [29.0, 24.0],
            pixels: [29.0, 24.0],
            nineslice: None,
        })
    }
}

/// Renders the shipped offhand control beside a hotbar with its normal GUI bounds.
fn offhand_nodes(
    right: bool,
    visible: bool,
    hotbar_visible: bool,
    viewport: [f64; 2],
) -> Vec<DrawNode> {
    let mut catalog = Catalog::default();
    catalog.overlay_text("ui/offhand_fixture.json", r#"{
        "namespace":"server_form",
        "long_form":{"type":"panel","controls":[{"bar":{
            "type":"panel","size":[182,22],"anchor_from":"bottom_middle","anchor_to":"bottom_middle",
            "controls":[
                {"hotbar":{"type":"custom","renderer":"hotbar_renderer","size":[182,22]}},
                {"offhand@hud.java_offhand_slot":{}}
            ]
        }}]},
        "icon":{"type":"custom","renderer":"inventory_item_renderer","size":[16,16]}
    }"#);
    let files = java_pack::files();
    catalog.apply_pack(
        files
            .iter()
            .map(|(path, bytes)| (path.as_str(), bytes.as_slice())),
    );
    catalog.overlay_text(
        "ui/offhand_item_fixture.json",
        r#"{
        "namespace":"hud","hotbar_hud_item_icon@server_form.icon":{}
    }"#,
    );
    let model = HudModel {
        hotbar_visible,
        offhand: visible.then_some(HudSlot {
            icon: Some(0),
            count: 1,
            ..Default::default()
        }),
        ..Default::default()
    };
    render_screen(
        "server_form.long_form",
        &catalog,
        &Context::desktop().with_flag("cinnabar_main_arm_left", right),
        &hud_data_source(&model),
        viewport,
        &LayoutEnv {
            text: &FixedText,
            textures: &OffhandTextures,
        },
        &ViewState::default(),
    )
    .unwrap()
    .nodes
    .to_vec()
}

#[test]
fn java_offhand_frame_and_item_align_with_the_hotbar() {
    for scale in 1..=8 {
        let viewport = [3840.0 / f64::from(scale), 2160.0 / f64::from(scale)];
        for right in [false, true] {
            let nodes = offhand_nodes(right, true, true, viewport);
            let hotbar = custom(&nodes, "hotbar_renderer")[0];
            let frames: Vec<_> = nodes
                .iter()
                .filter(|node| matches!(node.draw, Draw::Sprite { .. }))
                .collect();
            assert_eq!(frames.len(), 1, "one frame without extra caps or slices");
            let frame = frames[0];
            assert_eq!([frame.dest.w, frame.dest.h], [29.0, 24.0]);
            assert_eq!(frame.dest.y, hotbar.dest.y - 1.0);
            assert_eq!(
                frame.dest.x,
                if right {
                    hotbar.dest.x + hotbar.dest.w
                } else {
                    hotbar.dest.x - 29.0
                }
            );
            let Draw::Sprite { texture, uv, .. } = &frame.draw else {
                unreachable!()
            };
            assert_eq!(
                texture,
                &format!(
                    "textures/ui/hotbar_offhand_{}",
                    if right { "right" } else { "left" }
                )
            );
            assert_eq!(*uv, json_ui::UvRect::full());
            let icon = custom(&nodes, "inventory_item_renderer")[0];
            assert_eq!([icon.dest.w, icon.dest.h], [16.0, 16.0]);
            assert_eq!(icon.dest.x, frame.dest.x + if right { 10.0 } else { 3.0 });
            assert_eq!(icon.dest.y, hotbar.dest.y + 3.0);
        }
    }
    for (visible, hotbar_visible) in [(false, true), (true, false)] {
        let nodes = offhand_nodes(false, visible, hotbar_visible, [480.0, 270.0]);
        assert!(
            nodes
                .iter()
                .all(|node| !matches!(node.draw, Draw::Sprite { .. }))
        );
        assert!(custom(&nodes, "inventory_item_renderer").is_empty());
    }
}
