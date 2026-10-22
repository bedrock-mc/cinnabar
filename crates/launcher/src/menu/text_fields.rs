//! Resolves menu controls to their editable text fields.

use super::{MenuAction, MenuField, realm_membership};

impl MenuAction {
    /// The text field a press on this control focuses.
    pub fn text_field(self) -> Option<MenuField> {
        match self {
            Self::SettingsMacroCommand(slot) => Some(MenuField::MacroCommand(slot)),
            Self::RealmMembership(realm_membership::Action::EditCode) => Some(MenuField::RealmCode),
            Self::AddName => Some(MenuField::Name),
            Self::AddAddress => Some(MenuField::Address),
            Self::AddPort => Some(MenuField::Port),
            Self::EditSkinName => Some(MenuField::SkinName),
            Self::SearchFriends => Some(MenuField::FriendQuery),
            Self::SearchSettings => Some(MenuField::SettingsQuery),
            Self::LocalWorld(action) => action.field(),
            _ => None,
        }
    }
}
