use super::*;
use crate::review_tests::paint;
use launcher::menu::{
    MenuView, settings_options::SettingId, settings_sections::SettingSection, updates::Status,
};
use std::collections::HashMap;

/// Paints the selected section without clipping so tests can inspect its user-visible controls.
fn render(view: &MenuView, width: f32) -> (Vec<MenuAction>, String) {
    let (_, hits, nodes) = paint(HashMap::new(), |canvas| {
        let mut content = Content {
            canvas,
            view,
            span: [0.0, width],
            column_width: width,
            y: 0.0,
            translate: &|_| None,
            nested: false,
        };
        super::super::sections::draw(
            &mut content,
            SettingSection::from_index(view.settings.section).unwrap(),
        )
        .unwrap();
    });
    let text = nodes
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
        .collect::<String>()
        .replace(' ', "");
    (hits.into_iter().map(|(action, _)| action).collect(), text)
}

/// Manual checks remain available with automatic checks off; busy or in-world clients cannot retry.
#[test]
fn check_now_respects_worker_progress_and_the_home_menu_boundary() {
    for width in [320.0, 800.0] {
        let mut view = MenuView::new(true, "Player".into());
        view.settings.section = SettingSection::Updates.definition().index;
        std::sync::Arc::make_mut(&mut view.settings_options)
            .set(SettingId::AutoUpdateEnabled.index(), 0);
        for status in [
            Status::Idle,
            Status::Checking,
            Status::Downloading,
            Status::UpToDate,
            Status::Failed,
        ] {
            view.updates.status = status;
            let (actions, text) = render(&view, width);
            assert_eq!(
                actions.contains(&MenuAction::Update(Action::CheckNow)),
                !status.is_busy()
            );
            assert!(text.contains(&installed_version().replace(' ', "")));
            assert!(text.contains(&view.updates.status_text().replace(' ', "")));
            assert!(text.contains("Beta") && text.contains("Nightly"));
        }
        view.over_world = true;
        assert!(
            !render(&view, width)
                .0
                .contains(&MenuAction::Update(Action::CheckNow))
        );
        view.updates.active = false;
        let (actions, text) = render(&view, width);
        for setting in [SettingId::ModUpdateChecks, SettingId::ModAutoUpdate] {
            assert!(actions.contains(&MenuAction::SettingsOption(setting.index() as u16, 0)));
        }
        assert!(actions.iter().all(|action| matches!(action,
            MenuAction::SettingsOption(index, _)
                if matches!(launcher::menu::settings_options::SETTINGS_OPTIONS[usize::from(*index)].id,
                    SettingId::ModUpdateChecks | SettingId::ModAutoUpdate)
        )));
        assert!(text.contains(&installed_version().replace(' ', "")));
    }
}

/// App and mod update controls belong to Updates and are absent from General.
#[test]
fn updates_controls_belong_to_their_own_settings_tab() {
    let mut view = MenuView::new(true, "Player".into());
    view.settings.section = SettingSection::Updates.definition().index;
    let (updates, _) = render(&view, 800.0);
    assert!(updates.contains(&MenuAction::Update(Action::CheckNow)));
    for setting in [SettingId::ModUpdateChecks, SettingId::ModAutoUpdate] {
        assert!(updates.contains(&MenuAction::SettingsOption(setting.index() as u16, 0)));
    }
    view.settings.section = SettingSection::General.definition().index;
    let (general, _) = render(&view, 800.0);
    assert!(!general.iter().any(|action| match action {
        MenuAction::Update(_) => true,
        MenuAction::SettingsOption(index, _) => matches!(
            launcher::menu::settings_options::SETTINGS_OPTIONS[usize::from(*index)].id,
            SettingId::UpdateChannel
                | SettingId::AutoUpdateEnabled
                | SettingId::InstallUpdatesOnQuit
                | SettingId::ModUpdateChecks
                | SettingId::ModAutoUpdate
        ),
        _ => false,
    }));
}
