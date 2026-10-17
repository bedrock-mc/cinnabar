//! Enlarged modal and gameplay overlays retain visible text and reachable actions.

use super::*;

/// Runs a screen adapter with fixture fonts and the caller's unchanged frame metrics.
fn with_frame<T>(
    size: [f32; 2],
    metrics: TextMetrics,
    paint: impl FnOnce(&mut PaintFrame<'_>) -> T,
) -> (T, Vec<UiNode>) {
    let font = fixture_font();
    let mut layouts = TextLayoutCache::new(64, 1024 * 1024);
    let (mut nodes, mut next) = (Vec::new(), 1);
    let result = paint(&mut PaintFrame {
        layouts: &mut layouts,
        font: &font,
        metrics,
        solid_page: 0,
        safe_area: SafeArea::ZERO,
        content: size,
        translate: &|_| None,
        language: [0; 3],
        now: 0.0,
        nodes: &mut nodes,
        next: &mut next,
        renderers: &mut [],
    });
    (result, nodes)
}

#[test]
fn extra_large_death_keeps_both_actions_inside_small_windows() {
    for size in [ui::MIN_WINDOW_LOGICAL_SIZE, [400.0, 250.0], [752.0, 500.0]] {
        for dpi in [1.0, 2.0] {
            for enabled in [0, 1] {
                for hardcore in [false, true] {
                    for reason in ["", "A long death reason that wraps across several lines. "] {
                        let mut view = MenuView::new(true, "Fixture".into());
                        view.screen = MenuScreen::Death;
                        view.death_reason = reason.repeat(12);
                        view.death_presentation.hardcore = hardcore;
                        Arc::make_mut(&mut view.settings_options)
                            .set(option_index(EXTRA_LARGE_UI), enabled);
                        let metrics = TextMetrics::for_viewport(
                            size.map(|value| (value * dpi) as u32),
                            DpiScale::new(dpi).unwrap(),
                            Some(2),
                        );
                        let output = paint(&mut OreUiScreen::default(), &view, metrics, size);
                        let actions = if hardcore {
                            [MenuAction::DeathExitWorld, MenuAction::Respawn]
                        } else {
                            [MenuAction::Respawn, MenuAction::OpenDeathGameMenu]
                        };
                        let mut bottom = 0.0;
                        for action in actions {
                            let bounds = output
                                .hits
                                .iter()
                                .find_map(|(at, bounds)| (*at == action).then_some(*bounds))
                                .expect("settled death actions remain interactive");
                            assert!(bounds.width() > 0.0 && bounds.height() > 0.0);
                            assert!(bounds.min().x() >= 0.0 && bounds.min().y() >= bottom);
                            assert!(
                                bounds.max().x() <= size[0] && bounds.max().y() <= size[1],
                                "{action:?}: {bounds:?} at {size:?}, DPI {dpi}, enabled {enabled}"
                            );
                            assert_eq!(
                                output.hits.iter().rev().find_map(|(at, target)| {
                                    target
                                        .contains(crate::test_support::centre(bounds))
                                        .then_some(*at)
                                }),
                                Some(action)
                            );
                            bottom = bounds.max().y();
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn extra_large_death_preserves_roomy_control_and_text_scaling() {
    for (physical, gui) in [([1280, 720], 2), ([1920, 1080], 4)] {
        for dpi in [1.0, 2.0] {
            let size = physical.map(|value| value as f32 / dpi);
            let metrics =
                TextMetrics::for_viewport(physical, DpiScale::new(dpi).unwrap(), Some(gui));
            let mut view = MenuView::new(true, "Fixture".into());
            view.screen = MenuScreen::Death;
            let mut screen = OreUiScreen::default();
            let ordinary = paint(&mut screen, &view, metrics, size);
            Arc::make_mut(&mut view.settings_options).set(option_index(EXTRA_LARGE_UI), 1);
            let large = paint(&mut screen, &view, metrics, size);
            let button = |output: &Painted| {
                output
                    .hits
                    .iter()
                    .find_map(|(action, bounds)| {
                        (*action == MenuAction::Respawn).then_some(*bounds)
                    })
                    .unwrap()
            };
            let factor = (f32::from(gui) + 1.0) / f32::from(gui);
            assert!((button(&large).height() / button(&ordinary).height() - factor).abs() < 0.001);
            assert!(
                (title_height(&large, "YOU DIED!") / title_height(&ordinary, "YOU DIED!") - factor)
                    .abs()
                    < 0.01
            );
            Arc::make_mut(&mut view.settings_options).set(option_index(EXTRA_LARGE_UI), 0);
            let restored = paint(&mut screen, &view, metrics, size);
            assert_eq!(restored.hits, ordinary.hits);
            assert_eq!(
                title_height(&restored, "YOU DIED!"),
                title_height(&ordinary, "YOU DIED!")
            );
        }
    }
}

#[test]
fn extra_large_editor_keeps_rename_delete_and_cancel_reachable_in_short_windows() {
    use launcher::dressing_room::{Action, SkinEditor, SkinEditorMode, SkinEditorTarget};
    for size in [
        [400.0, 180.0],
        ui::MIN_WINDOW_LOGICAL_SIZE,
        [400.0, 250.0],
        [752.0, 500.0],
    ] {
        for dpi in [1.0, 2.0] {
            for enabled in [0, 1] {
                for target in [SkinEditorTarget::Skin, SkinEditorTarget::Cape] {
                    for mode in [SkinEditorMode::Rename, SkinEditorMode::Delete] {
                        for message in [
                            None,
                            Some("Could not save the collection. Please try again."),
                        ] {
                            let mut view = crate::test_support::dressing_room_view(1);
                            let wardrobe = Arc::make_mut(&mut view.dressing_room);
                            wardrobe.editor = Some(SkinEditor {
                                index: 0,
                                mode,
                                draft: "My look".into(),
                                target,
                            });
                            wardrobe.message = message.map(str::to_owned);
                            Arc::make_mut(&mut view.settings_options)
                                .set(option_index(EXTRA_LARGE_UI), enabled);
                            let metrics = TextMetrics::for_viewport(
                                size.map(|value| (value * dpi) as u32),
                                DpiScale::new(dpi).unwrap(),
                                Some(2),
                            );
                            let mut screen = OreUiScreen::default();
                            paint(&mut screen, &view, metrics, size);
                            let menu = screen.take_paint().unwrap();
                            let artwork = HashMap::new();
                            let (editor, _) = with_frame(size, metrics, |frame| {
                                let output = screen
                                    .paint_dressing_overlay(&menu, &view, size, &artwork, frame)
                                    .unwrap();
                                assert_eq!(frame.metrics.gui_scale, metrics.gui_scale);
                                output
                            });
                            let submit = if mode == SkinEditorMode::Rename {
                                Action::SaveRename
                            } else {
                                Action::ConfirmDelete
                            };
                            for action in [Action::Cancel, submit] {
                                let bounds = editor
                                    .hits
                                    .iter()
                                    .find_map(|(at, bounds)| {
                                        (*at == MenuAction::DressingRoom(action)).then_some(*bounds)
                                    })
                                    .expect("editor actions remain available");
                                assert!(bounds.width() > 0.0 && bounds.height() > 0.0);
                                assert!(bounds.min().x() >= 0.0 && bounds.min().y() >= 0.0);
                                assert!(
                                    bounds.max().x() <= size[0] && bounds.max().y() <= size[1],
                                    "{target:?}/{mode:?}/{action:?}: {bounds:?} at {size:?}, DPI {dpi}, enabled {enabled}"
                                );
                            }
                            if mode == SkinEditorMode::Rename {
                                let field = editor
                                    .hits
                                    .iter()
                                    .find_map(|(at, bounds)| {
                                        (*at == MenuAction::EditSkinName).then_some(*bounds)
                                    })
                                    .expect("the rename draft can still be edited");
                                assert!(
                                    field.min().x() >= 0.0
                                        && field.min().y() >= 0.0
                                        && field.max().x() <= size[0]
                                        && field.max().y() <= size[1]
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn extra_large_bed_keeps_status_clear_of_leave_and_chat_controls() {
    use crate::{BedHit, BedScreen, bed_screen::BedInput};
    for size in [
        [400.0, 120.0],
        ui::MIN_WINDOW_LOGICAL_SIZE,
        [400.0, 250.0],
        [752.0, 500.0],
    ] {
        for dpi in [1.0, 2.0] {
            for enabled in [0, 1] {
                for remote_players in [true, false] {
                    let mut settings = SettingsOptions::default();
                    settings.set(option_index(EXTRA_LARGE_UI), enabled);
                    let mut menu = OreUiScreen::default();
                    menu.configure_accessibility(&settings);
                    let ordinary = TextMetrics::for_viewport(
                        size.map(|value| (value * dpi) as u32),
                        DpiScale::new(dpi).unwrap(),
                        Some(2),
                    );
                    let metrics = menu.accessibility_metrics(ordinary);
                    let mut bed = BedScreen::default();
                    let (status_key, nodes) = with_frame(size, metrics, |frame| {
                        // Wrapping omits break whitespace from glyphs; the key retains the full input.
                        let request = frame.metrics.request(
                            "Sleeping through the night",
                            (size[0] * 64.0) as u32,
                            frame.font,
                        );
                        let status_key =
                            frame.layouts.layout(request).unwrap().key().content_sha256;
                        bed.paint(
                            BedInput {
                                asleep_for: Some(2400),
                                remote_players,
                                thunderstorm: false,
                                status: None,
                            },
                            frame,
                        )
                        .unwrap();
                        status_key
                    });
                    let status = nodes.iter().find(|node| matches!(node.visual(), ui::UiVisual::Text { layout, .. } if layout.key().content_sha256 == status_key)).expect("settled bed status remains visible").bounds();
                    assert!(
                        status.min().x() >= 0.0
                            && status.min().y() >= 0.0
                            && status.max().x() <= size[0]
                            && status.max().y() <= size[1]
                    );
                    for hit in [BedHit::LeaveBed]
                        .into_iter()
                        .chain(remote_players.then_some(BedHit::OpenChat))
                    {
                        let bounds = bed
                            .hits()
                            .iter()
                            .find_map(|(at, bounds)| (*at == hit).then_some(*bounds))
                            .expect("settled bed controls remain interactive");
                        assert!(
                            bounds.min().x() >= 0.0
                                && bounds.min().y() >= 0.0
                                && bounds.max().x() <= size[0]
                                && bounds.max().y() <= size[1]
                        );
                        assert!(
                            status.max().y() <= bounds.min().y(),
                            "{hit:?} covers the bed status at {size:?}, DPI {dpi}, enabled {enabled}: status {status:?}, button {bounds:?}"
                        );
                        assert_eq!(bed.hit(crate::test_support::centre(bounds)), Some(hit));
                    }
                }
            }
        }
    }
}
