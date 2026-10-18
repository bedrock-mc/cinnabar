use super::*;

fn sign(text: &str) -> SignEdit {
    let mut face = NbtCompound::default();
    face.insert("Text", NbtValue::String(text.into()));
    face.insert("SignTextColor", NbtValue::Int(-0x00ff_ff01));
    let mut root = NbtCompound::default();
    root.insert("FrontText", NbtValue::Compound(face));
    SignEdit::new([1, 2, 3], true, root)
}

#[test]
fn existing_text_and_color_load_by_line() {
    let edit = sign("a\nb§cd\n\nlast");
    assert_eq!(edit.lines(), &["a", "b§cd", "", "last"].map(String::from));
    assert_eq!(edit.color(), [0x00, 0x00, 0xFF, 255]);
    assert!(!edit.changed());
}

#[test]
fn insertion_respects_the_width_predicate_and_cursor_moves_across_lines() {
    let mut edit = sign("ab");
    edit.end();
    assert!(edit.insert('c', |line| line.len() <= 3));
    assert!(!edit.insert('d', |line| line.len() <= 3));
    assert_eq!(edit.lines()[0], "abc");
    assert!(!edit.insert('\n', |_| true));
    edit.right();
    assert_eq!(edit.cursor(), (1, 0));
    edit.left();
    assert_eq!(edit.cursor(), (0, 3));
    edit.backspace();
    edit.home();
    edit.delete();
    assert_eq!(edit.lines()[0], "b");
    assert!(edit.changed());
}

#[test]
fn enter_advances_until_the_last_line_then_finishes() {
    let mut edit = sign("");
    assert!(!edit.newline() && !edit.newline() && !edit.newline());
    assert!(edit.newline());
    assert_eq!(edit.cursor().0, 3);
}

#[test]
fn multibyte_characters_edit_by_character() {
    let mut edit = sign("é§");
    edit.end();
    edit.backspace();
    assert_eq!(edit.lines()[0], "é");
    edit.left();
    edit.delete();
    assert_eq!(edit.lines()[0], "");
}

#[test]
fn the_sent_compound_keeps_both_faces_and_identity() {
    let mut edit = sign("old");
    edit.end();
    edit.insert('!', |_| true);
    let bytes = edit.into_encoded_nbt().unwrap();
    let (nbt, used) = world::BlockEntityNbt::decode_prefix(&bytes).unwrap();
    assert_eq!(used, bytes.len());
    let root = nbt.parse().unwrap();
    assert_eq!(root.string("id"), Some("Sign"));
    assert_eq!(root.integer("y"), Some(2));
    assert_eq!(
        root.compound("FrontText").unwrap().string("Text"),
        Some("old!")
    );
    assert_eq!(root.compound("BackText").unwrap().string("Text"), Some(""));
}

#[test]
fn sign_art_follows_the_block_wood_and_mount() {
    let look = |name: &str| SignLook::of_block(Some(name));
    assert_eq!(look("minecraft:standing_sign").texture, "textures/ui/sign");
    assert_eq!(look("minecraft:wall_sign").texture, "textures/ui/sign");
    assert_eq!(
        look("minecraft:birch_wall_sign").texture,
        "textures/ui/sign_birch"
    );
    assert_eq!(
        look("minecraft:darkoak_standing_sign").texture,
        "textures/ui/sign_darkoak"
    );
    assert_eq!(
        look("minecraft:cherry_standing_sign").texture,
        "textures/ui/cherry_sign"
    );
    let hanging = look("minecraft:dark_oak_hanging_sign");
    assert!(hanging.hanging);
    assert_eq!(hanging.texture, "textures/ui/hanging_sign_darkoak");
    assert_eq!(
        look("minecraft:oak_hanging_sign").texture,
        "textures/ui/hanging_sign"
    );
    assert_eq!(SignLook::of_block(None).texture, "textures/ui/sign");
}

#[test]
fn pinned_poplar_signs_select_the_available_editor_art() {
    let registry = assets::read_registry_for_protocol(
        assets::pinned_block_registry_bytes(),
        assets::active_content_registry_protocol(),
    )
    .unwrap();
    let names = registry
        .iter()
        .filter(|record| {
            record.name.starts_with("minecraft:poplar_") && record.name.ends_with("sign")
        })
        .map(|record| record.name.as_ref())
        .collect::<std::collections::BTreeSet<_>>();
    assert!(!names.is_empty());
    for name in names {
        let look = SignLook::of_block(Some(name));
        if !look.hanging {
            assert_eq!(look.texture, "textures/ui/poplar_sign");
        }
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let pack = root.join(assets::vanilla_source().resource_pack_dir());
        if pack.is_dir() {
            assert!(
                pack.join(format!("{}.png", look.texture)).is_file(),
                "{name}"
            );
        } else {
            eprintln!("skipping poplar sign texture-file check: missing pinned resource pack");
        }
    }
}

#[test]
fn review_editing_the_legacy_back_preserves_the_front_text_and_color() {
    let mut root = NbtCompound::default();
    root.insert("Text", NbtValue::String("legacy front".into()));
    root.insert("SignTextColor", NbtValue::Int(-123));
    let mut edit = SignEdit::new([1, 2, 3], false, root);
    assert_eq!(edit.text(), "");
    edit.insert('b', |_| true);
    let bytes = edit.into_encoded_nbt().unwrap();
    let (encoded, _) = world::BlockEntityNbt::decode_prefix(&bytes).unwrap();
    let root = encoded.parse().unwrap();
    let front = root.compound("FrontText").unwrap();
    assert_eq!(front.string("Text"), Some("legacy front"));
    assert_eq!(front.integer("SignTextColor"), Some(-123));
    assert_eq!(root.compound("BackText").unwrap().string("Text"), Some("b"));
}

#[test]
fn review_sign_commit_survives_transport_backpressure() {
    let mut editor = SignEditor::default();
    let mut edit = SignEdit::new([1, 2, 3], true, NbtCompound::default());
    assert!(edit.insert('x', |_| true));
    editor.open(edit);
    assert!(!editor.finish(|_| Err(())));
    assert!(editor.is_open());
    assert!(editor.take_finish_request());
    let mut sent = 0;
    assert!(editor.finish(|_| {
        sent += 1;
        Ok(())
    }));
    assert_eq!(sent, 1);
    assert!(!editor.is_open());
}

#[test]
fn unchanged_close_still_sends_to_release_the_server_lock() {
    let mut editor = SignEditor::default();
    editor.open(SignEdit::new([1, 2, 3], true, NbtCompound::default()));
    let mut sent = 0;
    assert!(editor.finish(|_| {
        sent += 1;
        Ok(())
    }));
    assert_eq!(sent, 1);
    assert!(!editor.is_open());
}

/// Existing line breaks survive the same NBT encoding used when the editor closes.
#[test]
fn sign_text_preserves_authored_trailing_blank_lines() {
    for text in ["", "hello", "hello\n", "\n", "hello\n\n\n"] {
        let edit = sign(text);
        assert!(!edit.changed());
        let bytes = edit.into_encoded_nbt().unwrap();
        let (encoded, used) = world::BlockEntityNbt::decode_prefix(&bytes).unwrap();
        assert_eq!(used, bytes.len());
        assert_eq!(
            encoded
                .parse()
                .unwrap()
                .compound("FrontText")
                .unwrap()
                .string("Text"),
            Some(text),
        );
    }
}

/// Enter authors a line break even when the following line contains no characters.
#[test]
fn sign_text_preserves_enter_without_serializing_unused_editor_lines() {
    let mut edit = sign("hello");
    assert_eq!(edit.text(), "hello");
    assert!(!edit.newline());
    assert_eq!(edit.text(), "hello\n");
    assert!(edit.changed());
    assert!(!edit.newline());
    assert_eq!(edit.text(), "hello\n\n");
    assert!(!edit.newline());
    assert_eq!(edit.text(), "hello\n\n\n");
    assert!(edit.newline());
    assert_eq!(edit.text(), "hello\n\n\n");
}

/// Cursor navigation alone leaves padding inert; writing on a later line authors its separators.
#[test]
fn sign_text_distinguishes_cursor_padding_from_edited_lines() {
    let mut edit = sign("");
    edit.down();
    edit.down();
    assert_eq!(edit.text(), "");
    assert!(!edit.changed());
    assert!(edit.insert('é', |_| true));
    assert_eq!(edit.text(), "\n\né");
    edit.backspace();
    assert_eq!(edit.text(), "\n\n");
    assert!(edit.changed());
}
