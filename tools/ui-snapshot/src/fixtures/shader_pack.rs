//! Shader-pack picker and load feedback fixtures using the client's screen state.

use launcher::menu::{
    MenuAction, RendererChoice,
    settings_sections::SettingSection,
    shader_pack_status::{
        LOADING_TOAST_DELAY, PackLoad, PackNotices, PackToast, failure_description,
    },
};

use super::Fixture;
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

/// The Video pane with `PACK` chosen as the renderer, failed or not.
pub(super) fn settings_video(failed: bool) -> Fixture {
    let mut view = super::settings(SettingSection::Video.definition().name);
    view.render_mode = ui::RenderMode::ShaderPack;
    view.shader_packs = Arc::from([PACK.to_owned(), "Lumen".to_owned()]);
    view.renderer = Some(RendererChoice::ShaderPack(0));
    view.renderer_failure = failed.then(|| failure_description(REASON).into());
    Fixture::Menu(Box::new(view))
}

/// Scrolls Video to the renderer picker of a `shaderpacks` build listing two packs, with the
/// shader-pack folder's Open folder button a desktop host shows below it.
pub(super) fn video_picker() -> Fixture {
    use launcher::menu::settings_storage::{StorageAction, StorageFolder};
    let mut view = super::settings(SettingSection::Video.definition().name);
    view.shader_packs = Arc::from(["Dusk.zip".to_owned(), "Lumen".to_owned()]);
    Arc::make_mut(&mut view.storage).opens_folders = true;
    view.focused_action = Some(MenuAction::SettingsStorage(StorageAction::OpenFolder(
        StorageFolder::ShaderPacks,
    )));
    Fixture::Menu(Box::new(view))
}

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
