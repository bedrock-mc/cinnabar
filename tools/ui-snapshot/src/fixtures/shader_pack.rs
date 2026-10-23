//! Shader-pack picker and load feedback fixtures using the client's screen state.

use launcher::menu::{
    MenuAction, MenuView, RendererChoice,
    settings_options::SettingId,
    settings_sections::SettingSection,
    shader_pack_status::{
        LOADING_TOAST_DELAY, PackLoad, PackNotices, PackToast, failure_description,
    },
};

use super::{Fixture, GUI_SCALE, SHADER_PACK_ROW_SIZE, SIZE, State};
use std::sync::Arc;

const PACK: &str = "Dusk.zip";
/// A load failure as the render host reports it: one failed program per line.
const REASON: &str = "composite: composite.fsh:42: parse: Unknown variable: bloomTint\n\
                      final: final.fsh:17: parse: Unknown function 'tonemapFilmic'";

/// The toast the notices raise as the load goes through `loads`, each a frame apart once the
/// loading toast may stand.
fn toast(loads: &[PackLoad<'_>]) -> Fixture {
    let mut notices = PackNotices::default();
    let mut raised = None;
    for (frame, load) in (0u32..).zip(loads) {
        let now = LOADING_TOAST_DELAY * frame;
        raised = notices.observe(*load, now, true).or(raised);
    }
    let (title, message) = match raised.expect("the loads raise a toast") {
        PackToast::Loading { title, message } | PackToast::Outcome { title, message } => {
            (title, message)
        }
        PackToast::Withdraw => unreachable!("fixtures end on a shown toast"),
    };
    Fixture::Toast { title, message }
}

pub(super) fn loading() -> Fixture {
    let loading = PackLoad::Loading {
        pack: PACK,
        programs: Some((23, 61)),
    };
    toast(&[loading, loading])
}

pub(super) fn loaded() -> Fixture {
    toast(&[
        PackLoad::Loading {
            pack: PACK,
            programs: None,
        },
        PackLoad::Ready { pack: PACK },
    ])
}

pub(super) fn failed() -> Fixture {
    toast(&[
        PackLoad::Loading {
            pack: PACK,
            programs: None,
        },
        PackLoad::Failed {
            pack: PACK,
            reason: REASON,
        },
    ])
}

/// The Shaders tab while the render host is still loading the selected pack.
pub(super) fn settings_shaders_loading() -> Fixture {
    let Fixture::Menu(mut view) = settings_shaders(false) else {
        unreachable!()
    };
    view.shader_pack_loading = true;
    Fixture::Menu(view)
}

/// The Shaders tab with `PACK` chosen as the renderer, failed or not.
pub(super) fn settings_shaders(failed: bool) -> Fixture {
    let mut view = super::settings(SettingSection::Shaders.definition().name);
    view.render_mode = ui::RenderMode::ShaderPack;
    view.shader_pack = Some(PACK.to_owned());
    view.shader_packs = Arc::from([PACK.to_owned(), "Lumen".to_owned()]);
    view.renderer = Some(RendererChoice::ShaderPack(0, view.shader_packs_revision));
    view.shader_options_available = true;
    view.renderer_failure = failed.then(|| failure_description(REASON).into());
    Fixture::Menu(Box::new(view))
}

/// The Shaders tab with two packs and the folder import control.
pub(super) fn pack_picker() -> Fixture {
    let mut view = super::settings(SettingSection::Shaders.definition().name);
    view.shader_packs = Arc::from(["Dusk.zip".to_owned(), "Lumen".to_owned()]);
    desktop(&mut view);
    view.focused_action = Some(MenuAction::SettingsSection(
        SettingSection::Shaders.definition().index,
    ));
    Fixture::Menu(Box::new(view))
}

/// Shows first-use import guidance without installed packs or a saved selection.
pub(super) fn empty_library() -> Fixture {
    let mut view = super::settings(SettingSection::Shaders.definition().name);
    desktop(&mut view);
    Fixture::Menu(Box::new(view))
}

/// The empty library on a host like Android, whose window takes no dropped files and has no
/// file manager, so the import target opens the file picker.
pub(super) fn empty_library_without_drops() -> Fixture {
    Fixture::Menu(Box::new(super::settings(
        SettingSection::Shaders.definition().name,
    )))
}

/// A desktop host's folder shortcut and window file drops.
fn desktop(view: &mut MenuView) {
    let storage = Arc::make_mut(&mut view.storage);
    storage.opens_folders = true;
    storage.accepts_file_drops = true;
}

/// Exercises wrapping and scrolling in a narrow viewport, with a selected card and a zip and
/// folder that share a name.
pub(super) fn long_library() -> Fixture {
    let mut view = super::settings(SettingSection::Shaders.definition().name);
    view.shader_packs =
        std::iter::once("Evening Light 0 with a long descriptive release name".to_owned())
            .chain((0..16).map(|index| {
                format!("Evening Light {index} with a long descriptive release name.zip")
            }))
            .collect();
    view.render_mode = ui::RenderMode::ShaderPack;
    view.shader_pack = Some(view.shader_packs[0].clone());
    view.renderer = Some(RendererChoice::ShaderPack(0, view.shader_packs_revision));
    view.shader_options_available = true;
    Fixture::Menu(Box::new(view))
}

/// The failed selected card with every control at the narrowest settings width, enlarged.
pub(super) fn narrow_extra_large() -> Fixture {
    let Fixture::Menu(mut view) = settings_shaders(true) else {
        unreachable!()
    };
    desktop(&mut view);
    let enlarged = Arc::make_mut(&mut view.settings_options)
        .set(SettingId::GuiAccessibilityScaling.index(), 1);
    debug_assert!(enlarged);
    Fixture::Menu(view)
}

/// Shows the standard confirmation before deleting an imported shader pack.
pub(super) fn removal_dialog() -> Fixture {
    let Fixture::Menu(mut view) = settings_shaders(false) else {
        unreachable!()
    };
    view.dialog = Some(launcher::menu::MenuDialog::RemoveShaderPack(
        0,
        view.shader_packs_revision,
    ));
    view.focused_action = Some(MenuAction::DismissDialog);
    Fixture::Menu(view)
}

/// Frames the Video shortcut above the graphics controls so its full row is visible.
pub(super) fn video_shortcut() -> Fixture {
    let Fixture::Menu(mut view) = settings_shaders(false) else {
        unreachable!()
    };
    view.settings.section = SettingSection::Video.definition().index;
    view.focused_action = Some(MenuAction::SettingsOption(
        SettingId::GraphicsMode.index() as u16,
        0,
    ));
    Fixture::Menu(view)
}
/// Shows an ordinary OreUI route without account, world or network services.
pub(super) fn settings_video() -> Fixture {
    Fixture::Menu(Box::new(super::settings(
        SettingSection::Video.definition().name,
    )))
}

/// Shader-pack load toasts and the library's selected, loading and failed states.
pub(super) const STATES: &[State] = &[
    State {
        name: "settings-video",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: settings_video,
    },
    State {
        name: "toast-shader-pack-loading",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: loading,
    },
    State {
        name: "toast-shader-pack-loaded",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: loaded,
    },
    State {
        name: "toast-shader-pack-failed",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: failed,
    },
    State {
        name: "settings-shaders-selected",
        size: SHADER_PACK_ROW_SIZE,
        gui_scale: GUI_SCALE,
        fixture: || settings_shaders(false),
    },
    State {
        name: "settings-shaders-narrow-extra-large",
        size: [240, 2400],
        gui_scale: GUI_SCALE,
        fixture: narrow_extra_large,
    },
    State {
        name: "settings-shaders-loading",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: settings_shaders_loading,
    },
    State {
        name: "settings-shaders-failed",
        size: SHADER_PACK_ROW_SIZE,
        gui_scale: GUI_SCALE,
        fixture: || settings_shaders(true),
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_toast_fixture_shows_its_stage() {
        let title = |fixture| match fixture {
            Fixture::Toast { title, .. } => title,
            _ => unreachable!(),
        };
        assert!(title(loading()).starts_with("Loading"));
        assert!(title(loaded()).ends_with("loaded"));
        assert!(title(failed()).ends_with("failed to load"));
    }
}
