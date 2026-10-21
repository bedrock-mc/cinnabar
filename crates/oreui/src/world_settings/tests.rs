use super::super::theme::{NEUTRAL, NEUTRAL80, PRIMARY_ROLE, SECONDARY};
use super::super::{review_tests::paint, test_support::solids};
use {
    super::*,
    launcher::local_worlds::{Screen, Tab},
    launcher::menu::{LocalWorldAction as A, MenuAction, MenuView},
};

fn contains(outer: Bounds, inner: Bounds) -> bool {
    outer[0] <= inner[0] && outer[1] <= inner[1] && outer[2] >= inner[2] && outer[3] >= inner[3]
}

#[test]
fn world_choices_use_green_selected_and_light_unselected_faces() {
    let view = MenuView::new(true, "Fixture".into());
    let (_, hits, nodes) = paint(Default::default(), |canvas| {
        draw(canvas, &view, [1280.0, 720.0], Screen::Create).unwrap();
    });
    let fills = solids(&nodes);
    for (action, color) in [
        (A::GameMode(GameMode::Survival), PRIMARY_ROLE.fill),
        (A::GameMode(GameMode::Creative), SECONDARY.fill),
        (A::Difficulty(Difficulty::Normal), PRIMARY_ROLE.fill),
        (A::Difficulty(Difficulty::Hard), SECONDARY.fill),
    ] {
        let rect = hits
            .iter()
            .find(|(found, _)| *found == local(action))
            .unwrap()
            .1;
        let bounds = [
            rect.min().x(),
            rect.min().y(),
            rect.max().x(),
            rect.max().y(),
        ];
        assert!(
            fills
                .iter()
                .any(|(face, fill)| *fill == color && contains(bounds, *face)),
            "missing native face for {action:?}"
        );
    }
}

#[test]
fn world_sidebar_and_form_have_solid_enclosing_panels() {
    let view = MenuView::new(true, "Fixture".into());
    let (_, hits, nodes) = paint(Default::default(), |canvas| {
        draw(canvas, &view, [1280.0, 720.0], Screen::Create).unwrap();
    });
    let fills = solids(&nodes);
    for (action, color) in [(A::Create, NEUTRAL80.fill), (A::NameField, NEUTRAL.fill)] {
        let rect = hits
            .iter()
            .find(|(found, _)| *found == local(action))
            .unwrap()
            .1;
        let bounds = [
            rect.min().x(),
            rect.min().y(),
            rect.max().x(),
            rect.max().y(),
        ];
        assert!(
            fills.iter().any(|(panel, fill)| *fill == color
                && contains(*panel, bounds)
                && panel[2] - panel[0] > bounds[2] - bounds[0]),
            "missing enclosing panel for {action:?}"
        );
    }
}

#[test]
fn unsupported_world_settings_do_not_publish_inputs() {
    let view = MenuView::new(true, "Fixture".into());
    let (_, hits, _) = paint(Default::default(), |canvas| {
        draw(canvas, &view, [1280.0, 900.0], Screen::Create).unwrap();
    });
    assert!(
        hits.iter()
            .all(|(action, _)| matches!(action, MenuAction::LocalWorld(_)))
    );
    assert_eq!(
        hits.iter()
            .filter(|(action, _)| matches!(action, MenuAction::LocalWorld(A::Tab(_))))
            .count(),
        2
    );
}

#[test]
fn retained_world_form_draws_controls_above_their_panel_backgrounds() {
    let view = MenuView::new(true, "Fixture".into());
    let (_, hits, nodes) = paint(Default::default(), |canvas| {
        draw(canvas, &view, [1280.0, 900.0], Screen::Create).unwrap();
    });
    let mut tree = ui::UiTree::new(nodes).unwrap();
    tree.layout(
        ui_engine::engine::rect(0.0, 0.0, 1280.0, 900.0).unwrap(),
        ui::UiScale::new(1.0).unwrap(),
        ui::SafeArea::default(),
    )
    .unwrap();
    let draw = tree.build_draw_list().unwrap();
    for (action, expected) in [
        (A::NameField, NEUTRAL80.fill),
        (A::GameMode(GameMode::Survival), PRIMARY_ROLE.fill),
        (A::GameMode(GameMode::Creative), SECONDARY.fill),
    ] {
        let bounds = hits
            .iter()
            .find(|(found, _)| *found == local(action))
            .unwrap()
            .1;
        let point = [
            bounds.min().x() + 8.0,
            (bounds.min().y() + bounds.max().y()) * 0.5,
        ];
        let visible = draw
            .vertices
            .as_chunks::<4>()
            .0
            .iter()
            .filter_map(|quad| {
                let left = quad
                    .iter()
                    .map(|v| v.position[0])
                    .fold(f32::INFINITY, f32::min);
                let top = quad
                    .iter()
                    .map(|v| v.position[1])
                    .fold(f32::INFINITY, f32::min);
                let right = quad
                    .iter()
                    .map(|v| v.position[0])
                    .fold(f32::NEG_INFINITY, f32::max);
                let bottom = quad
                    .iter()
                    .map(|v| v.position[1])
                    .fold(f32::NEG_INFINITY, f32::max);
                (point[0] > left
                    && point[0] < right
                    && point[1] > top
                    && point[1] < bottom
                    && quad.iter().all(|v| v.color == quad[0].color)
                    && quad[0].color[3] == 255)
                    .then_some(quad[0].color)
            })
            .next_back();
        assert_eq!(
            visible,
            Some(expected),
            "{action:?} must remain visible after retained-tree ordering"
        );
    }
}

#[test]
fn advanced_world_form_exposes_backend_and_generator_separately() {
    use bridge::Backend;
    use launcher::local_worlds::world::UnavailableReason;
    let mut view = MenuView::new(true, "Fixture".into());
    view.local.tab = Tab::Advanced;
    view.local.bds_unavailable = Some(UnavailableReason::DockerMissing);
    for backend in [Backend::Dragonfly, Backend::Bds] {
        view.local.create.backend = backend;
        let (_, hits, nodes) = paint(Default::default(), |canvas| {
            draw(canvas, &view, [1280.0, 900.0], Screen::Create).unwrap();
        });
        for action in [A::Backend(Backend::Bds), A::Flat(false), A::Flat(true)] {
            assert!(hits.iter().any(|(found, _)| *found == local(action)));
        }
        let dragonfly = local(A::Backend(Backend::Dragonfly));
        assert!(!hits.iter().any(|(found, _)| *found == dragonfly));
        let text: String = nodes
            .iter()
            .filter_map(|node| match node.visual() {
                ui::UiVisual::Text { layout, .. } => Some(
                    layout
                        .glyphs()
                        .iter()
                        .map(|g| g.codepoint)
                        .collect::<String>(),
                ),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("")
            .replace(' ', "");
        assert!(text.contains("Worldgenerator"));
        assert_eq!(
            text.contains("BDSneedsDockeronMac"),
            backend == Backend::Bds
        );
    }
}
