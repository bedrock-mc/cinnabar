use super::*;

#[test]
fn native_replacement_omits_its_subtree_and_keeps_the_pack_backdrop() {
    let art = ScreenArt {
        omit_controls: &["world_modal_progress_panel"],
        ..Default::default()
    };
    let node = |key: &str| DrawNode {
        name: "control".into(),
        key: key.into(),
        dest: RectOut {
            x: 0.0,
            y: 0.0,
            w: 1.0,
            h: 1.0,
        },
        clip: RectOut {
            x: 0.0,
            y: 0.0,
            w: 1.0,
            h: 1.0,
        },
        layer: 0,
        alpha: 1.0,
        anim: None,
        draw: Draw::Solid { color: [255; 4] },
        gates: Vec::new(),
    };
    for key in [
        "screen/content/world_modal_progress_panel",
        "screen/content/world_modal_progress_panel[2]~1/button/label",
    ] {
        assert!(art.omits(&node(key)));
    }
    for key in [
        "screen/background/dirt",
        "screen/title",
        "screen/world_modal_progress_panel_decoration",
    ] {
        assert!(!art.omits(&node(key)));
    }
}
