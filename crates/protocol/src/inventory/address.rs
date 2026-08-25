//! The one canonical container-address projection.
//!
//! Bedrock names an inventory cell three different ways on the wire: a legacy
//! bare window id plus slot index (`InventoryContent`/`InventorySlot`), a full
//! container name plus dynamic id alongside that window id, and a response
//! container that carries only the container name plus dynamic id. Matching
//! any one wire field alone collapses distinct surfaces onto each other — a
//! cursor update can land in hotbar cell 0, or an offhand rewrite can clear
//! unseen inventory cells.
//!
//! [`project_container_cell`] is the single resolver every admission, lookup,
//! and accepted-response path routes through. It maps the wire triple (window
//! id, decoded container-name code, slot index) onto one explicit
//! [`CanonicalCell`], so a Content event, a Slot event, and an accepted item
//! stack response describing the same physical cell all resolve to the same
//! canonical value while distinct cells stay distinct.
//!
//! Only identities today's protocol layer actually decodes are enumerated;
//! anything else — including decoded container names this client has no
//! reviewed mapping for — resolves to `None`, and callers treat that as odd
//! but well-formed data: a typed counted skip, never a mutation and never a
//! disconnect.

use super::ContainerIdentity;

/// `EnumsContainerEnumName::ArmorContainer`, the player armor surface.
pub const CONTAINER_NAME_ARMOR: u8 = 6;
/// `EnumsContainerEnumName::LevelEntityContainer`, the generic screen-specific
/// storage surface keyed by its dynamic container id.
pub const CONTAINER_NAME_LEVEL_ENTITY: u8 = 7;
/// `EnumsContainerEnumName::CombinedHotbarAndInventoryContainer`, the combined
/// player inventory surface every gesture request names.
pub const CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY: u8 = 12;
/// `EnumsContainerEnumName::OffhandContainer`.
pub const CONTAINER_NAME_OFFHAND: u8 = 34;
/// `EnumsContainerEnumName::CursorContainer`.
pub const CONTAINER_NAME_CURSOR: u8 = 59;

/// The combined player-inventory window id (`CONTAINER_ID_INVENTORY`).
pub const PLAYER_INVENTORY_WINDOW_ID: i32 = 0;
/// The legacy offhand window id (`CONTAINER_ID_OFFHAND`), which servers send
/// without a full container name.
pub const OFFHAND_WINDOW_ID: i32 = 119;

/// One canonical inventory cell in the explicit cross-surface address space.
///
/// Distinct members never collide: the thirty-six
/// [`CanonicalCell::PlayerInventory`] indices cover the nine hotbar cells
/// (0..9) followed by the twenty-seven main-inventory cells (9..36), and
/// armor, offhand, cursor, and the dynamic generic-storage surface are
/// separate values regardless of what legacy window id happened to ride the
/// packet.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum CanonicalCell {
    /// One combined player-inventory cell: indices 0..9 are the hotbar cells,
    /// 9..36 the main-inventory cells.
    PlayerInventory(u8),
    /// One armor-surface cell addressed by its container-relative index.
    Armor(u8),
    /// The single offhand cell.
    Offhand,
    /// The single held-stack cursor cell.
    Cursor,
    /// One screen-specific generic storage cell identified by its open
    /// container's dynamic id.
    GenericStorage { dynamic_id: Option<u32>, slot: u16 },
}

impl CanonicalCell {
    /// Whether this cell belongs to the combined player-inventory surface.
    #[must_use]
    pub const fn is_player_inventory(self) -> bool {
        matches!(self, Self::PlayerInventory(_))
    }
}

/// Resolves one wire cell address onto its canonical cell, or `None` when the
/// container identity does not route onto the canonical space.
///
/// Named containers resolve by their decoded container-name code alone, so a
/// Content event, a Slot event, and an accepted item stack response (whose
/// container carries no window id) converge on the same value. Unnamed
/// addresses fall back to the two legacy window ids the client recognizes:
/// window 0 as the combined player inventory and window 119 as the offhand.
///
/// Slot-index sanity is part of the mapping: a cursor or offhand address only
/// exists at index 0, and player-inventory indices outside `0..36` are not
/// player-inventory cells at all.
#[must_use]
pub fn project_container_cell(identity: &ContainerIdentity, slot: u16) -> Option<CanonicalCell> {
    match identity.slot_type {
        Some(CONTAINER_NAME_CURSOR) => (slot == 0).then_some(CanonicalCell::Cursor),
        Some(CONTAINER_NAME_ARMOR) => Some(CanonicalCell::Armor(u8::try_from(slot).ok()?)),
        Some(CONTAINER_NAME_OFFHAND) => (slot == 0).then_some(CanonicalCell::Offhand),
        Some(CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY) => player_inventory_cell(slot),
        Some(CONTAINER_NAME_LEVEL_ENTITY) => Some(CanonicalCell::GenericStorage {
            dynamic_id: identity.dynamic_id,
            slot,
        }),
        // Every other decoded container name — crafting inputs, furnaces,
        // trades, unknown codes — has no reviewed canonical mapping here.
        Some(_) => None,
        None => match identity.window_id {
            Some(PLAYER_INVENTORY_WINDOW_ID) => player_inventory_cell(slot),
            Some(OFFHAND_WINDOW_ID) if slot == 0 => Some(CanonicalCell::Offhand),
            _ => None,
        },
    }
}

fn player_inventory_cell(slot: u16) -> Option<CanonicalCell> {
    let slots = u16::from(super::request::PLAYER_INVENTORY_SLOTS);
    if slot < slots {
        #[allow(
            clippy::cast_possible_truncation,
            reason = "guarded by the PLAYER_INVENTORY_SLOTS bound above"
        )]
        Some(CanonicalCell::PlayerInventory(slot as u8))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity(window_id: i32, slot_type: Option<u8>) -> ContainerIdentity {
        ContainerIdentity {
            window_id: Some(window_id),
            slot_type,
            dynamic_id: None,
        }
    }

    /// Encodes one generated container-name variant so the pinned constants
    /// can be checked against the generated enum numbering.
    fn encoded_name(value: valentine::bedrock::version::v1_26_44::EnumsContainerEnumName) -> u8 {
        use valentine::bedrock::codec::BedrockCodec;

        let mut bytes = bytes::BytesMut::with_capacity(1);
        value
            .encode(&mut bytes)
            .expect("a one-byte container name always encodes");
        bytes[0]
    }

    /// Pins the hand-copied container-name constants against the generated
    /// encoder so a valentine renumber fails loudly here instead of silently
    /// misrouting live traffic.
    #[test]
    fn pinned_container_name_constants_match_the_generated_enum_encoding() {
        use valentine::bedrock::version::v1_26_44::EnumsContainerEnumName;

        let pairs = [
            (CONTAINER_NAME_ARMOR, EnumsContainerEnumName::ArmorContainer),
            (
                CONTAINER_NAME_LEVEL_ENTITY,
                EnumsContainerEnumName::LevelEntityContainer,
            ),
            (
                CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY,
                EnumsContainerEnumName::CombinedHotbarAndInventoryContainer,
            ),
            (
                CONTAINER_NAME_OFFHAND,
                EnumsContainerEnumName::OffhandContainer,
            ),
            (
                CONTAINER_NAME_CURSOR,
                EnumsContainerEnumName::CursorContainer,
            ),
        ];
        for (pinned, generated) in pairs {
            assert_eq!(pinned, encoded_name(generated));
        }
    }

    #[test]
    fn unnamed_and_named_player_inventory_addresses_converge_on_one_canonical_cell() {
        let expected = CanonicalCell::PlayerInventory(20);
        assert_eq!(
            project_container_cell(&identity(0, None), 20),
            Some(expected)
        );
        assert_eq!(
            project_container_cell(
                &identity(0, Some(CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY)),
                20
            ),
            Some(expected)
        );
        // Accepted responses carry no window id at all.
        assert_eq!(
            project_container_cell(
                &ContainerIdentity {
                    window_id: None,
                    slot_type: Some(CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY),
                    dynamic_id: Some(7),
                },
                20
            ),
            Some(expected)
        );
        // Hotbar and main-inventory ranges are both inside the surface.
        assert_eq!(
            project_container_cell(&identity(0, None), 0),
            Some(CanonicalCell::PlayerInventory(0))
        );
        assert_eq!(
            project_container_cell(&identity(0, None), 35),
            Some(CanonicalCell::PlayerInventory(35))
        );
        // Out-of-range indices are not player-inventory cells.
        assert_eq!(project_container_cell(&identity(0, None), 36), None);
        assert_eq!(
            project_container_cell(
                &identity(0, Some(CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY)),
                4_095
            ),
            None
        );
    }

    #[test]
    fn cursor_armor_offhand_and_storage_surfaces_stay_distinct_from_player_cells() {
        assert_eq!(
            project_container_cell(&identity(0, Some(CONTAINER_NAME_CURSOR)), 0),
            Some(CanonicalCell::Cursor)
        );
        // A cursor address beyond its single cell does not exist.
        assert_eq!(
            project_container_cell(&identity(0, Some(CONTAINER_NAME_CURSOR)), 1),
            None
        );
        assert_eq!(
            project_container_cell(&identity(0, Some(CONTAINER_NAME_ARMOR)), 2),
            Some(CanonicalCell::Armor(2))
        );
        assert_ne!(
            project_container_cell(&identity(0, Some(CONTAINER_NAME_ARMOR)), 2),
            project_container_cell(&identity(0, None), 2)
        );

        // Both offhand encodings converge; neither touches a player cell.
        assert_eq!(
            project_container_cell(&identity(0, Some(CONTAINER_NAME_OFFHAND)), 0),
            Some(CanonicalCell::Offhand)
        );
        assert_eq!(
            project_container_cell(&identity(OFFHAND_WINDOW_ID, None), 0),
            Some(CanonicalCell::Offhand)
        );
        assert_eq!(
            project_container_cell(&identity(OFFHAND_WINDOW_ID, None), 5),
            None
        );

        let storage = project_container_cell(
            &ContainerIdentity {
                window_id: Some(4),
                slot_type: Some(CONTAINER_NAME_LEVEL_ENTITY),
                dynamic_id: Some(9),
            },
            53,
        );
        assert_eq!(
            storage,
            Some(CanonicalCell::GenericStorage {
                dynamic_id: Some(9),
                slot: 53
            })
        );
        assert!(!storage.is_some_and(CanonicalCell::is_player_inventory));
    }

    #[test]
    fn unrouted_container_names_and_legacy_windows_resolve_to_none() {
        assert_eq!(project_container_cell(&identity(0, Some(211)), 0), None);
        assert_eq!(
            project_container_cell(&identity(0, Some(28)), 0),
            None,
            "the standalone hotbar container name has no reviewed mapping"
        );
        assert_eq!(project_container_cell(&identity(-777, None), 0), None);
        assert_eq!(project_container_cell(&identity(119, None), 1), None);
        assert_eq!(project_container_cell(&identity(0, None), 4_096), None);
    }
}
