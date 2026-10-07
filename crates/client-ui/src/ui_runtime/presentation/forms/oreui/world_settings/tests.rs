use super::super::review_tests::{paint, solids};
use super::super::theme::{NEUTRAL, NEUTRAL80, PRIMARY_ROLE, SECONDARY};
use super::*;
use crate::ui_runtime::presentation::{TextMetrics, UiPresentationRuntime, tests::fixture_font};

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
fn installed_create_world_draws_its_preview_and_native_category_art() {
    use crate::ui_runtime::oreui_assets::{
        WORLD_CATEGORY_ICONS, WORLD_PREVIEW, load_optional_oreui_images,
    };
    let Some(images) = load_optional_oreui_images() else {
        eprintln!(
            "skipping installed_create_world_draws_its_preview_and_native_category_art: installed OreUI bundle unavailable"
        );
        return;
    };
    if !images.contains(WORLD_PREVIEW) {
        eprintln!(
            "skipping installed_create_world_draws_its_preview_and_native_category_art: native world preview unavailable"
        );
        return;
    }
    assert!(
        images.sprites.contains_key(WORLD_PREVIEW),
        "the preview must be resident before opening a menu"
    );
    let mut runtime = UiPresentationRuntime::new(fixture_font()).unwrap();
    runtime.enable_oreui_originals(images).unwrap();
    let mut view = MenuView::new(true, "Fixture".into());
    view.screen = crate::menu::MenuScreen::Play;
    view.local.screen = Screen::Create;
    let metrics = TextMetrics::for_viewport([1280, 900], ui::DpiScale::new(1.0).unwrap(), Some(2));
    let mut nodes = Vec::new();
    runtime
        .append_oreui_screen(
            &view,
            &mut nodes,
            &mut 1,
            metrics,
            [1280.0, 900.0],
            None,
            &|_| None,
        )
        .unwrap();
    let originals = runtime.form_presentation.oreui_originals.as_ref().unwrap();
    for key in [WORLD_PREVIEW].into_iter().chain(WORLD_CATEGORY_ICONS) {
        let sprite = originals.sprites[key];
        assert!(nodes.iter().any(|node| matches!(node.visual(), ui::UiVisual::Sprite {texture_page, uv, ..} if *texture_page == originals.page + sprite.page && *uv == sprite.bounds)), "missing installed world art {key}");
    }
}

#[test]
fn create_world_keeps_sidebar_scroll_independent_and_reveals_keyboard_choices() {
    let mut runtime = UiPresentationRuntime::new(fixture_font()).unwrap();
    let mut view = MenuView::new(true, "Fixture".into());
    view.screen = crate::menu::MenuScreen::Play;
    view.local.screen = Screen::Create;
    view.focused_action = Some(local(A::Difficulty(Difficulty::Hard)));
    view.navigation_focus_visible = true;
    let metrics = TextMetrics::for_viewport([1280, 150], ui::DpiScale::new(1.0).unwrap(), Some(2));
    let hits = runtime
        .append_oreui_screen(
            &view,
            &mut Vec::new(),
            &mut 1,
            metrics,
            [1280.0, 150.0],
            None,
            &|_| None,
        )
        .unwrap()
        .unwrap();
    let rect = hits
        .iter()
        .find(|(action, _)| Some(*action) == view.focused_action)
        .unwrap()
        .1;
    assert!(rect.min().y() >= 0.0 && rect.max().y() <= 150.0);
    assert!(
        runtime
            .form_presentation
            .menu_focus
            .contains(&local(A::NameField))
    );
    assert!(
        runtime
            .menu_scrolls
            .offsets()
            .get("world_create_general")
            .is_some_and(|offset| *offset > 0.0)
    );
    assert_eq!(
        runtime
            .menu_scrolls
            .offsets()
            .get("world_settings_sidebar")
            .copied()
            .unwrap_or(0.0),
        0.0
    );
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
        crate::ui_runtime::presentation::rect(0.0, 0.0, 1280.0, 900.0).unwrap(),
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
            .chunks_exact(4)
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
fn general_world_form_exposes_independent_choices_and_disables_unavailable_bds() {
    use protocol::world_control::{Backend, UnavailableReason};
    let mut view = MenuView::new(true, "Fixture".into());
    view.local.bds_unavailable = Some(UnavailableReason::DockerMissing);
    for available in [false, true] {
        view.local.bds_can_run = available;
        let (_, hits, _) = paint(Default::default(), |canvas| {
            draw(canvas, &view, [1280.0, 2400.0], Screen::Create).unwrap();
        });
        for action in [
            A::Backend(Backend::Dragonfly),
            A::Flat(false),
            A::Flat(true),
            A::SeedField,
            A::Cheats(true),
        ] {
            assert!(
                hits.iter().any(|(found, _)| *found == local(action)),
                "missing {action:?}"
            );
        }
        assert_eq!(
            hits.iter()
                .any(|(found, _)| *found == local(A::Backend(Backend::Bds))),
            available
        );
        assert_eq!(
            hits.iter()
                .any(|(found, _)| *found == local(A::RedetectBds)),
            !available
        );
    }
}

#[test]
fn keyboard_and_controller_focus_reveal_the_server_choice_after_scrolling() {
    use protocol::world_control::Backend;
    let mut runtime = UiPresentationRuntime::new(fixture_font()).unwrap();
    let mut view = MenuView::new(true, "Fixture".into());
    view.screen = crate::menu::MenuScreen::Play;
    view.local.screen = Screen::Create;
    view.focused_action = Some(local(A::Backend(Backend::Dragonfly)));
    view.navigation_focus_visible = true;
    let metrics = TextMetrics::for_viewport([1280, 720], ui::DpiScale::new(1.0).unwrap(), Some(2));
    let hits = runtime
        .append_oreui_screen(
            &view,
            &mut Vec::new(),
            &mut 1,
            metrics,
            [1280.0, 720.0],
            None,
            &|_| None,
        )
        .unwrap()
        .unwrap();
    let target = hits
        .iter()
        .find(|(action, _)| Some(*action) == view.focused_action)
        .unwrap()
        .1;
    assert!(target.min().y() >= 0.0 && target.max().y() <= 720.0);
    assert!(
        !runtime
            .form_presentation
            .menu_focus
            .contains(&local(A::Backend(Backend::Bds)))
    );
}
