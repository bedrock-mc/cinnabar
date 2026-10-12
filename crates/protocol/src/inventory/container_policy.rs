//! Which window owns each protocol container name, ported from the owner's
//! proxy policy table. Requests may only address fixed player windows or the
//! one replaceable open container.

/// The window that owns a container name.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ContainerWindow {
    /// The replaceable open container (chest, furnace, anvil, ...).
    Open,
    /// The 36 player cells: combined, hotbar and inventory names.
    Player,
    Armor,
    Offhand,
    /// The personal UI inventory: cursor, crafting input and created output.
    Ui,
}

pub const CONTAINER_NAME_HOTBAR: u8 = 28;
pub const CONTAINER_NAME_CREATED_OUTPUT: u8 = 60;
/// The last container name the pinned protocol enumerates.
pub const LAST_CONTAINER_NAME: u8 = 66;

/// Classifies one decoded container name; `None` for codes this protocol
/// version does not enumerate.
#[must_use]
pub const fn container_window(name: u8) -> Option<ContainerWindow> {
    use ContainerWindow::{Armor, Offhand, Open, Player, Ui};
    Some(match name {
        6 => Armor,
        12 | CONTAINER_NAME_HOTBAR | 29 => Player,
        13 | 59 | CONTAINER_NAME_CREATED_OUTPUT => Ui,
        34 => Offhand,
        0..=LAST_CONTAINER_NAME => Open,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use bedrock_protocol_1_26_51::enums::ContainerEnumName as Name;

    use super::*;

    fn decoded(code: u8) -> Name {
        Name::from(code)
    }

    /// Every enumerated name is classified and every unenumerated code is not,
    /// so a renumbered protocol fails here instead of misrouting requests.
    #[test]
    fn every_enumerated_container_name_is_classified() {
        for code in 0..=u8::MAX {
            let known = !matches!(decoded(code), Name::Unknown(_));
            assert_eq!(container_window(code).is_some(), known, "code {code}");
        }
        let fixed = [
            (Name::Armorcontainer, ContainerWindow::Armor),
            (
                Name::Combinedhotbarandinventorycontainer,
                ContainerWindow::Player,
            ),
            (Name::Hotbarcontainer, ContainerWindow::Player),
            (Name::Inventorycontainer, ContainerWindow::Player),
            (Name::Offhandcontainer, ContainerWindow::Offhand),
            (Name::Craftinginputcontainer, ContainerWindow::Ui),
            (Name::Cursorcontainer, ContainerWindow::Ui),
            (Name::Createdoutputcontainer, ContainerWindow::Ui),
        ];
        for code in 0..=LAST_CONTAINER_NAME {
            let name = decoded(code);
            let expected = fixed
                .iter()
                .find(|(fixed, _)| *fixed == name)
                .map_or(ContainerWindow::Open, |(_, window)| *window);
            assert_eq!(container_window(code), Some(expected), "{name:?}");
        }
    }
}
