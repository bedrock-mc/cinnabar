//! The Shader Pack Settings screen built from an original fixture pack through the same
//! option session and editor the client runs.

use super::{Arc, Fixture, settings};
use launcher::menu::settings_sections::SettingSection;
use launcher::menu::{MenuAction, MenuView, RendererChoice};
use launcher::shader_options::{Action, Editor, Input};
use shaderpack::{OptionSession, OptionValues, PackSource, ShaderPack};

/// The fixture pack as the shader-pack folder would hold it.
const PACK: &str = "Fixture Pack.zip";

fn pack() -> ShaderPack {
    ShaderPack::new(PackSource::from_files([
        (
            "settings.glsl",
            include_str!("shader_options/settings.glsl"),
        ),
        ("final.fsh", include_str!("shader_options/final.fsh")),
        (
            "shaders.properties",
            include_str!("shader_options/shaders.properties"),
        ),
        ("lang/en_us.lang", include_str!("shader_options/en_us.lang")),
    ]))
}

/// An editor with `saved` values, as read from the pack's option file.
fn editor(saved: &[(&str, &str)]) -> Editor {
    let saved: OptionValues = saved
        .iter()
        .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
        .collect();
    Editor::new(OptionSession::new(&pack(), &saved), "Fixture Pack")
}

/// Video settings with the fixture pack selected and `editor`'s screen open over them.
fn view(editor: &Editor) -> MenuView {
    let mut view = selected();
    view.shader_options = Some(Arc::new(editor.view()));
    view
}

/// Video settings with the fixture pack selected.
fn selected() -> MenuView {
    let mut view = settings(SettingSection::Video.definition().name);
    view.shader_packs = Arc::from([PACK.to_owned()]);
    view.renderer = Some(RendererChoice::ShaderPack(0));
    view.render_mode = ui::RenderMode::ShaderPack;
    view.shader_options_available = true;
    view
}

/// The pointer resting on element `index` of the shown screen.
fn hover(view: &mut MenuView, index: u16, input: Input) {
    let screen = view
        .shader_options
        .as_ref()
        .map_or(0, |options| options.screen);
    view.hovered = Some(MenuAction::ShaderOptions(Action::Element {
        screen,
        index,
        input,
    }));
}

/// The Renderer row with a pack selected, scrolled to the button that opens its options.
pub(super) fn renderer_row() -> Fixture {
    let mut view = selected();
    view.focused_action = Some(MenuAction::ShaderOptions(Action::Open));
    Fixture::Menu(Box::new(view))
}

/// The main screen: the pack's banner value, profile selector and links.
pub(super) fn main_screen() -> Fixture {
    let mut view = view(&editor(&[("CLOUD_COVER", "2"), ("SHADOWS", "false")]));
    hover(&mut view, 1, Input::Next);
    Fixture::Menu(Box::new(view))
}

/// The Lighting sub-screen with saved values, an unapplied change and a hovered comment.
pub(super) fn lighting() -> Fixture {
    let mut editor = editor(&[("shadowMapResolution", "3072")]);
    editor.handle(Action::Element {
        screen: editor.view().screen,
        index: 4,
        input: Input::Next,
    });
    let screen = editor.view().screen;
    editor.handle(Action::Element {
        screen,
        index: 0,
        input: Input::Next,
    });
    editor.handle(Action::Element {
        screen: editor.view().screen,
        index: 4,
        input: Input::Next,
    });
    let mut view = view(&editor);
    hover(&mut view, 0, Input::Next);
    Fixture::Menu(Box::new(view))
}

/// The Water sub-screen, filled by `*` with the options no screen lists.
pub(super) fn rest() -> Fixture {
    let mut editor = editor(&[]);
    editor.handle(Action::Element {
        screen: editor.view().screen,
        index: 7,
        input: Input::Next,
    });
    Fixture::Menu(Box::new(view(&editor)))
}
