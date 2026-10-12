//! The OreUI account picker drawn through the presentation runtime.

#[cfg(test)]
mod tests {
    use super::super::{
        pack_harness::{drawn_texts, engine_presentation, menu_nodes},
        test_support::draw_menu_actions,
    };
    use launcher::menu::{MenuAction, MenuScreen, auth::AuthState};
    use launcher::test_support::saved_accounts_view;

    #[test]
    fn account_picker_draws_cached_gamerpic_pixels() {
        let Some(mut presentation) = engine_presentation() else {
            eprintln!(
                "skipping account gamerpic rendering test: missing local UI carrier; make assets"
            );
            return;
        };
        let dir = std::env::temp_dir().join(format!("cinnabar-gamerpic-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let picture = dir.join("picture.png");
        if let Some(source) = std::env::var_os("CINNABAR_GAMERPIC_FIXTURE") {
            if let Err(error) = std::fs::copy(&source, &picture) {
                if error.kind() == std::io::ErrorKind::NotFound {
                    eprintln!(
                        "skipping account gamerpic rendering test: missing fixture {}",
                        std::path::Path::new(&source).display()
                    );
                    std::fs::remove_dir_all(dir).unwrap();
                    return;
                }
                panic!("copy account gamerpic fixture: {error}");
            }
        } else {
            image::RgbaImage::from_pixel(2, 2, image::Rgba([170, 40, 80, 255]))
                .save(&picture)
                .unwrap();
        }
        let mut view = saved_accounts_view();
        let path = picture.to_string_lossy().into_owned();
        view.feeds.accounts[0].picture_path = Some(path.clone());
        presentation.sync_menu_artwork(super::super::super::menu_artwork::view_paths(&view));
        presentation.finish_menu_artwork();
        let icon = presentation
            .menu_artwork_icon(&path)
            .expect("decoded cached gamerpic");
        draw_menu_actions(&player_state::PlayerState::new(1), &mut presentation, &view);
        assert!(menu_nodes(&presentation).iter().any(|node| matches!(node.visual(), ui::UiVisual::Sprite { texture_page, uv, .. } if *texture_page == icon.page && *uv == icon.uv)), "account picture must sample its decoded artwork");
        let input = presentation.last_input.as_ref().unwrap();
        super::super::snapshot::write(input, "account-picker-pixels");
        if std::env::var_os("CINNABAR_GAMERPIC_FIXTURE").is_none() {
            let pixels = super::super::snapshot::rasterize(input);
            assert!(
                pixels
                    .pixels()
                    .filter(|pixel| pixel.0 == [170, 40, 80, 255])
                    .count()
                    > 100,
                "decoded account picture must remain visible after compositing"
            );
        }
        view.dialog = None;
        draw_menu_actions(&player_state::PlayerState::new(1), &mut presentation, &view);
        assert!(menu_nodes(&presentation).iter().any(|node| matches!(node.visual(), ui::UiVisual::Sprite { texture_page, uv, .. } if *texture_page == icon.page && *uv == icon.uv)), "Home profile must sample the cached Xbox picture");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn account_popup_only_exposes_saved_switch_and_manager_actions() {
        let Some(mut presentation) = engine_presentation() else {
            eprintln!("skipping account popup action test: missing local UI carrier; make assets");
            return;
        };
        let actions = draw_menu_actions(
            &player_state::PlayerState::new(1),
            &mut presentation,
            &saved_accounts_view(),
        );
        let texts = drawn_texts(menu_nodes(&presentation));
        assert!(texts.iter().any(|text| text.contains("First")), "{texts:?}");
        assert!(
            texts.iter().any(|text| text.contains("Second")),
            "{texts:?}"
        );
        assert!(
            actions.contains(&MenuAction::SwitchAccount(1)),
            "{actions:?}"
        );
        assert!(
            !actions.contains(&MenuAction::SwitchAccount(0)),
            "current account cannot switch to itself"
        );
        for action in [
            MenuAction::AddAccount,
            MenuAction::Navigate(MenuScreen::Profile),
            MenuAction::DismissDialog,
        ] {
            assert!(actions.contains(&action), "missing {action:?}: {actions:?}");
        }
        assert!(
            !actions.contains(&MenuAction::Navigate(MenuScreen::Play)),
            "popup blocks the main menu"
        );
    }

    #[test]
    fn pending_sign_in_blocks_switching_and_closing_cancels_it() {
        let Some(mut presentation) = engine_presentation() else {
            eprintln!("skipping pending account popup test: missing local UI carrier; make assets");
            return;
        };
        let mut view = saved_accounts_view();
        view.feeds.account_adding = true;
        view.auth_state = AuthState::AwaitingCode {
            uri: "https://example.invalid/link".into(),
            code: "TEST".into(),
        };
        let actions =
            draw_menu_actions(&player_state::PlayerState::new(1), &mut presentation, &view);
        let texts = drawn_texts(menu_nodes(&presentation));
        assert!(
            !texts
                .iter()
                .any(|text| text.contains("TEST") || text.contains("example.invalid")),
            "{texts:?}"
        );
        assert!(actions.contains(&MenuAction::OpenSignInLink), "{actions:?}");
        assert!(actions.contains(&MenuAction::CancelSignIn), "{actions:?}");
        assert!(!actions.iter().any(|action| matches!(
            action,
            MenuAction::SwitchAccount(_) | MenuAction::AddAccount | MenuAction::DismissDialog
        )));
    }

    #[test]
    fn xbox_signup_shows_browser_actions_without_exposing_the_signed_url() {
        let Some(mut presentation) = engine_presentation() else {
            eprintln!("skipping Xbox signup popup test: missing local UI carrier; make assets");
            return;
        };
        let player = player_state::PlayerState::new(1);
        let mut view = saved_accounts_view();
        view.feeds.account_adding = true;
        view.auth_state = AuthState::AwaitingCode {
            uri: "https://example.invalid/link".into(),
            code: "TEST".into(),
        };
        draw_menu_actions(&player, &mut presentation, &view);
        super::super::snapshot::write(
            presentation.last_input.as_ref().unwrap(),
            "xbox-signup-before",
        );
        view.auth_state = AuthState::AwaitingXboxSignup {
            uri: "https://sisu.xboxlive.com/signup?signature=private-fixture".into(),
        };
        for (browser, name) in [
            (
                launcher::menu::sign_in::BrowserState::Opened,
                "xbox-signup-opened",
            ),
            (
                launcher::menu::sign_in::BrowserState::Failed,
                "xbox-signup-browser-failed",
            ),
        ] {
            view.sign_in_browser = browser;
            let actions = draw_menu_actions(&player, &mut presentation, &view);
            let texts = drawn_texts(menu_nodes(&presentation));
            assert!(
                texts.iter().any(|text| text.contains("Xbox profile")),
                "{texts:?}"
            );
            assert!(
                !texts
                    .iter()
                    .any(|text| text.contains("private-fixture")
                        || text.contains("sisu.xboxlive.com")),
                "{texts:?}"
            );
            assert!(actions.contains(&MenuAction::OpenSignInLink));
            assert!(actions.contains(&MenuAction::CancelSignIn));
            super::super::snapshot::write(presentation.last_input.as_ref().unwrap(), name);
        }
    }
}
