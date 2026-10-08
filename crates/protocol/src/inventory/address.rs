//! The one canonical container-address projection.
//!
//! The vanilla client routes inventory traffic by two rules.
//! `InventoryContent`/`InventorySlot` carry a
//! legacy window id and route by it alone — window 0 fills the player
//! inventory, the offhand and armor legacy windows their surfaces, and other
//! windows the open or dynamic container — consulting the packet's
//! `FullContainerName` only on the dynamic-storage window. `ItemStackResponse`
//! carries no window, only that `FullContainerName`, so it routes by the
//! decoded container name. Across both, the `DynamicContainerID` identifies a
//! generic-storage instance and is a discriminator for that one surface only;
//! for every fixed surface it is ignored, and a present zero is an ordinary id,
//! not a sentinel.
//!
//! [`project_container_cell`] folds those two rules onto the wire triple
//! (window id, decoded container-name code, slot index) so a Content event, a
//! Slot event, and an accepted item stack response describing the same physical
//! cell resolve to one [`CanonicalCell`] while distinct surfaces stay distinct.
//! Only identities this layer maps are recognized; anything else — an
//! unreviewed container name, or a player name arriving on a foreign window —
//! resolves to `None`, which callers treat as odd but well-formed data: a typed
//! counted skip, never a mutation and never a disconnect.

use super::ContainerIdentity;
use super::container_policy::{CONTAINER_NAME_CREATED_OUTPUT, CONTAINER_NAME_HOTBAR};
use super::windows::{
    is_chest_like_name, is_open_window_name, is_result_preview_name, ui_slot_for_name,
};

/// `EnumsContainerEnumName::Armorcontainer`, the player armor surface.
pub const CONTAINER_NAME_ARMOR: u8 = 6;
/// `EnumsContainerEnumName::Levelentitycontainer`, the generic screen-specific
/// storage surface keyed by its dynamic container id.
pub const CONTAINER_NAME_LEVEL_ENTITY: u8 = 7;
/// `EnumsContainerEnumName::Combinedhotbarandinventorycontainer`, the combined
/// player inventory surface every gesture request names.
pub const CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY: u8 = 12;
/// The distinct personal crafting input surface; wire cells 28..31.
pub const CONTAINER_NAME_CRAFT_INPUT: u8 = 13;
/// `EnumsContainerEnumName::Inventorycontainer`, the player-inventory name that
/// rides the legacy player window (the pinned fixture corpus encodes this shape).
pub const CONTAINER_NAME_INVENTORY: u8 = 29;
/// `EnumsContainerEnumName::Offhandcontainer`.
pub const CONTAINER_NAME_OFFHAND: u8 = 34;
/// `EnumsContainerEnumName::Cursorcontainer`.
pub const CONTAINER_NAME_CURSOR: u8 = 59;

/// `EnumsContainerEnumName::Dynamiccontainer`, a bundle's contents keyed by dynamic id.
pub const CONTAINER_NAME_DYNAMIC: u8 = 63;

/// The combined player-inventory window id (`CONTAINER_ID_INVENTORY`).
pub const PLAYER_INVENTORY_WINDOW_ID: i32 = 0;
/// The legacy offhand window id (`CONTAINER_ID_OFFHAND`), which servers send
/// without a full container name.
pub const OFFHAND_WINDOW_ID: i32 = 119;
/// The legacy armor window id, addressed like the offhand window.
pub const ARMOR_WINDOW_ID: i32 = 120;
/// The player's fixed UI inventory, including the cursor and crafting cells.
pub const UI_INVENTORY_WINDOW_ID: i32 = 124;

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
    /// One of four personal crafting cells, never a player-inventory alias.
    CraftInput(u8),
    /// One of nine crafting-table cells (wire slots 32..=40).
    TableCraftInput(u8),
    /// The created-output cell (wire slot 50).
    CreatedOutput,
    /// One screen-specific generic storage cell identified by its open
    /// container's dynamic id.
    GenericStorage { dynamic_id: Option<u32>, slot: u16 },
    /// A screen input at a fixed UI inventory slot (anvil, loom, beacon, ...).
    UiSlot(u8),
    /// A cell of a named open window (furnace, brewing stand, horse, crafter),
    /// addressed by container name and window index.
    WindowSlot { name: u8, slot: u16 },
}

impl CanonicalCell {
    /// Whether this cell belongs to the combined player-inventory surface.
    #[cfg(test)]
    #[must_use]
    pub const fn is_player_inventory(self) -> bool {
        matches!(self, Self::PlayerInventory(_))
    }
}

/// Resolves one wire cell address onto its canonical cell, or `None` when the
/// identity maps onto no canonical surface (callers count that as a skip).
///
/// The legacy player, offhand and armor windows route by window id alone and
/// ignore the container name, as vanilla does; servers fill that name freely.
/// Other addresses resolve by their decoded name, so a windowless response and
/// a windowed Content/Slot event for the same surface converge. Generic storage
/// is the one surface keyed by the dynamic id; every fixed surface ignores it,
/// so a present zero routes exactly like an absent one. Player-inventory names
/// bind only on a windowless response; the same name on another window is the
/// open container's, not the player's, and stays unrouted here.
///
/// Slot-index sanity is part of the mapping: cursor and offhand exist only at
/// their single indices, the hotbar name covers only its nine cells, and
/// player-inventory indices outside `0..36` are not player-inventory cells.
#[must_use]
pub fn project_container_cell(identity: &ContainerIdentity, slot: u16) -> Option<CanonicalCell> {
    match identity.window_id {
        Some(PLAYER_INVENTORY_WINDOW_ID) => return player_inventory_cell(slot),
        Some(OFFHAND_WINDOW_ID) => return (slot == 0).then_some(CanonicalCell::Offhand),
        Some(ARMOR_WINDOW_ID) => return armor_cell(slot),
        _ => {}
    }
    match identity.slot_type {
        Some(CONTAINER_NAME_CRAFT_INPUT) => {
            let index = u8::try_from(slot.checked_sub(28)?).ok()?;
            match index {
                0..4 => Some(CanonicalCell::CraftInput(index)),
                4..13 => Some(CanonicalCell::TableCraftInput(index - 4)),
                _ => None,
            }
        }
        Some(CONTAINER_NAME_CREATED_OUTPUT) => (slot == 50).then_some(CanonicalCell::CreatedOutput),
        Some(CONTAINER_NAME_CURSOR) => (slot == 0).then_some(CanonicalCell::Cursor),
        Some(CONTAINER_NAME_ARMOR) => armor_cell(slot),
        // Requests address the single offhand cell as wire slot 1 and
        // responses echo it; slot updates use index 0.
        Some(CONTAINER_NAME_OFFHAND) => matches!(slot, 0 | 1).then_some(CanonicalCell::Offhand),
        // The one surface the dynamic id keys.
        Some(CONTAINER_NAME_LEVEL_ENTITY) => Some(CanonicalCell::GenericStorage {
            dynamic_id: identity.dynamic_id,
            slot,
        }),
        // The three player-inventory names all fill the player container and
        // the dynamic id never discriminates among them; the hotbar name spans
        // only its nine cells, the other two the whole combined surface.
        Some(CONTAINER_NAME_HOTBAR) if identity.window_id.is_none() => {
            (slot < 9).then(|| player_inventory_cell(slot)).flatten()
        }
        Some(CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY | CONTAINER_NAME_INVENTORY)
            if identity.window_id.is_none() =>
        {
            player_inventory_cell(slot)
        }
        // Every screen's result preview mirrors the single output cell.
        Some(name) if is_result_preview_name(name) => {
            (slot == 50).then_some(CanonicalCell::CreatedOutput)
        }
        // Barrels and shulker boxes are chest-like windows under their own name.
        Some(name) if is_chest_like_name(name) => Some(CanonicalCell::GenericStorage {
            dynamic_id: identity.dynamic_id,
            slot,
        }),
        Some(name) if is_open_window_name(name) => Some(CanonicalCell::WindowSlot { name, slot }),
        // Screen inputs at fixed UI slots; a name that disagrees with its slot
        // stays unrouted.
        Some(name) => ui_slot_for_name(name, slot).map(CanonicalCell::UiSlot),
        // Trades and unnamed foreign windows have no canonical mapping here.
        None => None,
    }
}

fn armor_cell(slot: u16) -> Option<CanonicalCell> {
    let slot = u8::try_from(slot).ok()?;
    (slot < super::request::ARMOR_SLOTS).then_some(CanonicalCell::Armor(slot))
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

/// Whether an identity is the personal UI inventory (cursor, crafting cells,
/// created output) as servers address it.
#[must_use]
pub fn is_personal_ui_inventory(identity: &ContainerIdentity) -> bool {
    is_personal_ui_storage(identity)
}

fn is_personal_ui_storage(identity: &ContainerIdentity) -> bool {
    identity.window_id == Some(UI_INVENTORY_WINDOW_ID)
        && identity.slot_type == Some(0)
        && identity.dynamic_id.is_none()
}

/// Selects the four personal input references from an exact UI storage snapshot.
/// This contextual observation does not add a canonical or ordinary ledger alias.
#[must_use]
pub fn personal_craft_content_indices(
    identity: &ContainerIdentity,
    slots: usize,
) -> Option<[usize; 4]> {
    (is_personal_ui_storage(identity) && slots == 54).then_some([28, 29, 30, 31])
}

/// Selects one personal input observation from the default UI storage identity.
/// Other UI cells, named containers and cursor authority are not inferred here.
#[must_use]
pub fn personal_craft_slot_index(identity: &ContainerIdentity, slot: u16) -> Option<u8> {
    if !is_personal_ui_storage(identity) {
        return None;
    }
    let index = slot.checked_sub(28)?;
    (index < 4).then_some(u8::try_from(index).ok()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `InventoryContainer` on window 0 with a present zero dynamic id routes
    /// every player slot: the dynamic id is not a gate on a fixed surface.
    #[test]
    fn inventory_name_with_zero_dynamic_id_on_window_zero_routes_every_slot() {
        let identity = ContainerIdentity {
            window_id: Some(PLAYER_INVENTORY_WINDOW_ID),
            slot_type: Some(CONTAINER_NAME_INVENTORY),
            dynamic_id: Some(0),
        };
        for slot in 0..36 {
            assert_eq!(
                project_container_cell(&identity, slot),
                Some(CanonicalCell::PlayerInventory(u8::try_from(slot).unwrap()))
            );
        }
    }

    /// The whole container × dynamic-id × window/slot matrix. The dynamic id is
    /// swept `{None, Some(0), Some(7)}` against every fixed surface to pin that
    /// it discriminates generic storage alone; a present zero must route exactly
    /// like an absent id.
    #[test]
    fn container_routing_matrix_matches_the_vanilla_contract() {
        use CanonicalCell::{
            Armor, CraftInput, CreatedOutput, Cursor, GenericStorage, Offhand, PlayerInventory,
            TableCraftInput,
        };

        const COMBINED: u8 = CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY;
        // (window, name, slot, expected) — checked under every dynamic id.
        type Row = (Option<i32>, Option<u8>, u16, Option<CanonicalCell>);
        let dyn_invariant: &[Row] = &[
            // Player-inventory names bind on window 0 or windowless; the dynamic
            // id is not a discriminator, and a foreign window is not the player.
            (
                Some(0),
                Some(CONTAINER_NAME_INVENTORY),
                0,
                Some(PlayerInventory(0)),
            ),
            (
                Some(0),
                Some(CONTAINER_NAME_INVENTORY),
                35,
                Some(PlayerInventory(35)),
            ),
            (Some(0), Some(CONTAINER_NAME_INVENTORY), 36, None),
            (
                None,
                Some(CONTAINER_NAME_INVENTORY),
                13,
                Some(PlayerInventory(13)),
            ),
            (Some(0), Some(COMBINED), 20, Some(PlayerInventory(20))),
            (None, Some(COMBINED), 20, Some(PlayerInventory(20))),
            (
                Some(0),
                Some(CONTAINER_NAME_HOTBAR),
                3,
                Some(PlayerInventory(3)),
            ),
            // The legacy windows ignore their name, as vanilla does.
            (
                Some(0),
                Some(CONTAINER_NAME_HOTBAR),
                9,
                Some(PlayerInventory(9)),
            ),
            (Some(0), Some(1), 3, Some(PlayerInventory(3))),
            (Some(ARMOR_WINDOW_ID), Some(1), 2, Some(Armor(2))),
            (Some(OFFHAND_WINDOW_ID), Some(1), 0, Some(Offhand)),
            (Some(5), Some(CONTAINER_NAME_INVENTORY), 0, None),
            (Some(5), Some(COMBINED), 0, None),
            // Fixed non-player surfaces, likewise dynamic-id-invariant.
            (None, Some(CONTAINER_NAME_CURSOR), 0, Some(Cursor)),
            (None, Some(CONTAINER_NAME_CURSOR), 1, None),
            (None, Some(CONTAINER_NAME_ARMOR), 2, Some(Armor(2))),
            (None, Some(CONTAINER_NAME_ARMOR), 4, Some(Armor(4))),
            (None, Some(CONTAINER_NAME_ARMOR), 5, None),
            (None, Some(CONTAINER_NAME_OFFHAND), 0, Some(Offhand)),
            (None, Some(CONTAINER_NAME_OFFHAND), 1, Some(Offhand)),
            (None, Some(CONTAINER_NAME_OFFHAND), 2, None),
            (
                Some(124),
                Some(CONTAINER_NAME_CRAFT_INPUT),
                28,
                Some(CraftInput(0)),
            ),
            (
                Some(124),
                Some(CONTAINER_NAME_CRAFT_INPUT),
                32,
                Some(TableCraftInput(0)),
            ),
            (Some(124), Some(CONTAINER_NAME_CRAFT_INPUT), 27, None),
            (
                None,
                Some(CONTAINER_NAME_CREATED_OUTPUT),
                50,
                Some(CreatedOutput),
            ),
            (None, Some(CONTAINER_NAME_CREATED_OUTPUT), 0, None),
            // Unreviewed name and bare legacy windows.
            (Some(0), Some(211), 0, Some(PlayerInventory(0))),
            (Some(0), None, 20, Some(PlayerInventory(20))),
            (Some(OFFHAND_WINDOW_ID), None, 0, Some(Offhand)),
            (Some(OFFHAND_WINDOW_ID), None, 1, None),
            (Some(ARMOR_WINDOW_ID), None, 2, Some(Armor(2))),
        ];
        for &(window_id, slot_type, slot, expected) in dyn_invariant {
            for dynamic_id in [None, Some(0), Some(7)] {
                let identity = ContainerIdentity {
                    window_id,
                    slot_type,
                    dynamic_id,
                };
                assert_eq!(
                    project_container_cell(&identity, slot),
                    expected,
                    "{identity:?} slot {slot} under dynamic_id {dynamic_id:?}"
                );
            }
        }

        // Generic storage is the one surface the dynamic id keys, so each id
        // yields a distinct cell rather than collapsing.
        for dynamic_id in [None, Some(0), Some(7)] {
            let storage = ContainerIdentity {
                window_id: Some(4),
                slot_type: Some(CONTAINER_NAME_LEVEL_ENTITY),
                dynamic_id,
            };
            assert_eq!(
                project_container_cell(&storage, 5),
                Some(GenericStorage {
                    dynamic_id,
                    slot: 5
                })
            );
        }
    }

    #[test]
    fn contextual_personal_inputs_require_exact_identity_and_snapshot_shape() {
        let valid = identity(124, Some(0));
        assert_eq!(
            personal_craft_content_indices(&valid, 54),
            Some([28, 29, 30, 31])
        );
        for count in [0, 4, 53, 55, usize::MAX] {
            assert_eq!(personal_craft_content_indices(&valid, count), None);
        }
        for slot in 28..32 {
            assert_eq!(
                personal_craft_slot_index(&valid, slot),
                Some(u8::try_from(slot - 28).unwrap())
            );
            assert_eq!(project_container_cell(&valid, slot), None);
            assert_eq!(
                project_container_cell(&identity(0, None), slot),
                Some(CanonicalCell::PlayerInventory(u8::try_from(slot).unwrap()))
            );
        }
        for slot in [0, 27, 32, 50, 53, u16::MAX] {
            assert_eq!(personal_craft_slot_index(&valid, slot), None);
        }
        for invalid in [
            identity(0, Some(0)),
            identity(123, Some(0)),
            identity(124, None),
            identity(124, Some(CONTAINER_NAME_CRAFT_INPUT)),
            ContainerIdentity {
                window_id: None,
                ..valid
            },
            ContainerIdentity {
                dynamic_id: Some(1),
                ..valid
            },
        ] {
            assert_eq!(personal_craft_content_indices(&invalid, 54), None);
            assert_eq!(personal_craft_slot_index(&invalid, 28), None);
        }
    }

    /// A windowless (stack-response) address naming one container.
    fn named(slot_type: u8) -> ContainerIdentity {
        ContainerIdentity {
            window_id: None,
            slot_type: Some(slot_type),
            dynamic_id: None,
        }
    }

    fn identity(window_id: i32, slot_type: Option<u8>) -> ContainerIdentity {
        ContainerIdentity {
            window_id: Some(window_id),
            slot_type,
            dynamic_id: None,
        }
    }

    /// Encodes one generated container-name variant so the pinned constants
    /// can be checked against the generated enum numbering.
    fn encoded_name(value: valentine::bedrock::version::v1_26_51::EnumsContainerEnumName) -> u8 {
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
        use valentine::bedrock::version::v1_26_51::EnumsContainerEnumName;

        let pairs = [
            (
                CONTAINER_NAME_CRAFT_INPUT,
                EnumsContainerEnumName::Craftinginputcontainer,
            ),
            (CONTAINER_NAME_ARMOR, EnumsContainerEnumName::Armorcontainer),
            (
                CONTAINER_NAME_LEVEL_ENTITY,
                EnumsContainerEnumName::Levelentitycontainer,
            ),
            (
                CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY,
                EnumsContainerEnumName::Combinedhotbarandinventorycontainer,
            ),
            (
                CONTAINER_NAME_INVENTORY,
                EnumsContainerEnumName::Inventorycontainer,
            ),
            (
                CONTAINER_NAME_OFFHAND,
                EnumsContainerEnumName::Offhandcontainer,
            ),
            (
                CONTAINER_NAME_CURSOR,
                EnumsContainerEnumName::Cursorcontainer,
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
    fn combined_player_name_requires_the_player_window_when_a_window_is_present() {
        assert_eq!(
            project_container_cell(
                &identity(6, Some(CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY)),
                3,
            ),
            None,
            "inbound Content and Slot traffic cannot relabel another window as player inventory",
        );
        assert_eq!(
            project_container_cell(
                &ContainerIdentity {
                    window_id: None,
                    slot_type: Some(CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY),
                    dynamic_id: Some(7),
                },
                3,
            ),
            Some(CanonicalCell::PlayerInventory(3)),
            "windowless stack-response containers remain routable",
        );
    }

    #[test]
    fn cursor_armor_offhand_and_storage_surfaces_stay_distinct_from_player_cells() {
        assert_eq!(
            project_container_cell(&named(CONTAINER_NAME_CURSOR), 0),
            Some(CanonicalCell::Cursor)
        );
        // A cursor address beyond its single cell does not exist.
        assert_eq!(
            project_container_cell(&named(CONTAINER_NAME_CURSOR), 1),
            None
        );
        assert_eq!(
            project_container_cell(&named(CONTAINER_NAME_ARMOR), 2),
            Some(CanonicalCell::Armor(2))
        );
        assert_ne!(
            project_container_cell(&named(CONTAINER_NAME_ARMOR), 2),
            project_container_cell(&identity(0, None), 2)
        );

        // Both offhand encodings converge; neither touches a player cell.
        assert_eq!(
            project_container_cell(&named(CONTAINER_NAME_OFFHAND), 0),
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
    fn named_inventory_alias_on_the_legacy_window_converges_with_unnamed_window_zero() {
        use valentine::bedrock::version::v1_26_51::EnumsContainerEnumName;

        let inventory_name = encoded_name(EnumsContainerEnumName::Inventorycontainer);
        let expected = CanonicalCell::PlayerInventory(4);
        assert_eq!(
            project_container_cell(&identity(0, Some(inventory_name)), 4),
            Some(expected)
        );
        assert_eq!(
            project_container_cell(&identity(0, Some(inventory_name)), 4),
            project_container_cell(&identity(0, None), 4),
        );
        // Responses echo the request's name without a window; a foreign window
        // stays unrouted, but the dynamic id never un-routes a player name.
        assert_eq!(
            project_container_cell(
                &ContainerIdentity {
                    window_id: None,
                    slot_type: Some(inventory_name),
                    dynamic_id: None,
                },
                13
            ),
            Some(CanonicalCell::PlayerInventory(13))
        );
        assert_eq!(
            project_container_cell(&identity(6, Some(inventory_name)), 4),
            None
        );
        assert_eq!(
            project_container_cell(
                &ContainerIdentity {
                    window_id: None,
                    slot_type: Some(inventory_name),
                    dynamic_id: Some(3),
                },
                4
            ),
            Some(expected),
        );
        // Surface bounds still hold.
        assert_eq!(
            project_container_cell(&identity(0, Some(inventory_name)), 36),
            None
        );
    }

    #[test]
    fn unrouted_container_names_and_legacy_windows_resolve_to_none() {
        assert_eq!(project_container_cell(&named(211), 0), None);
        assert_eq!(
            project_container_cell(&named(CONTAINER_NAME_HOTBAR), 9),
            None,
            "the hotbar name covers only the nine hotbar cells"
        );
        assert_eq!(
            project_container_cell(&identity(7, Some(CONTAINER_NAME_HOTBAR)), 0),
            None
        );
        assert_eq!(project_container_cell(&identity(-777, None), 0), None);
        assert_eq!(project_container_cell(&identity(119, None), 1), None);
        assert_eq!(project_container_cell(&identity(0, None), 4_096), None);
    }

    #[test]
    fn four_named_crafting_cells_are_distinct_from_player_main_inventory_and_bare_ui() {
        for slot in 28..32 {
            let craft =
                project_container_cell(&identity(124, Some(CONTAINER_NAME_CRAFT_INPUT)), slot);
            assert_eq!(
                craft,
                Some(CanonicalCell::CraftInput(u8::try_from(slot - 28).unwrap()))
            );
            assert_ne!(craft, project_container_cell(&identity(0, None), slot));
            assert_eq!(project_container_cell(&identity(124, None), slot), None);
        }
        assert_eq!(
            project_container_cell(&identity(124, Some(CONTAINER_NAME_CRAFT_INPUT)), 27),
            None
        );
        for slot in 32..41 {
            assert_eq!(
                project_container_cell(&identity(124, Some(CONTAINER_NAME_CRAFT_INPUT)), slot),
                Some(CanonicalCell::TableCraftInput(
                    u8::try_from(slot - 32).unwrap()
                ))
            );
        }
        assert_eq!(
            project_container_cell(&identity(124, Some(CONTAINER_NAME_CRAFT_INPUT)), 41),
            None
        );
    }

    /// Request-shaped names from the owner's captures: hotbar cells, the
    /// offhand's wire slot 1, armor by name or window 120, and created output.
    #[test]
    fn vanilla_request_container_names_resolve_to_their_cells() {
        let window_less = |slot_type| ContainerIdentity {
            window_id: None,
            slot_type: Some(slot_type),
            dynamic_id: None,
        };
        assert_eq!(
            project_container_cell(&window_less(CONTAINER_NAME_HOTBAR), 3),
            Some(CanonicalCell::PlayerInventory(3))
        );
        assert_eq!(
            project_container_cell(&window_less(CONTAINER_NAME_OFFHAND), 1),
            Some(CanonicalCell::Offhand)
        );
        assert_eq!(
            project_container_cell(&window_less(CONTAINER_NAME_OFFHAND), 2),
            None
        );
        assert_eq!(
            project_container_cell(&window_less(CONTAINER_NAME_ARMOR), 4),
            Some(CanonicalCell::Armor(4))
        );
        assert_eq!(
            project_container_cell(&window_less(CONTAINER_NAME_ARMOR), 5),
            None
        );
        assert_eq!(
            project_container_cell(&identity(ARMOR_WINDOW_ID, None), 2),
            Some(CanonicalCell::Armor(2))
        );
        assert_eq!(
            project_container_cell(&window_less(CONTAINER_NAME_CREATED_OUTPUT), 50),
            Some(CanonicalCell::CreatedOutput)
        );
        assert_eq!(
            project_container_cell(&window_less(CONTAINER_NAME_CREATED_OUTPUT), 0),
            None
        );
    }
}
