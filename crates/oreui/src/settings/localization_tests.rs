//! French settings text stays readable through the production label resolver.

use super::{Content, localization};
use crate::review_tests::paint;
use launcher::menu::MenuView;
use launcher::menu::settings_options::SettingId;
use std::{collections::HashMap, sync::Arc};

#[test]
fn french_option_descriptions_precede_english_catalog_fallbacks() {
    let translate = |key: &str| match key {
        "options.fullscreen.description" => Some(Arc::<str>::from("English fullscreen help")),
        _ => None,
    };
    for locale in ["fr_FR", "fr_CA", "en_US"] {
        let mut view = MenuView::new(true, "Player".into());
        assert!(Arc::make_mut(&mut view.settings_options).set_language(locale));
        paint(HashMap::new(), |canvas| {
            let content = Content {
                canvas,
                view: &view,
                span: [0.0, 1000.0],
                column_width: 1000.0,
                y: 0.0,
                translate: &translate,
                nested: false,
            };
            let (_, description) = content.option_text(SettingId::FullScreen);
            let expected = if locale.starts_with("fr") {
                "Activer ou désactiver l'affichage plein écran"
            } else {
                "English fullscreen help"
            };
            assert_eq!(description, expected, "{locale}");
        });
    }
}

#[test]
fn french_settings_text_preserves_accents_and_punctuation() {
    let cases = [
        ("options.framerateLimit.max", "Illimité"),
        ("options.fullscreen", "Plein écran"),
        ("options.cinnabar.advanced", "Avancé"),
        ("menu.accessibility.tab.tts.title", "Synthèse vocale"),
        (
            "options.cinnabar.worldsFolder.description",
            "Emplacement où vos mondes sont enregistrés",
        ),
        (
            "options.cinnabar.shaderPacksFolder.description",
            "Ajoutez des shaders Java ici ou glissez-déposez-les sur cette fenêtre",
        ),
        (
            "options.cinnabar.shaderOptions.buttonLabel",
            "Options du shader…",
        ),
        (
            "menu.keyboardAndMouse.tab.mappings.description",
            "Réattribuez toutes les touches clavier & souris pour jouer à votre façon",
        ),
        (
            "options.oreuiDarkMode.description",
            "Utiliser des surfaces sombres pour les menus, paramètres et boîtes de dialogue.",
        ),
    ];
    for locale in ["fr_FR", "fr_CA"] {
        let mut view = MenuView::new(true, "Player".into());
        assert!(Arc::make_mut(&mut view.settings_options).set_language(locale));
        paint(HashMap::new(), |canvas| {
            let content = Content {
                canvas,
                view: &view,
                span: [0.0, 1000.0],
                column_width: 1000.0,
                y: 0.0,
                translate: &|_| None,
                nested: false,
            };
            for (key, expected) in cases {
                assert_eq!(content.word(key), expected, "{locale}: {key}");
            }
        });
    }
}

#[test]
fn settings_text_preserves_catalog_and_fallback_resolution() {
    let translate = |key: &str| match key {
        "options.fullscreen" => Some(Arc::<str>::from("Vollbild")),
        "catalog.only" => Some(Arc::<str>::from("Texte du catalogue")),
        _ => None,
    };
    for language in [None, Some("en_US"), Some("de_DE")] {
        assert_eq!(
            localization::word("options.fullscreen", language, &translate),
            "Vollbild"
        );
    }
    for language in [Some("fr_FR"), Some("fr_CA")] {
        assert_eq!(
            localization::word("options.fullscreen", language, &translate),
            "Plein écran"
        );
        assert_eq!(
            localization::word("catalog.only", language, &translate),
            "Texte du catalogue"
        );
        assert_eq!(
            localization::word("options.on", language, &translate),
            "Oui"
        );
        assert_eq!(
            localization::word("options.off", language, &translate),
            "Non"
        );
        assert_eq!(
            localization::word("chat.settings.muteAll", language, &translate),
            "Mute All Chat"
        );
        assert_eq!(
            localization::word("unknown.setting", language, &translate),
            "unknown.setting"
        );
        assert_eq!(localization::word("", language, &translate), "");
    }
}
