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
