use super::*;
use crate::ui_runtime::presentation::forms::oreui::review_tests::paint;
use std::collections::HashMap;

#[test]
fn radio_selection_fades_its_center_and_face_instead_of_snapping() {
    use super::super::transitions::Transitions;
    let mut transitions = Transitions::default();
    let mut frame = |checked, seconds| animated_radio(&mut transitions, checked, seconds);
    frame(false, 0.0);
    frame(true, 1.0);
    let middle = frame(true, 1.035);
    assert!(middle.iter().any(|node| matches!(node.visual(),
        ui::UiVisual::RotatedSprite { color, .. } if color[..3] == [255; 3]
            && node.bounds().width() == node.bounds().height()
            && color[3] > 0 && color[3] < 255)));
    let settled = frame(true, 1.1);
    assert!(settled.iter().any(|node| matches!(node.visual(),
        ui::UiVisual::RotatedSprite { color, .. } if *color == theme::PRIMARY_ROLE.fill)));
}

fn animated_radio(
    transitions: &mut super::super::transitions::Transitions,
    checked: bool,
    seconds: f64,
) -> Vec<ui::UiNode> {
    use crate::ui_runtime::presentation::{TextMetrics, tests::fixture_font};
    let (mut nodes, mut next, mut layouts) = (Vec::new(), 1, ui::TextLayoutCache::new(8, 65536));
    let font = fixture_font();
    let metrics = TextMetrics::for_viewport([1280, 720], ui::DpiScale::new(1.0).unwrap(), Some(2));
    transitions.begin_frame(None, false, seconds);
    let mut canvas = Canvas::new(&mut nodes, &mut next, &mut layouts, &font, metrics, 0, None);
    canvas.seconds = seconds;
    canvas.transitions = Some(transitions);
    draw(
        &mut canvas,
        [100.0, 100.0],
        checked,
        Interaction {
            action: Some(crate::menu::MenuAction::SettingsLanguage(1)),
            ..Default::default()
        },
        true,
    )
    .unwrap();
    drop(canvas);
    transitions.end_frame();
    nodes
}

fn covers(node: &ui::UiNode, point: [f32; 2], angle: f32) -> bool {
    let bounds = node.bounds();
    let (min, max) = (bounds.min(), bounds.max());
    let centre = [(min.x() + max.x()) * 0.5, (min.y() + max.y()) * 0.5];
    let delta = [point[0] - centre[0], point[1] - centre[1]];
    let local = [
        delta[0] * angle.cos() + delta[1] * angle.sin(),
        -delta[0] * angle.sin() + delta[1] * angle.cos(),
    ];
    local[0].abs() < (max.x() - min.x()) * 0.5 && local[1].abs() < (max.y() - min.y()) * 0.5
}

#[test]
fn radio_rotates_both_specular_layers_at_opposite_corners_even_when_disabled() {
    let centre = [100.0, 100.0];
    for enabled in [true, false] {
        let mut rem = 0.0;
        let (_, _, nodes) = paint(HashMap::new(), |canvas| {
            rem = canvas.r(1.0);
            draw(canvas, centre, true, Interaction::default(), enabled).unwrap();
        });
        for (local, expected) in [
            (
                [0.7, -0.7],
                vec![theme::NEUTRAL.specular[1], theme::NEUTRAL.specular[0]],
            ),
            (
                [-0.7, 0.7],
                vec![theme::NEUTRAL.specular[1], theme::NEUTRAL.specular[0]],
            ),
            ([-0.7, -0.7], vec![theme::NEUTRAL.specular[0]]),
            ([0.7, 0.7], vec![theme::NEUTRAL.specular[1]]),
        ] {
            let point = [
                centre[0] + (local[0] - local[1]) * rem * std::f32::consts::FRAC_1_SQRT_2,
                centre[1] + (local[0] + local[1]) * rem * std::f32::consts::FRAC_1_SQRT_2,
            ];
            let layers: Vec<_> = nodes
                .iter()
                .filter_map(|node| match node.visual() {
                    ui::UiVisual::RotatedSprite {
                        color,
                        angle_radians,
                        ..
                    } if theme::NEUTRAL.specular.contains(color)
                        && covers(node, point, *angle_radians) =>
                    {
                        Some(*color)
                    }
                    _ => None,
                })
                .collect();
            assert_eq!(
                layers, expected,
                "each rotated corner retains its lighting layers, enabled={enabled}, local={local:?}"
            );
        }
    }
}
