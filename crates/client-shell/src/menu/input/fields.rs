//! Menu text editors share their construction and byte limits.
use crate::menu::{MAX_SERVER_ADDRESS_BYTES, MAX_SERVER_NAME_BYTES, MAX_SERVER_PORT_BYTES};
use launcher::{
    local_worlds::form::{MAX_SEED_CHARS, MAX_WORLD_NAME_CHARS},
    menu::MenuField,
};
use ui::ChatEditor;

/// Creates an empty text editor with the byte limit for its menu field.
pub(in crate::menu) fn field_editor(field: MenuField) -> ChatEditor {
    ChatEditor::new(max_bytes(field)).expect("menu field budgets are valid editor bounds")
}

/// A field's byte budget; world fields allow their vanilla character limits in any script.
fn max_bytes(field: MenuField) -> usize {
    match field {
        MenuField::FriendQuery => ui::MAX_CHAT_INPUT_BYTES,
        MenuField::RealmCode => MAX_SERVER_ADDRESS_BYTES,
        MenuField::Name => MAX_SERVER_NAME_BYTES,
        MenuField::Address => MAX_SERVER_ADDRESS_BYTES,
        MenuField::Port => MAX_SERVER_PORT_BYTES,
        MenuField::WorldName => MAX_WORLD_NAME_CHARS * 4,
        MenuField::WorldSeed => MAX_SEED_CHARS * 4,
        MenuField::WorldOption(_) => i32::MAX.to_string().len(),
        MenuField::SkinName => launcher::dressing_room::MAX_SKIN_NAME_BYTES,
        MenuField::MacroCommand(_) => ui::MAX_CHAT_INPUT_BYTES,
    }
}
