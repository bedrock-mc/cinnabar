use super::*;
use shaderpack::{OptionSession, OptionValues, PackSource, ShaderPack};

/// GLOW (switch, on), QUALITY (1–3, 2), DEPTH (slider 1–4, 2), a LIGHT sub-screen and two
/// profiles.
fn editor(saved: &[(&str, &str)]) -> Editor {
    let pack = ShaderPack::new(PackSource::from_files([
        (
            "settings.glsl",
            "#define GLOW\n#define QUALITY 2 // [1 2 3]\n#define DEPTH 2 // [1 2 3 4]\n",
        ),
        ("final.fsh", "#ifdef GLOW\n#endif\nvoid main() {}\n"),
        (
            "shaders.properties",
            "screen=<profile> [LIGHT] <empty> QUALITY\nscreen.LIGHT=GLOW DEPTH\n\
             sliders=DEPTH\nprofile.FAST=!GLOW QUALITY=1\nprofile.FANCY=QUALITY=3\n",
        ),
        (
            "lang/en_US.lang",
            "screen.LIGHT=§eLighting\noption.QUALITY=Quality\noption.QUALITY.comment=Detail.\n\
             value.QUALITY.3=High\nprofile.FAST=Fast\n",
        ),
    ]));
    let saved: OptionValues = saved
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect();
    Editor::new(OptionSession::new(&pack, &saved), "Dusk")
}

fn press(editor: &mut Editor, index: u16, input: Input) -> Outcome {
    let screen = editor.view().screen;
    editor.handle(Action::Element {
        screen,
        index,
        input,
    })
}

#[test]
fn the_main_screen_shows_the_packs_layout_and_text() {
    let view = editor(&[]).view();
    assert_eq!(
        (view.title.as_str(), view.nested, view.columns),
        ("Dusk", false, 2)
    );
    assert_eq!(view.status, Status::Ready);
    let controls: Vec<_> = view.elements.iter().map(|e| e.control.clone()).collect();
    assert_eq!(
        controls,
        [
            Control::Profile {
                value: "Custom".to_owned()
            },
            Control::Link,
            Control::Empty,
            Control::Choice {
                value: "2".to_owned()
            },
        ]
    );
    assert_eq!(view.elements[1].label, "§eLighting");
    assert_eq!(view.elements[3].label, "Quality");
    assert_eq!(view.elements[3].comment.as_deref(), Some("Detail."));
}

#[test]
fn presses_edit_values_and_apply_saves_only_changes() {
    let mut editor = editor(&[]);
    press(&mut editor, 3, Input::Next);
    let view = editor.view();
    assert_eq!(
        view.elements[3].control,
        Control::Choice {
            value: "High".to_owned()
        }
    );
    assert!(view.elements[3].modified && view.changed);
    assert_eq!(
        view.elements[0].control,
        Control::Profile {
            value: "FANCY".to_owned()
        },
        "matching values select a profile"
    );
    let saved = editor.handle(Action::Apply);
    assert_eq!(
        saved,
        Outcome {
            save: Some(OptionValues::from([("QUALITY".to_owned(), "3".to_owned())])),
            close: false,
        }
    );
    assert!(!editor.view().changed && !editor.view().elements[3].modified);
    assert_eq!(
        editor.handle(Action::Done),
        Outcome {
            save: None,
            close: true
        }
    );
}

#[test]
fn links_open_sub_screens_and_back_returns() {
    let mut editor = editor(&[("DEPTH", "4")]);
    let main = editor.view().screen;
    press(&mut editor, 1, Input::Next);
    let view = editor.view();
    assert_eq!((view.title.as_str(), view.nested), ("§eLighting", true));
    assert_ne!(view.screen, main);
    assert_eq!(view.elements[0].control, Control::Switch { on: true });
    assert_eq!(
        view.elements[1].control,
        Control::Slider {
            value: "4".to_owned(),
            index: 3,
            count: 4
        }
    );
    editor.handle(Action::Element {
        screen: main,
        index: 3,
        input: Input::Next,
    });
    assert!(
        !editor.view().changed,
        "a press from the old screen is dropped"
    );
    press(&mut editor, 1, Input::Select(0));
    press(&mut editor, 0, Input::Previous);
    let view = editor.view();
    assert_eq!(view.elements[0].control, Control::Switch { on: false });
    assert_eq!(
        view.elements[1].control,
        Control::Slider {
            value: "1".to_owned(),
            index: 0,
            count: 4
        }
    );
    assert_eq!(editor.handle(Action::Back), Outcome::default());
    assert_eq!(editor.view().title, "Dusk");
    let closed = editor.handle(Action::Back);
    assert!(closed.close, "Back on the main screen closes, applying");
    assert_eq!(
        closed.save,
        Some(OptionValues::from([
            ("DEPTH".to_owned(), "1".to_owned()),
            ("GLOW".to_owned(), "false".to_owned()),
        ]))
    );
}

#[test]
fn profiles_cancel_and_reset() {
    let mut editor = editor(&[("QUALITY", "3")]);
    press(&mut editor, 0, Input::Next);
    assert_eq!(
        editor.view().elements[0].control,
        Control::Profile {
            value: "Fast".to_owned()
        }
    );
    assert_eq!(
        editor.handle(Action::Cancel),
        Outcome {
            save: None,
            close: true
        }
    );
    assert!(!editor.view().changed);
    assert_eq!(
        editor.handle(Action::Reset),
        Outcome {
            save: Some(OptionValues::new()),
            close: false
        }
    );
    assert_eq!(
        editor.view().elements[3].control,
        Control::Choice {
            value: "2".to_owned()
        }
    );
}

#[test]
fn a_value_without_alternatives_is_a_heading() {
    let pack = ShaderPack::new(PackSource::from_files([
        ("settings.glsl", "#define INFO 1 // [1]\n"),
        ("shaders.properties", "screen=INFO\n"),
        (
            "lang/en_us.lang",
            "option.INFO=Dusk\nvalue.INFO.1=by someone\n",
        ),
    ]));
    let editor = Editor::new(OptionSession::new(&pack, &OptionValues::new()), "Dusk");
    assert_eq!(
        editor.view().elements[0].control,
        Control::Fixed {
            value: "by someone".to_owned()
        }
    );
}
