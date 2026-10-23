//! Search regressions use the production painter and real menu actions.

use super::*;
use crate::test_support::settings_view;
use launcher::menu::settings_options::SettingId;
use std::collections::HashMap;

/// Paints a full settings screen with translated labels, retaining all published controls.
fn actions(query: &str, translate: Translate<'_>) -> Vec<MenuAction> {
    let mut view = settings_view("accessibility_forced_index");
    view.settings_query = query.to_owned();
    let (_, hits, _) = crate::review_tests::paint(HashMap::new(), |canvas| {
        super::super::draw(
            canvas,
            &view,
            [1280.0, 20000.0],
            translate,
            None,
            &mut Default::default(),
        )
        .unwrap();
    });
    hits.into_iter().map(|(action, _)| action).collect()
}

#[test]
fn search_finds_controls_across_categories_and_deduplicates_shared_rows() {
    for query in ["vsync", "  VSYNC  "] {
        let actions = actions(query, &|_| None);
        let mut vsync_actions: Vec<_> = actions.iter().copied().filter(|action| matches!(action,
            MenuAction::SettingsOption(index, _) if usize::from(*index) == SettingId::Vsync.index()
        )).collect();
        vsync_actions.dedup();
        assert_eq!(
            vsync_actions,
            [MenuAction::SettingsOption(
                SettingId::Vsync.index() as u16,
                1 - launcher::menu::settings_options::VSYNC_OPTION.default,
            )]
        );
        assert!(!actions.iter().any(|action| matches!(action, MenuAction::SettingsOption(index, _) if usize::from(*index) == SettingId::MainVolume.index())));
        assert!(actions.contains(&MenuAction::SearchSettings));
        assert!(actions.contains(&MenuAction::ClearSettingsSearch));
    }
}

#[test]
fn search_matches_translations_help_text_and_unordered_terms() {
    let translate = |key: &str| match key {
        "options.vsync" => Some("Synchronisation verticale".into()),
        "options.vsync.description" => Some("Limiter les images".into()),
        _ => None,
    };
    for query in ["VERTICALE synchronisation", "limiter images"] {
        assert!(actions(query, &translate).iter().any(|action| matches!(action, MenuAction::SettingsOption(index, _) if usize::from(*index) == SettingId::Vsync.index())));
    }
}

#[test]
fn search_exposes_bindings_and_category_navigation() {
    let jump = KEY_BINDINGS
        .iter()
        .position(|(_, label)| *label == "key.jump")
        .unwrap();
    assert!(actions("jump", &|_| None).contains(&MenuAction::SettingsKey(jump as u16)));
    assert!(
        actions("language", &|_| None).contains(&MenuAction::SettingsSearchSection(
            SettingSection::Language.definition().index
        ))
    );
}

#[test]
fn no_matches_keeps_the_editor_and_clear_action_without_unrelated_controls() {
    let actions = actions("no-such-setting-xyz", &|_| None);
    assert!(actions.contains(&MenuAction::SearchSettings));
    assert!(actions.contains(&MenuAction::ClearSettingsSearch));
    assert!(!actions.iter().any(|action| matches!(
        action,
        MenuAction::SettingsOption(..) | MenuAction::SettingsKey(_)
    )));
}

#[test]
fn whitespace_query_keeps_the_selected_category() {
    let actions = actions("  ", &|_| None);
    assert!(actions.contains(&MenuAction::SettingsResetGroup(
        launcher::menu::settings_options::SettingsGroup::Accessibility
    )));
}

/// The declared category ownership must agree with the normal settings controls.
#[test]
fn panel_categories_match_the_controls_rendered_by_each_section() {
    for section in SECTION_DEFINITIONS
        .iter()
        .filter(|section| section.tab_title.is_some())
    {
        let mut view = settings_view(section.name);
        view.touch_available = true;
        view.updates.active = true;
        let (_, hits, _) = crate::review_tests::paint(HashMap::new(), |canvas| {
            super::super::draw(
                canvas,
                &view,
                [1280.0, 50000.0],
                &|_| None,
                None,
                &mut Default::default(),
            )
            .unwrap();
        });
        for (action, _) in hits {
            let index = match action {
                MenuAction::SettingsOption(index, _) | MenuAction::SettingsDropdown(index) => index,
                _ => continue,
            };
            let option = &SETTINGS_OPTIONS[usize::from(index)];
            assert!(
                option
                    .section
                    .iter()
                    .any(|(panel, _)| panel.section() == section.id),
                "{:?} is displayed in {:?}",
                option.id,
                section.id
            );
        }
    }
}
