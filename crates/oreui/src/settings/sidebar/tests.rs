use crate::images::{checkout_compiled_dir, load_optional_oreui_images};
use crate::paint::Originals;
use std::collections::HashMap;
use ui_engine::test_support::fixture_font;
use ui_engine::text_metrics::TextMetrics;
use {
    super::*,
    launcher::menu::{MenuAction, MenuView},
};

/// The position of the category named `section`.
fn at(section: &str) -> usize {
    TABS.iter()
        .position(|tab| tab.section == section)
        .expect("a listed category")
}

#[test]
fn installed_settings_categories_use_their_full_native_icon_cells() {
    let Some(images) = load_optional_oreui_images(&checkout_compiled_dir()) else {
        eprintln!(
            "skipping installed_settings_categories_use_their_full_native_icon_cells: compiled OreUI artwork carrier unavailable"
        );
        return;
    };
    for key in SETTINGS_ICONS {
        let sprite = images
            .sprites
            .get(key)
            .expect("native Settings icon exists");
        let [left, top, right, bottom] = sprite.bounds;
        assert_eq!([right - left, bottom - top], [24; 2], "{key}");
    }
}

#[test]
fn mods_follow_resource_packs_in_the_same_group() {
    let position = |section: &str| TABS.iter().position(|tab| tab.section == section).unwrap();
    let packs = position("global_texture_pack_forced_index");
    let mods = position(launcher::mods::SECTION);
    assert_eq!(
        mods,
        packs + 1 + usize::from(launcher::menu::settings_options::SHADER_PACK_BINDINGS)
    );
    assert!(TABS[mods].group.is_none());
    assert_ne!(section_index(launcher::mods::SECTION), 0);
}

#[test]
fn every_installed_category_icon_belongs_to_exactly_one_tab() {
    for key in SETTINGS_ICONS {
        assert_eq!(
            TABS.iter().filter(|tab| tab.art == Some(key)).count(),
            1,
            "{key}"
        );
    }
    assert!(
        TABS.iter()
            .filter_map(|tab| tab.art)
            .all(|art| SETTINGS_ICONS.contains(&art))
    );
}

#[test]
fn category_group_divider_has_two_full_width_edges_and_its_own_flow_slot() {
    use super::super::super::{review_tests::paint, test_support::solids};
    let (_, _, nodes) = paint(HashMap::new(), |canvas| {
        assert_eq!(
            group_label(canvas, "Controls", [20.0, 300.0], 100.0).unwrap(),
            150.0
        );
    });
    assert_eq!(
        solids(&nodes),
        [
            ([20.0, 146.0, 300.0, 148.0], [0, 0, 0, 102]),
            ([20.0, 148.0, 300.0, 150.0], [255, 255, 255, 26]),
        ]
    );
}

#[test]
fn category_images_keep_native_pixel_steps_at_odd_gui_scales() {
    let originals = Originals::new(0, crate::test_support::settings_icon_images());
    let font = fixture_font();
    for (gui, dpi, expected) in [
        (1, 1.0, 12.0),
        (2, 1.0, 24.0),
        (3, 1.0, 24.0),
        (4, 1.0, 48.0),
        (3, 2.0, 12.0),
    ] {
        let metrics =
            TextMetrics::for_viewport([3840, 2160], ui::DpiScale::new(dpi).unwrap(), Some(gui));
        let (mut nodes, mut next, mut layouts) = (Vec::new(), 1, ui::TextLayoutCache::new(8, 4096));
        let mut canvas = Canvas::new(
            &mut nodes,
            &mut next,
            &mut layouts,
            &font,
            metrics,
            0,
            Some(&originals),
        );
        icon(
            &mut canvas,
            &TABS[at("keyboard_and_mouse_forced_index")],
            [100.0, 100.0],
        )
        .unwrap();
        let cell = nodes.last().unwrap().bounds();
        assert_eq!(
            [
                cell.max().x() - cell.min().x(),
                cell.max().y() - cell.min().y()
            ],
            [expected; 2]
        );
    }
}

#[test]
fn category_bevels_follow_selection_hover_and_press() {
    use super::super::super::{review_tests::paint, test_support::solids};

    let tab = &TABS[at("cinnabar_discord_index")];
    let other_tab = TABS
        .iter()
        .find(|other| other.section != tab.section)
        .expect("another registered category");
    let action = MenuAction::SettingsSection(section_index(tab.section));
    for (selected, hovered, pressed, fill, edges) in [
        (false, false, false, NEUTRAL80.fill, None),
        (false, false, true, NEUTRAL80.fill, None),
        (
            false,
            true,
            false,
            NEUTRAL80.hovered,
            Some(([255, 255, 255, 26], [0, 0, 0, 102])),
        ),
        (
            false,
            true,
            true,
            NEUTRAL80.hovered,
            Some(([0, 0, 0, 204], [255, 255, 255, 26])),
        ),
        (
            true,
            false,
            false,
            NEUTRAL80.hovered,
            Some(([0, 0, 0, 102], [255, 255, 255, 26])),
        ),
        (
            true,
            true,
            true,
            NEUTRAL80.hovered,
            Some(([0, 0, 0, 102], [255, 255, 255, 26])),
        ),
    ] {
        let mut view = MenuView::new(true, "Player".into());
        view.settings.section = section_index(if selected {
            tab.section
        } else {
            other_tab.section
        });
        view.hovered = hovered.then_some(action);
        view.pressed = pressed.then_some(action);
        let mut edge = 0.0;
        let (_, hits, nodes) = paint(HashMap::new(), |canvas| {
            edge = canvas.r(EDGE);
            draw(canvas, &view, [20.0, 20.0, 420.0, 720.0], &|_| None, None).unwrap();
        });
        let row = hits.iter().find(|(found, _)| *found == action).unwrap().1;
        let [left, top, right, bottom] =
            [row.min().x(), row.min().y(), row.max().x(), row.max().y()];
        let fills = solids(&nodes);
        assert!(
            fills.contains(&([left, top, right, bottom], fill)),
            "selected={selected}, hovered={hovered}, pressed={pressed}"
        );
        let strips: Vec<_> = fills
            .into_iter()
            .filter(|(bounds, _)| {
                bounds[0] == left
                    && bounds[2] == right
                    && bounds[1] >= top
                    && bounds[3] <= bottom
                    && bounds[3] - bounds[1] <= edge
            })
            .collect();
        if let Some((upper, lower)) = edges {
            assert_eq!(
                strips,
                [
                    ([left, top, right, top + edge], upper),
                    ([left, bottom - edge, right, bottom], lower)
                ]
            );
        } else {
            assert!(strips.is_empty(), "an idle category has no divider");
        }
    }
}

#[test]
fn category_press_and_selection_keep_the_same_highlight_brightness() {
    use super::super::super::{motion::Feedback, review_tests::paint, test_support::solids};
    let color = |motion| {
        let (_, _, nodes) = paint(HashMap::new(), |canvas| {
            row_background(canvas, [20.0, 20.0, 420.0, 80.0], motion).unwrap();
        });
        solids(&nodes)[0].1
    };
    assert_eq!(
        color(Feedback {
            press: 1.0,
            ..Default::default()
        }),
        color(Feedback {
            selected: 1.0,
            ..Default::default()
        }),
    );
}

fn visible_color(draw: &ui::UiDrawList, point: [f32; 2]) -> Option<[u8; 4]> {
    let hit = ui::UiPoint::new(point[0], point[1]).unwrap();
    draw.batches
        .iter()
        .filter(|batch| batch.clip.contains(hit))
        .flat_map(|batch| {
            draw.indices[batch.index_range.start as usize..batch.index_range.end as usize]
                .as_chunks::<6>()
                .0
                .iter()
        })
        .rfind(|quad| {
            let min = std::array::from_fn::<_, 2, _>(|axis| {
                quad.iter()
                    .map(|index| draw.vertices[*index as usize].position[axis])
                    .fold(f32::INFINITY, f32::min)
            });
            let max = std::array::from_fn::<_, 2, _>(|axis| {
                quad.iter()
                    .map(|index| draw.vertices[*index as usize].position[axis])
                    .fold(f32::NEG_INFINITY, f32::max)
            });
            point[0] >= min[0] && point[0] < max[0] && point[1] >= min[1] && point[1] < max[1]
        })
        .map(|quad| draw.vertices[quad[0] as usize].color)
}

fn draw_list(nodes: Vec<ui::UiNode>) -> ui::UiDrawList {
    let mut tree = ui::UiTree::new(nodes).unwrap();
    tree.layout(
        ui::UiRect::new(
            ui::UiPoint::new(0.0, 0.0).unwrap(),
            ui::UiPoint::new(1280.0, 720.0).unwrap(),
        )
        .unwrap(),
        ui::UiScale::default(),
        ui::SafeArea::default(),
    )
    .unwrap();
    tree.build_draw_list().unwrap()
}

#[test]
fn focused_category_outline_remains_visible_over_the_following_row() {
    use super::super::super::review_tests::paint;

    let action = MenuAction::SettingsSection(section_index("global_texture_pack_forced_index"));
    for (selected, hovered) in [(true, false), (false, false), (false, true)] {
        let mut view = MenuView::new(true, "Player".into());
        view.settings.section = if selected {
            section_index("global_texture_pack_forced_index")
        } else {
            section_index(TABS[0].section)
        };
        view.focused_action = Some(action);
        view.hovered = hovered.then_some(action);
        let mut edge = 0.0;
        let (_, hits, nodes) = paint(
            HashMap::from([("oreui_settings_sidebar".into(), 120.0)]),
            |canvas| {
                edge = canvas.r(EDGE);
                draw(canvas, &view, [20.0, 20.0, 420.0, 720.0], &|_| None, None).unwrap();
            },
        );
        let row = hits.iter().find(|(found, _)| *found == action).unwrap().1;
        let overlap = edge * if selected || hovered { 2.0 } else { 1.0 };
        let centre = [
            (row.min().x() + row.max().x()) * 0.5,
            (row.min().y() + row.max().y()) * 0.5,
        ];
        let samples = [
            [centre[0], row.min().y() - overlap + edge * 0.5],
            [centre[0], row.max().y() + overlap - edge * 0.5],
            [row.min().x() + edge * 0.5, centre[1]],
            [row.max().x() - edge * 0.5, centre[1]],
        ];
        let draw = draw_list(nodes);
        for point in samples {
            assert_eq!(
                visible_color(&draw, point),
                Some(theme::OUTLINE),
                "all focus edges remain visible: selected={selected}, hovered={hovered}, point={point:?}"
            );
        }
    }
}

#[test]
fn focused_category_outline_stays_clipped_to_its_sidebar_viewport() {
    use super::super::super::review_tests::paint;

    let action = MenuAction::SettingsSection(section_index(
        TABS[at("keyboard_and_mouse_forced_index")].section,
    ));
    let mut view = MenuView::new(true, "Player".into());
    view.settings.section = section_index(TABS[at("keyboard_and_mouse_forced_index")].section);
    view.focused_action = Some(action);
    let bounds = [20.0, 20.0, 420.0, 300.0];
    let (scrolls, hits, _) = paint(HashMap::new(), |canvas| {
        draw(canvas, &view, bounds, &|_| None, None).unwrap();
    });
    let viewport = scrolls[0].viewport;
    let row = hits.iter().find(|(found, _)| *found == action).unwrap().1;
    let offset = row.min().y() - viewport.min().y();
    let mut edge = 0.0;
    let (_, hits, nodes) = paint(
        HashMap::from([("oreui_settings_sidebar".into(), offset)]),
        |canvas| {
            edge = canvas.r(EDGE);
            draw(canvas, &view, bounds, &|_| None, None).unwrap();
        },
    );
    let row = hits.iter().find(|(found, _)| *found == action).unwrap().1;
    let draw = draw_list(nodes);
    let x = row.min().x() + edge * 0.5;
    assert_eq!(
        visible_color(&draw, [x, viewport.min().y() + edge]),
        Some(theme::OUTLINE)
    );
    assert_ne!(
        visible_color(&draw, [x, viewport.min().y() - edge * 0.5]),
        Some(theme::OUTLINE),
        "the focused row's vertical border remains inside its scroll clip"
    );
}

#[test]
fn account_is_followed_by_resources_without_an_empty_category_row() {
    use crate::review_tests::paint;
    let mut view = MenuView::new(true, "Player".into());
    view.over_world = true;
    let (_, hits, _) = paint(HashMap::new(), |canvas| {
        draw(canvas, &view, [20.0, 20.0, 420.0, 1100.0], &|_| None, None).unwrap();
    });
    let row = |section| {
        hits.iter()
            .find(|(action, _)| *action == MenuAction::SettingsSection(section_index(section)))
            .unwrap()
            .1
    };
    let account = row("account_forced_index");
    let resources = row("global_texture_pack_forced_index");
    assert_eq!(account.max().y(), resources.min().y());
    let actions: Vec<_> = hits
        .iter()
        .filter_map(|(action, _)| match action {
            MenuAction::SettingsSection(index) => Some(*index),
            _ => None,
        })
        .collect();
    let shown: Vec<_> = TABS
        .iter()
        .map(|tab| section_index(tab.section))
        .filter(|&index| {
            launcher::menu::settings_sections::SettingSection::from_index(index)
                .is_some_and(|section| view.settings_section_shown(section))
        })
        .collect();
    assert_eq!(actions, shown);
}

/// Updates is visible immediately below Accessibility without scrolling.
#[test]
fn updates_is_visible_below_accessibility() {
    use launcher::menu::settings_sections::SettingSection;
    let view = MenuView::new(true, "Player".into());
    let (_, hits, _) = crate::review_tests::paint(HashMap::new(), |canvas| {
        draw(canvas, &view, [20.0, 20.0, 420.0, 400.0], &|_| None, None).unwrap();
    });
    let updates = SettingSection::Updates.definition().index;
    assert_ne!(updates, SettingSection::General.definition().index);
    assert_eq!(
        hits[0].0,
        MenuAction::SettingsSection(SettingSection::Accessibility.definition().index)
    );
    assert_eq!(hits[1].0, MenuAction::SettingsSection(updates));
    assert_eq!(hits[0].1.max().y(), hits[1].1.min().y());
}

/// The final tab remains fully clickable after scrolling to the sidebar's end.
#[test]
fn creator_remains_reachable_at_the_bottom() {
    use launcher::menu::settings_sections::SettingSection;
    let view = MenuView::new(true, "Player".into());
    let creator = MenuAction::SettingsSection(SettingSection::Creator.definition().index);
    let mut row_height = 0.0;
    let (scrolls, hits, _) = crate::review_tests::paint(
        HashMap::from([("oreui_settings_sidebar".into(), 1_000_000.0)]),
        |canvas| {
            row_height = canvas.r(4.8);
            draw(canvas, &view, [20.0, 20.0, 420.0, 400.0], &|_| None, None).unwrap();
        },
    );
    let row = hits
        .iter()
        .find(|(action, _)| *action == creator)
        .expect("Creator must be reachable at the end of the sidebar")
        .1;
    assert_eq!(hits.last().unwrap().0, creator);
    let viewport = scrolls[0].viewport;
    assert!(row.min().y() >= viewport.min().y());
    assert!(row.max().y() <= viewport.max().y());
    assert!(row.max().y() - row.min().y() >= row_height);
}

/// With Party removed, the Social group heading moves to Chat, directly followed by Discord.
#[test]
fn social_group_starts_at_chat_without_party() {
    use launcher::menu::settings_sections::SettingSection;
    let position = |section: SettingSection| {
        TABS.iter()
            .position(|tab| tab.section == section.definition().name)
    };
    assert_eq!(position(SettingSection::Party), None);
    let chat = position(SettingSection::Chat).unwrap();
    assert_eq!(TABS[chat].group, Some("options.social"));
    assert_eq!(position(SettingSection::Discord), Some(chat + 1));
}
