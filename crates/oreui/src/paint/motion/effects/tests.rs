use super::*;

const SIZE: [f32; 2] = [1280.0, 720.0];

fn overlay_frame(effects: &mut Effects, alpha: Option<u8>, seconds: f64) -> Vec<UiNode> {
    let mut nodes = vec![
        UiNode::new(
            UiNodeId::new(1),
            None,
            rect(0.0, 0.0, SIZE[0], SIZE[1]).unwrap(),
        )
        .with_visual(UiVisual::Solid {
            texture_page: 0,
            color: [50, 60, 70, 255],
        }),
    ];
    let mut next = 2;
    if let Some(alpha) = alpha {
        nodes.push(
            UiNode::new(UiNodeId::new(next), None, nodes[0].bounds()).with_visual(
                UiVisual::Solid {
                    texture_page: 0,
                    color: [0, 0, 0, alpha],
                },
            ),
        );
        effects.mark_overlay(UiNodeId::new(next));
        next += 1;
    }
    effects
        .finish(&mut nodes, &mut next, seconds, SIZE)
        .unwrap();
    assert!(nodes.iter().any(
        |node| matches!(node.visual(), UiVisual::Solid { color, .. } if *color == [50, 60, 70, 255])
    ));
    nodes
}

fn dim_alpha(nodes: &[UiNode]) -> u8 {
    nodes
        .iter()
        .find_map(|node| match node.visual() {
            UiVisual::Solid { color, .. } if color[..3] == [0; 3] => Some(color[3]),
            _ => None,
        })
        .unwrap_or(0)
}

#[test]
fn background_dim_and_undim_are_finite_and_reverse_continuously() {
    let mut effects = Effects::default();
    overlay_frame(&mut effects, None, 0.0);
    assert_eq!(dim_alpha(&overlay_frame(&mut effects, Some(128), 1.0)), 0);
    let halfway = dim_alpha(&overlay_frame(&mut effects, Some(128), 1.04));
    assert!(halfway > 0 && halfway < 128);
    assert_eq!(dim_alpha(&overlay_frame(&mut effects, None, 1.04)), halfway);
    assert!(dim_alpha(&overlay_frame(&mut effects, None, 1.06)) < halfway);
    assert_eq!(dim_alpha(&overlay_frame(&mut effects, None, 1.13)), 0);
    assert!(effects.overlays.is_empty());
    overlay_frame(&mut effects, Some(128), 2.0);
    let settled = overlay_frame(&mut effects, Some(128), 2.12);
    assert_eq!(dim_alpha(&settled), 128);
    assert_eq!(overlay_frame(&mut effects, Some(128), 20.0), settled);
}

fn dialog() -> Vec<UiNode> {
    vec![
        UiNode::new(
            UiNodeId::new(2),
            None,
            rect(100.0, 100.0, 400.0, 300.0).unwrap(),
        )
        .with_clip_children(true),
        UiNode::new(
            UiNodeId::new(3),
            Some(UiNodeId::new(2)),
            rect(10.0, 10.0, 290.0, 190.0).unwrap(),
        )
        .with_focusable(true)
        .with_visual(UiVisual::Solid {
            texture_page: 0,
            color: [255; 4],
        }),
    ]
}

fn exit_frame(effects: &mut Effects, seconds: f64) -> Vec<UiNode> {
    let mut nodes = vec![UiNode::new(
        UiNodeId::new(2),
        None,
        rect(0.0, 0.0, 100.0, 100.0).unwrap(),
    )];
    effects.finish(&mut nodes, &mut 3, seconds, SIZE).unwrap();
    let mut tree = ui::UiTree::new(nodes.clone()).unwrap();
    let frame = tree
        .layout(
            rect(0.0, 0.0, SIZE[0], SIZE[1]).unwrap(),
            ui::UiScale::new(1.0).unwrap(),
            ui::SafeArea::ZERO,
        )
        .unwrap();
    tree.build_draw_list().unwrap();
    assert!(frame.focus_order().is_empty());
    nodes
}

#[test]
fn closing_dialog_retains_only_visuals_reparents_clips_and_retires_quickly() {
    let mut effects = Effects::default();
    let source = dialog();
    effects.capture(Surface::Dialog(1), &source, 10.0, SIZE);
    exit_frame(&mut effects, 0.0);
    let allocation = effects.dialogs[0].nodes.as_ptr();
    effects.capture(Surface::Dialog(1), &source, 10.0, SIZE);
    assert_eq!(effects.dialogs[0].nodes.as_ptr(), allocation);
    exit_frame(&mut effects, 0.1);
    let first = exit_frame(&mut effects, 1.0);
    assert_eq!(first.len(), 3);
    assert_ne!(first[1].id(), source[0].id());
    assert_eq!(first[2].parent(), Some(first[1].id()));
    let halfway = exit_frame(&mut effects, 1.04);
    assert!(halfway[1].bounds().min().y() > source[0].bounds().min().y());
    assert_eq!(halfway[2].bounds(), source[1].bounds());
    assert!(
        matches!(halfway[2].visual(), UiVisual::Solid { color, .. } if color[3] > 0 && color[3] < 255)
    );
    assert_eq!(effects.dialogs[0].nodes, source);
    assert_eq!(exit_frame(&mut effects, 1.09).len(), 1);
    assert!(effects.dialogs.is_empty());
}

#[test]
fn disabling_settles_backgrounds_and_discards_any_outgoing_dialog() {
    let mut effects = Effects::default();
    overlay_frame(&mut effects, None, 0.0);
    overlay_frame(&mut effects, Some(128), 1.0);
    effects.capture(Surface::Dialog(1), &dialog(), 10.0, SIZE);
    exit_frame(&mut effects, 1.0);
    effects.configure(false);
    assert_eq!(
        dim_alpha(&overlay_frame(&mut effects, Some(128), 1.001)),
        128
    );
    assert_eq!(exit_frame(&mut effects, 1.001).len(), 1);
    effects.configure(true);
    assert_eq!(
        dim_alpha(&overlay_frame(&mut effects, Some(128), 1.002)),
        128
    );
}

#[test]
fn reopening_cancels_exit_and_resize_discards_stale_geometry() {
    let mut effects = Effects::default();
    let source = dialog();
    effects.capture(Surface::Dialog(1), &source, 10.0, SIZE);
    exit_frame(&mut effects, 0.0);
    exit_frame(&mut effects, 1.0);
    effects.capture(Surface::Dialog(1), &source, 10.0, SIZE);
    let mut reopened = source.clone();
    effects.finish(&mut reopened, &mut 4, 1.02, SIZE).unwrap();
    assert_eq!(reopened, source);
    let mut resized = Vec::new();
    effects
        .finish(&mut resized, &mut 1, 1.03, [640.0, 360.0])
        .unwrap();
    assert!(resized.is_empty());
}

#[test]
fn unregistered_json_ui_backgrounds_keep_their_authored_alpha() {
    let mut effects = Effects::default();
    overlay_frame(&mut effects, None, 0.0);
    let mut nodes = vec![
        UiNode::new(
            UiNodeId::new(1),
            None,
            rect(0.0, 0.0, SIZE[0], SIZE[1]).unwrap(),
        )
        .with_visual(UiVisual::Solid {
            texture_page: 0,
            color: [0, 0, 0, 128],
        }),
    ];
    let authored = nodes.clone();
    effects.finish(&mut nodes, &mut 2, 1.0, SIZE).unwrap();
    assert_eq!(nodes, authored);
    assert!(effects.overlays.is_empty());
}
