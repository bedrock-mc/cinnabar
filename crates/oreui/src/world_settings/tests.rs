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
        4
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

/// Every catalog option is reachable on its tab, and no tab offers a Dragonfly backend.
#[test]
fn advanced_world_form_exposes_terrain_and_catalog_options() {
    use launcher::local_worlds::options::{Control, WorldOption as O};
    let mut view = MenuView::new(true, "Fixture".into());
    for tab in [Tab::Advanced, Tab::Multiplayer, Tab::Cheats] {
        view.local.tab = tab;
        let (_, hits, _) = paint(Default::default(), |canvas| {
            draw(canvas, &view, [1280.0, 4096.0], Screen::Create).unwrap();
        });
        for &id in O::ALL {
            let definition = id.definition();
            if definition.tab != tab {
                continue;
            }
            let expected = match definition.control {
                Control::Toggle => A::Option(id, 1 - definition.default),
                Control::Number { .. } => A::OptionField(id),
                Control::Choice(choices) => A::Option(id, choices[0].value),
            };
            assert!(
                hits.iter().any(|(found, _)| *found == local(expected)),
                "{id:?}"
            );
        }
        if tab == Tab::Advanced {
            assert!(
                hits.iter()
                    .any(|(found, _)| *found == local(A::Flat(false)))
            );
            assert!(hits.iter().any(|(found, _)| *found == local(A::Flat(true))));
        }
        assert!(
            !hits
                .iter()
                .any(|(found, _)| *found == local(A::Backend(bridge::Backend::Dragonfly)))
        );
    }
}

#[test]
fn world_options_edit_shows_the_saved_seed_without_generation_inputs() {
    use launcher::local_worlds::{Event, Input, WorldsMenu, world::World};
    let world = World {
        id: "fixture".into(),
        name: "Fixture".into(),
        game_mode: GameMode::Survival,
        generator: Generator::Normal,
        difficulty: Difficulty::Normal,
        backend: bridge::Backend::Bds,
        seed: -42,
        options: Default::default(),
        created_unix: 0,
        last_played_unix: 0,
        options_unavailable: false,
        size_bytes: 0,
    };
    let mut menu = WorldsMenu::default();
    menu.apply(Event::Listed(vec![world]));
    menu.update(Input::BeginEdit(0));
    menu.update(Input::SelectTab(Tab::Advanced));
    let mut view = MenuView::new(true, "Fixture".into());
    view.local = menu.view();
    let (_, hits, nodes) = paint(Default::default(), |canvas| {
        draw(canvas, &view, [1280.0, 900.0], Screen::Edit).unwrap();
    });
    assert!(
        !hits
            .iter()
            .any(|(action, _)| matches!(action, MenuAction::LocalWorld(A::SeedField | A::Flat(_))))
    );
    let text: String = nodes
        .iter()
        .filter_map(|node| match node.visual() {
            ui::UiVisual::Text { layout, .. } => Some(
                layout
                    .glyphs()
                    .iter()
                    .map(|glyph| glyph.codepoint)
                    .collect::<String>(),
            ),
            _ => None,
        })
        .collect();
    assert!(text.contains("-42"));
    assert!(text.contains(launcher::local_worlds::NORMAL_WORLD_LABEL));
}
