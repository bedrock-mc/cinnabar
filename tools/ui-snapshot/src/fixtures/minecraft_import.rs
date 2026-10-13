//! Anonymous settings and packs for the import modal captures.

use super::{Arc, Fixture, MenuView};
use launcher::{
    menu::MenuDialog,
    minecraft_import::{
        Phase,
        options::{MAPPINGS, ParsedOptions},
    },
};

/// Populates every supported preference without reading an installed game profile.
fn options() -> ParsedOptions {
    ParsedOptions {
        values: MAPPINGS
            .iter()
            .map(|mapping| {
                let definition = mapping.destination.definition();
                (mapping.destination.name().to_owned(), definition.default)
            })
            .collect(),
        bindings: launcher::menu::settings_options::KEY_BINDINGS
            .iter()
            .map(|(_, key)| (key.to_string(), None))
            .chain(
                launcher::menu::settings_options::EXTRA_KEYS
                    .iter()
                    .map(|(key, _)| (key.to_string(), None)),
            )
            .collect(),
        language: Some("en_US".into()),
        fullscreen: Some(false),
        ..Default::default()
    }
}

/// Shows a populated review using safe profile, server and pack display names.
pub(super) fn preview() -> Fixture {
    let mut view = MenuView::new(true, "Player".into());
    view.dialog = Some(MenuDialog::MinecraftImport);
    view.focused_action = None;
    let state = Arc::make_mut(&mut view.minecraft_import);
    state.phase = Phase::Preview;
    let options = options();
    state.body = format!(
        "Minecraft profile\n\nSettings ({})\n{}\n\nNew servers (2)\nCreative, Survival\n\nResource packs (3)\nClear Glass, Soft Colors, Simple Icons\n\nActive packs, highest priority first (2)\nSoft Colors, Clear Glass\n\nSkipped: 469 unsupported, duplicate or unavailable entries.\n\nRevert later in Settings; copied packs stay installed.",
        options.count(),
        options.summary()
    );
    Fixture::Menu(Box::new(view))
}

/// Shows the second consent step while retaining the original reviewed preview.
pub(super) fn confirmation() -> Fixture {
    let Fixture::Menu(mut view) = preview() else {
        unreachable!()
    };
    Arc::make_mut(&mut view.minecraft_import).phase = Phase::ConfirmImport;
    Fixture::Menu(view)
}
