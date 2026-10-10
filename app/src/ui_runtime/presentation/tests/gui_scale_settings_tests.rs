use bevy::{
    prelude::{App, MinimalPlugins, Update, With},
    window::{PrimaryWindow, Window, WindowResolution},
};
use ui::{DpiScale, UiPoint};

use super::{engine_hud_tests::engine_presentation, fixture_font, fixture_hud};
use crate::{
    menu::{MenuAction, MenuRuntime, MenuScreen},
    ui_runtime::presentation::apply_gui_scale_setting,
};
use client_ui::ui_runtime::{UiRuntime, presentation::UiPresentationRuntime};

fn settings_app(visible: bool, preference: Option<u8>) -> App {
    let mut menu = MenuRuntime::new(visible, 2, "Player".to_owned());
    menu.activate(MenuAction::SettingsScale(0));
    menu.set_gui_scale_preference(preference);
    let presentation = UiPresentationRuntime::with_hud(fixture_font(), fixture_hud()).unwrap();
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .insert_resource(crate::player_runtime::PlayerRuntime::new(1))
        .insert_resource(menu)
        .insert_resource(presentation)
        .add_systems(Update, apply_gui_scale_setting);
    app.world_mut().spawn((
        Window {
            resolution: WindowResolution::new(1920, 1080),
            ..Default::default()
        },
        PrimaryWindow,
    ));
    app.update();
    app
}

fn build_menu(app: &mut App, physical: [u32; 2], dpi: f32) -> render_model::UiRenderInput {
    {
        let world = app.world_mut();
        let mut windows = world.query_filtered::<&mut Window, With<PrimaryWindow>>();
        windows
            .single_mut(world)
            .unwrap()
            .resolution
            .set_physical_resolution(physical[0], physical[1]);
    }
    app.update();
    let view = app.world().resource::<MenuRuntime>().view();
    app.world_mut().resource_scope(
        |world, mut presentation: bevy::prelude::Mut<UiPresentationRuntime>| {
            presentation.set_menu_view(Some(view));
            presentation
                .build(
                    world.resource::<crate::player_runtime::PlayerRuntime>(),
                    &UiRuntime::new(1),
                    0,
                    physical,
                    DpiScale::new(dpi).unwrap(),
                )
                .unwrap()
        },
    )
}

fn largest_font_quad_height(input: &render_model::UiRenderInput) -> f32 {
    input
        .batches
        .iter()
        .filter(|batch| batch.texture_page == 0)
        .flat_map(|batch| {
            let start = batch.first_index as usize;
            let end = start + batch.index_count as usize;
            input.indices[start..end]
                .as_chunks::<6>()
                .0
                .iter()
                .map(|indices| {
                    let positions = indices
                        .iter()
                        .map(|index| input.vertices[*index as usize].position[1]);
                    let bottom = positions.clone().fold(f32::NEG_INFINITY, f32::max);
                    let top = positions.fold(f32::INFINITY, f32::min);
                    bottom - top
                })
        })
        .fold(0.0, f32::max)
}

fn native_toggle_height(app: &App, dpi: DpiScale) -> f32 {
    let presentation = app.world().resource::<UiPresentationRuntime>();
    let (action, bounds) = client_ui::test_support::menu_hit_targets(presentation)
        .iter()
        .find(|(action, _)| {
            matches!(action, MenuAction::SettingsOption(index, _)
            if matches!(crate::menu::settings_options::SETTINGS_OPTIONS[usize::from(*index)].kind,
                crate::menu::settings_options::SettingKind::Toggle))
        })
        .expect("the initial native Settings category renders a toggle");
    let physical_centre = [
        (bounds.min().x() + bounds.max().x()) / 2.0 * dpi.get(),
        (bounds.min().y() + bounds.max().y()) / 2.0 * dpi.get(),
    ];
    assert_eq!(
        presentation.hit_test_menu(UiPoint::from_physical(physical_centre, dpi).unwrap()),
        Some(*action),
        "physical pointer conversion selects the rendered native control"
    );
    bounds.height() * dpi.get()
}

#[test]
fn gui_scale_minimum_on_high_dpi_resizes_native_menu_text_controls_and_pointer() {
    let Some(presentation) = engine_presentation() else {
        eprintln!(
            "skipping gui_scale_minimum_on_high_dpi_resizes_native_menu_text_controls_and_pointer: fixture unavailable; requires installed local carriers (make assets)"
        );
        return;
    };
    let mut app = settings_app(true, None);
    app.insert_resource(presentation);
    app.world_mut()
        .resource_mut::<MenuRuntime>()
        .activate(MenuAction::Navigate(MenuScreen::Settings));
    let physical = [1280, 720];
    let dpi = DpiScale::new(2.0).unwrap();
    let before = build_menu(&mut app, physical, dpi.get());
    let before_toggle = native_toggle_height(&app, dpi);
    let point = UiPoint::new(120.0, 90.0).unwrap();
    let before_pointer = app
        .world()
        .resource::<UiPresentationRuntime>()
        .inventory_gui_point(point, physical, dpi.get())
        .unwrap();
    app.world_mut()
        .resource_mut::<MenuRuntime>()
        .activate(MenuAction::SettingsScale(-1));
    let after = build_menu(&mut app, physical, dpi.get());
    assert!(largest_font_quad_height(&before) > 0.0);
    assert_eq!(
        largest_font_quad_height(&after),
        largest_font_quad_height(&before) / 2.0,
        "native scale 1 is half scale 2 even when the platform DPI is 2"
    );
    assert_eq!(native_toggle_height(&app, dpi), before_toggle / 2.0);
    assert_ne!(before.revision, after.revision);
    let presentation = app.world().resource::<UiPresentationRuntime>();
    assert_eq!(
        presentation
            .inventory_gui_point(point, physical, dpi.get())
            .unwrap(),
        [before_pointer[0] * 2.0, before_pointer[1] * 2.0]
    );
}

#[test]
fn viewport_text_metrics_cover_supported_dpi_and_native_gui_scale_bounds() {
    for dpi in [DpiScale::MIN, 2.0, DpiScale::MAX] {
        for gui in [1, ui::gui_scale([3840, 2160], None) as u8] {
            let metrics = client_ui::test_support::text_metrics(
                [3840, 2160],
                DpiScale::new(dpi).unwrap(),
                Some(gui),
            );
            assert_eq!(
                client_ui::test_support::text_scale(&metrics).get()
                    * ui::FONT_DESIGN_PIXEL_TEXELS as f32
                    * dpi,
                gui as f32,
                "derived font scale preserves physical GUI scale {gui} at DPI {dpi}"
            );
            for factor in [0.5, 0.75, 1.5] {
                let styled = client_ui::test_support::text_scale(&metrics).get() * factor;
                assert_eq!(ui::UiScale::new_display(styled).unwrap().get(), styled);
            }
        }
    }
}

#[test]
fn gui_scale_video_action_resizes_rendered_menu_text_and_keeps_hits_aligned() {
    let mut app = settings_app(true, None);
    app.world_mut()
        .resource_mut::<MenuRuntime>()
        .activate(MenuAction::Navigate(MenuScreen::Settings));
    let before = build_menu(&mut app, [1920, 1080], 1.0);
    let before_height = largest_font_quad_height(&before);
    assert!(before_height > 0.0, "the menu renders text");

    app.world_mut()
        .resource_mut::<MenuRuntime>()
        .activate(MenuAction::SettingsScale(-1));
    app.update();
    let after = build_menu(&mut app, [1920, 1080], 1.0);
    assert_eq!(largest_font_quad_height(&after), before_height / 4.0 * 3.0);
    assert_ne!(before.revision, after.revision);

    let presentation = app.world().resource::<UiPresentationRuntime>();
    assert!(!client_ui::test_support::menu_hit_targets(presentation).is_empty());
    for (action, bounds) in client_ui::test_support::menu_hit_targets(presentation) {
        let centre = UiPoint::new(
            (bounds.min().x() + bounds.max().x()) / 2.0,
            (bounds.min().y() + bounds.max().y()) / 2.0,
        )
        .unwrap();
        assert_eq!(presentation.hit_test_menu(centre), Some(*action));
    }
}

#[test]
fn gui_scale_video_action_updates_inventory_pointer_coordinates_while_menu_is_hidden() {
    let mut app = settings_app(false, None);
    let point = UiPoint::new(300.0, 180.0).unwrap();
    let physical = [1920, 1080];
    let dpi = 1.5;
    let before = app
        .world()
        .resource::<UiPresentationRuntime>()
        .inventory_gui_point(point, physical, dpi)
        .unwrap();

    app.world_mut()
        .resource_mut::<MenuRuntime>()
        .activate(MenuAction::SettingsScale(-1));
    app.update();
    let after = app
        .world()
        .resource::<UiPresentationRuntime>()
        .inventory_gui_point(point, physical, dpi)
        .unwrap();
    assert_eq!(after, [before[0] * 4.0 / 3.0, before[1] * 4.0 / 3.0]);
    assert!(!app.world().resource::<MenuRuntime>().is_visible());
}

#[test]
fn gui_scale_keeps_auto_responsive_and_clamps_saved_native_offset_after_resize() {
    let mut app = settings_app(true, None);
    let automatic = build_menu(&mut app, [1920, 1080], 1.0);
    let smaller = build_menu(&mut app, [1280, 720], 1.0);
    assert_eq!(
        largest_font_quad_height(&automatic),
        largest_font_quad_height(&smaller) * 2.0,
        "automatic scale responds to the current physical window dimensions"
    );

    build_menu(&mut app, [1920, 1080], 1.0);
    app.world_mut()
        .resource_mut::<MenuRuntime>()
        .activate(MenuAction::SettingsScale(-2));
    app.update();
    let capped = build_menu(&mut app, [1280, 720], 1.0);
    assert_eq!(
        largest_font_quad_height(&capped),
        largest_font_quad_height(&smaller) / 2.0
    );
    let menu = app.world().resource::<MenuRuntime>();
    assert_eq!(
        menu.gui_scale_offset(),
        -2,
        "the saved option survives resize"
    );
    assert_eq!(
        menu.view().gui_scale_offset,
        -1,
        "the native option shows the clamped modifier"
    );
    let restored = build_menu(&mut app, [1920, 1080], 1.0);
    assert_eq!(
        largest_font_quad_height(&restored),
        largest_font_quad_height(&automatic) / 2.0
    );
}

#[test]
fn gui_scale_video_action_relayouts_cached_engine_hud_at_the_new_scale() {
    let mut player_runtime = crate::player_runtime::PlayerRuntime::new(1);

    let Some(mut presentation) = engine_presentation() else {
        eprintln!(
            "skipping gui_scale_video_action_relayouts_cached_engine_hud_at_the_new_scale: fixture unavailable; requires installed local carriers (make assets)"
        );
        return; // The real JSON-UI carrier is local and never committed.
    };
    *presentation.hud_frame_mut() = super::super::HudFrame {
        first_person: true,
        ..Default::default()
    };
    let mut app = settings_app(false, None);
    app.insert_resource(presentation);
    app.update();

    let runtime = UiRuntime::new(1);
    player_runtime
        .facts
        .publish_player_game_mode(protocol::PlayerGameMode::Survival);
    app.insert_resource(player_runtime);
    let physical = [1920, 1080];
    for (offset, physical_scale) in [(-2, 2), (0, 4)] {
        app.world_mut()
            .resource_mut::<MenuRuntime>()
            .activate(MenuAction::SettingsScale(offset));
        app.update();
        app.world_mut().resource_scope(
            |world, mut presentation: bevy::prelude::Mut<UiPresentationRuntime>| {
                let player_runtime = world.resource::<crate::player_runtime::PlayerRuntime>();
                let passes = presentation.hud_passes();
                let input = presentation
                    .build(
                        player_runtime,
                        &runtime,
                        0,
                        physical,
                        DpiScale::new(1.5).unwrap(),
                    )
                    .unwrap();
                assert_eq!(presentation.hud_passes(), passes + 1);
                assert_crosshair_size(&input, physical, physical_scale);
            },
        );
    }
}

fn assert_crosshair_size(input: &render_model::UiRenderInput, physical: [u32; 2], scale: u8) {
    let crosshair = input
        .vertices
        .as_chunks::<4>()
        .0
        .iter()
        .map(|quad| {
            quad.iter().fold(
                [
                    f32::INFINITY,
                    f32::INFINITY,
                    f32::NEG_INFINITY,
                    f32::NEG_INFINITY,
                ],
                |bounds, vertex| {
                    [
                        bounds[0].min(vertex.position[0]),
                        bounds[1].min(vertex.position[1]),
                        bounds[2].max(vertex.position[0]),
                        bounds[3].max(vertex.position[1]),
                    ]
                },
            )
        })
        .find(|bounds| {
            (bounds[0] + bounds[2]) / 2.0 == physical[0] as f32 / 2.0
                && (bounds[1] + bounds[3]) / 2.0 == physical[1] as f32 / 2.0
        })
        .expect("the engine HUD renders its crosshair");
    let size = assets::HudTextureRole::Crosshair.expected_size();
    assert_eq!(crosshair[2] - crosshair[0], size[0] as f32 * scale as f32);
    assert_eq!(crosshair[3] - crosshair[1], size[1] as f32 * scale as f32);
}

#[test]
fn gui_scale_minimum_on_high_dpi_relayouts_cached_native_hud() {
    let mut player_runtime = crate::player_runtime::PlayerRuntime::new(1);

    let Some(mut presentation) = engine_presentation() else {
        eprintln!(
            "skipping gui_scale_minimum_on_high_dpi_relayouts_cached_native_hud: fixture unavailable; requires installed local carriers (make assets)"
        );
        return;
    };
    *presentation.hud_frame_mut() = super::super::HudFrame {
        first_person: true,
        ..Default::default()
    };
    let mut app = settings_app(false, None);
    app.insert_resource(presentation);
    let physical = [1280, 720];
    {
        let world = app.world_mut();
        let mut windows = world.query_filtered::<&mut Window, With<PrimaryWindow>>();
        windows
            .single_mut(world)
            .unwrap()
            .resolution
            .set_physical_resolution(physical[0], physical[1]);
    }
    let runtime = UiRuntime::new(1);
    player_runtime
        .facts
        .publish_player_game_mode(protocol::PlayerGameMode::Survival);
    app.insert_resource(player_runtime);
    for (offset, scale) in [(0, 2), (-1, 1), (0, 2)] {
        app.world_mut()
            .resource_mut::<MenuRuntime>()
            .activate(MenuAction::SettingsScale(offset));
        app.update();
        app.world_mut().resource_scope(
            |world, mut presentation: bevy::prelude::Mut<UiPresentationRuntime>| {
                let player_runtime = world.resource::<crate::player_runtime::PlayerRuntime>();
                let passes = presentation.hud_passes();
                let input = presentation
                    .build(
                        player_runtime,
                        &runtime,
                        0,
                        physical,
                        DpiScale::new(2.0).unwrap(),
                    )
                    .unwrap();
                assert_eq!(presentation.hud_passes(), passes + 1);
                assert_crosshair_size(&input, physical, scale);
            },
        );
    }
}
