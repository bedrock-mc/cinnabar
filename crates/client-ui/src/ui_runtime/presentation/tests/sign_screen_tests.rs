//! The sign editor through vanilla `sign_screen.json`: the wood's art, the
//! lines with a caret, and a press outside the sign closing it.

use json_ui::Draw;
use world::{NbtCompound, NbtValue};

use super::engine_hud_tests::{engine_presentation, engine_presentation_with};
use super::*;
use crate::ui_runtime::sign_editor::SignEdit;

fn open_sign(runtime: &mut UiRuntime, block: &str) {
    let mut face = NbtCompound::default();
    face.insert("Text", NbtValue::String("Hello\nWorld".into()));
    let mut root = NbtCompound::default();
    root.insert("FrontText", NbtValue::Compound(face));
    runtime
        .sign_editor_mut()
        .open(SignEdit::new([0, 64, 0], true, root).with_block(Some(block)));
}

#[test]
fn sign_screen_shows_the_woods_art_and_the_lines_with_a_caret() {
    let player_runtime = player_state::PlayerState::new(1);

    let Some(mut presentation) = engine_presentation() else {
        eprintln!(
            "skipping sign_screen_shows_the_woods_art_and_the_lines_with_a_caret: fixture unavailable; requires installed local carriers (make assets)"
        );
        return;
    };
    let mut runtime = UiRuntime::new(1);
    open_sign(&mut runtime, "minecraft:birch_standing_sign");
    presentation
        .build(
            &player_runtime,
            &runtime,
            0,
            [1280, 720],
            DpiScale::new(1.0).unwrap(),
        )
        .unwrap();
    let nodes = presentation.sign_draw_nodes();
    assert!(nodes.iter().any(|node| matches!(
        &node.draw,
        Draw::Sprite { texture, .. } if texture == "textures/ui/sign_birch"
    )));
    let text: String = nodes
        .iter()
        .filter_map(|node| match &node.draw {
            Draw::Text { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert!(text.contains("|Hello"), "caret at the start: {text:?}");
    assert!(text.contains("World"));
    assert!(presentation.sign_editor_exit_hit(UiPoint::new(10.0, 10.0).unwrap()));
    assert!(!presentation.sign_editor_exit_hit(UiPoint::new(640.0, 360.0).unwrap()));
    runtime.sign_editor_mut().close();
    presentation
        .build(
            &player_runtime,
            &runtime,
            0,
            [1280, 720],
            DpiScale::new(1.0).unwrap(),
        )
        .unwrap();
    assert!(!presentation.sign_editor_exit_hit(UiPoint::new(10.0, 10.0).unwrap()));
}

/// The edited lines draw in the face's own text colour, black when the sign stores none.
#[test]
fn sign_screen_draws_the_lines_in_the_faces_text_color() {
    let player_runtime = player_state::PlayerState::new(1);
    let Some(mut presentation) = engine_presentation() else {
        eprintln!(
            "skipping sign_screen_draws_the_lines_in_the_faces_text_color: fixture unavailable; requires installed local carriers (make assets)"
        );
        return;
    };
    // Signed ARGB, as `SignTextColor` stores it.
    let red = i32::from_be_bytes([0xff, 0xb0, 0x2e, 0x26]);
    for (stored, expected) in [(None, [0, 0, 0, 255]), (Some(red), [0xb0, 0x2e, 0x26, 255])] {
        let mut face = NbtCompound::default();
        face.insert("Text", NbtValue::String("Hello\nWorld".into()));
        if let Some(argb) = stored {
            face.insert("SignTextColor", NbtValue::Int(argb));
        }
        let mut root = NbtCompound::default();
        root.insert("FrontText", NbtValue::Compound(face));
        let mut runtime = UiRuntime::new(1);
        runtime.sign_editor_mut().open(
            SignEdit::new([0, 64, 0], true, root).with_block(Some("minecraft:standing_sign")),
        );
        presentation
            .build(
                &player_runtime,
                &runtime,
                0,
                [1280, 720],
                DpiScale::new(1.0).unwrap(),
            )
            .unwrap();
        let colors: Vec<[u8; 4]> = presentation
            .sign_draw_nodes()
            .iter()
            .filter_map(|node| match &node.draw {
                Draw::Text { text, color, .. } if text.contains("World") => Some(*color),
                _ => None,
            })
            .collect();
        assert!(
            !colors.is_empty() && colors.iter().all(|color| *color == expected),
            "stored colour {stored:?} drew {colors:?}"
        );
    }
}

/// Local-only: writes `sign_screen.png` when `CINNABAR_FORM_SNAPSHOT_DIR` is set.
#[test]
fn sign_screen_snapshot() {
    let player_runtime = player_state::PlayerState::new(1);

    let Some(mut presentation) =
        engine_presentation_with(super::super::forms::pack_harness::font())
    else {
        eprintln!(
            "skipping sign_screen_snapshot: fixture unavailable; requires installed local carriers (make assets)"
        );
        return;
    };
    let mut runtime = UiRuntime::new(1);
    open_sign(&mut runtime, "minecraft:dark_oak_hanging_sign");
    let input = presentation
        .build(
            &player_runtime,
            &runtime,
            0,
            [1280, 720],
            DpiScale::new(1.0).unwrap(),
        )
        .unwrap();
    super::super::forms::snapshot::write(&input, "sign_screen");
}
