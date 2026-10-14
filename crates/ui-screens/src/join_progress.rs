//! World and Realms join progress screens: the shown join state bound to vanilla's screens.

use json_ui::{DataSource, Scalar};

use super::menu_screens::{flags, text};
use launcher::menu::{JoinKind, JoinProgress, join_progress::shown};
use ui_engine::engine::screen::{Translate, translated};

/// Vanilla's world join progress screen.
const WORLD_SCREEN: &str = "progress.world_loading_progress_screen";
/// Vanilla's Realm join progress screen.
const REALM_SCREEN: &str = "progress.realms_stories_loading_progress_screen";

/// Binds `join`'s title, message, bar and cancel button; returns the screen to open.
pub(super) fn bind(
    join: &JoinProgress,
    data: &mut DataSource,
    translate: Translate<'_>,
) -> &'static str {
    let tr = |key: &str, fallback: &str| translated(translate, key, fallback);
    let shown = shown(join, &tr);
    data.set_global("#title_text", text(shown.title));
    data.set_global("#progress_text", text(shown.message));
    match shown.clipped {
        Some(clipped) => {
            flags(data, &["#loading_bar_visible"]);
            // The bar binds this as its `#clip_ratio`.
            data.set_global("#loading_bar_percentage", Scalar::Num(clipped));
        }
        None => flags(data, &["#bar_animation_visible"]),
    }
    if shown.cancel {
        flags(data, &["#cancel_visible"]);
        data.set_global("#cancel_button_text", text(tr("gui.cancel", "Cancel")));
    }
    match join.kind {
        JoinKind::Realm => REALM_SCREEN,
        JoinKind::External | JoinKind::Local => WORLD_SCREEN,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use launcher::menu::JoinStage;

    // Realm joins open the Realms screen; every other join opens the world screen.
    #[test]
    fn realm_joins_open_the_realms_screen() {
        let connecting = JoinProgress::new(JoinKind::External);
        let mut realm = JoinProgress::new(JoinKind::Realm);
        realm.observe(Some(JoinStage::Realm));
        let screen = |join: &JoinProgress| bind(join, &mut DataSource::new(), &|_| None);
        assert_eq!(screen(&realm), REALM_SCREEN);
        assert_eq!(screen(&connecting), WORLD_SCREEN);
    }
}
