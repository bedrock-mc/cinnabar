//! Which vanilla screen each container window opens, and which inventory cell
//! each index of that screen's item collections addresses. Storage cells are the
//! window's own; UI cells (anvil, enchanting, stonecutter, …) live in the
//! personal UI inventory, as the ledger keeps them. Collection names follow the
//! 26.30 `ui/*_screen.json` templates.

use protocol::WindowKind;

use crate::ui_runtime::presentation::inventory_pointer::InventoryCellHit;

/// One cell a collection index addresses.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Cell {
    Storage(u8),
    /// A personal UI-inventory slot.
    Ui(u8),
    /// The screen's created output.
    Output,
}

impl Cell {
    pub const fn hit(self) -> InventoryCellHit {
        match self {
            Self::Storage(slot) => InventoryCellHit::Storage(slot),
            Self::Ui(slot) => InventoryCellHit::Craft(slot),
            Self::Output => InventoryCellHit::CraftOutput,
        }
    }
}

/// One container window's screen and cell collections.
#[derive(Debug, PartialEq, Eq)]
pub struct ContainerKind {
    pub screen: &'static str,
    pub title_key: &'static str,
    /// `(collection, cells)` in collection-index order.
    pub collections: &'static [(&'static str, &'static [Cell])],
    /// Screen variables the controller sets.
    pub flags: &'static [&'static str],
}

const fn storage_run<const N: usize>(first: u8) -> [Cell; N] {
    let mut cells = [Cell::Storage(0); N];
    let mut index = 0;
    while index < N {
        cells[index] = Cell::Storage(first + index as u8);
        index += 1;
    }
    cells
}

const CHEST_27: [Cell; 27] = storage_run(0);
const CHEST_54: [Cell; 54] = storage_run(0);
const CHEST_18: [Cell; 18] = storage_run(0);
const CHEST_36: [Cell; 36] = storage_run(0);
const CHEST_45: [Cell; 45] = storage_run(0);
const NINE: [Cell; 9] = storage_run(0);
const FIVE: [Cell; 5] = storage_run(0);
/// The largest chest a mount carries; smaller ones show a prefix.
pub const MOUNT_CHEST: [Cell; 15] = storage_run(2);

const fn kind(
    screen: &'static str,
    title_key: &'static str,
    collections: &'static [(&'static str, &'static [Cell])],
) -> ContainerKind {
    ContainerKind {
        screen,
        title_key,
        collections,
        flags: &[],
    }
}

const CHEST_CELLS: &[(&str, &[Cell])] = &[("container_items", &CHEST_27)];
const SMALL_CHEST: ContainerKind = kind("chest.small_chest_screen", "container.chest", CHEST_CELLS);
const LARGE_CHEST: ContainerKind = kind(
    "chest.large_chest_screen",
    "container.chestDouble",
    &[("container_items", &CHEST_54)],
);
// Vanilla chest screens use the container's size, rather than requiring exactly 27 or 54.
// Server-authored chest layouts may display any of these bounded row counts.
const MENU_CHESTS: [ContainerKind; 4] = [
    kind(
        "chest.small_chest_screen",
        "container.chest",
        &[("container_items", &NINE)],
    ),
    kind(
        "chest.small_chest_screen",
        "container.chest",
        &[("container_items", &CHEST_18)],
    ),
    kind(
        "chest.small_chest_screen",
        "container.chest",
        &[("container_items", &CHEST_36)],
    ),
    kind(
        "chest.small_chest_screen",
        "container.chest",
        &[("container_items", &CHEST_45)],
    ),
];
const BARREL: ContainerKind = kind("chest.barrel_screen", "container.barrel", CHEST_CELLS);
const SHULKER_BOX: ContainerKind = kind(
    "chest.shulker_box_screen",
    "container.shulkerbox",
    CHEST_CELLS,
);
const ENDER_CHEST: ContainerKind = kind(
    "chest.ender_chest_screen",
    "container.enderchest",
    CHEST_CELLS,
);

const FURNACE_CELLS: &[(&str, &[Cell])] = &[
    ("furnace_ingredient_items", &[Cell::Storage(0)]),
    ("furnace_fuel_items", &[Cell::Storage(1)]),
    ("furnace_output_items", &[Cell::Storage(2)]),
];

/// A chest-like storage window by its cell count and the block entity it opened
/// (`id` of its NBT), as the client picks the chest, barrel, shulker or ender screen.
pub fn storage_kind(slots: usize, block_entity: Option<&str>) -> &'static ContainerKind {
    match (slots, block_entity) {
        (54, _) => &LARGE_CHEST,
        (_, Some("Barrel")) => &BARREL,
        (_, Some("ShulkerBox")) => &SHULKER_BOX,
        (_, Some("EnderChest")) => &ENDER_CHEST,
        _ => MENU_CHESTS
            .iter()
            .find(|kind| kind.collections[0].1.len() == slots)
            .unwrap_or(&SMALL_CHEST),
    }
}

/// Row counts supported by the bounded chest templates and server menu layouts.
pub(crate) fn supported_storage_slots(slots: usize) -> bool {
    matches!(WindowKind::Storage.open_cells(), Some(protocol::OpenCells::Generic(lengths)) if lengths.contains(&slots))
}

/// The vanilla screen of a station window, `None` for kinds drawn elsewhere.
pub fn window_kind(kind: WindowKind) -> Option<&'static ContainerKind> {
    const FURNACE: ContainerKind =
        self::kind("furnace.furnace_screen", "container.furnace", FURNACE_CELLS);
    const BLAST_FURNACE: ContainerKind = self::kind(
        "blast_furnace.blast_furnace_screen",
        "tile.blast_furnace.name",
        FURNACE_CELLS,
    );
    const SMOKER: ContainerKind =
        self::kind("smoker.smoker_screen", "tile.smoker.name", FURNACE_CELLS);
    const BREWING: ContainerKind = self::kind(
        "brewing_stand.brewing_stand_screen",
        "container.brewing",
        &[
            ("brewing_input_item", &[Cell::Storage(0)]),
            (
                "brewing_result_items",
                &[Cell::Storage(1), Cell::Storage(2), Cell::Storage(3)],
            ),
            ("brewing_fuel_item", &[Cell::Storage(4)]),
        ],
    );
    const ANVIL: ContainerKind = self::kind(
        "anvil.anvil_screen",
        "container.repair",
        &[
            ("anvil_input_items", &[Cell::Ui(1)]),
            ("anvil_material_items", &[Cell::Ui(2)]),
            ("anvil_result_items", &[Cell::Output]),
        ],
    );
    const ENCHANTING: ContainerKind = self::kind(
        "enchanting.enchanting_screen",
        "container.enchant",
        &[
            ("enchanting_input_items", &[Cell::Ui(14)]),
            ("enchanting_lapis_items", &[Cell::Ui(15)]),
        ],
    );
    const GRINDSTONE: ContainerKind = self::kind(
        "grindstone.grindstone_screen",
        "container.grindstone_title",
        &[
            ("grindstone_input_items", &[Cell::Ui(16)]),
            ("grindstone_additional_items", &[Cell::Ui(17)]),
            ("grindstone_result_items", &[Cell::Output]),
        ],
    );
    const LOOM: ContainerKind = self::kind(
        "loom.loom_screen",
        "container.loom",
        &[
            ("loom_input_items", &[Cell::Ui(9)]),
            ("loom_dye_items", &[Cell::Ui(10)]),
            ("loom_material_items", &[Cell::Ui(11)]),
            ("loom_result_items", &[Cell::Output]),
        ],
    );
    // 26.30 draws the template-slot table through `$use_smithing_table_2_ui`.
    const SMITHING: ContainerKind = ContainerKind {
        flags: &["use_smithing_table_2_ui"],
        ..self::kind(
            "smithing_table.smithing_table_screen",
            "container.smithing_table",
            &[
                ("smithing_table_template_items", &[Cell::Ui(53)]),
                ("smithing_table_input_items", &[Cell::Ui(51)]),
                ("smithing_table_material_items", &[Cell::Ui(52)]),
                ("smithing_table_result_items", &[Cell::Output]),
            ],
        )
    };
    const CARTOGRAPHY: ContainerKind = self::kind(
        "cartography.cartography_screen",
        "container.cartography_table",
        &[
            ("cartography_input_items", &[Cell::Ui(12)]),
            ("cartography_additional_items", &[Cell::Ui(13)]),
            ("cartography_result_items", &[Cell::Output]),
        ],
    );
    const STONECUTTER: ContainerKind = self::kind(
        "stonecutter.stonecutter_screen",
        "container.stonecutter",
        &[
            ("stonecutter_input_items", &[Cell::Ui(3)]),
            ("stonecutter_result_items", &[Cell::Output]),
        ],
    );
    const BEACON: ContainerKind = self::kind(
        "beacon.beacon_screen",
        "container.beacon",
        &[("beacon_payment_items", &[Cell::Ui(27)])],
    );
    const HOPPER: ContainerKind = self::kind(
        "redstone.hopper_screen",
        "container.hopper",
        &[("container_items", &FIVE)],
    );
    const DISPENSER: ContainerKind = self::kind(
        "redstone.dispenser_screen",
        "container.dispenser",
        &[("container_items", &NINE)],
    );
    const DROPPER: ContainerKind = self::kind(
        "redstone.dropper_screen",
        "container.dropper",
        &[("container_items", &NINE)],
    );
    const CRAFTER: ContainerKind = self::kind(
        "redstone.crafter_screen",
        "container.crafter",
        &[("container_items", &NINE)],
    );
    Some(match kind {
        WindowKind::Furnace => &FURNACE,
        WindowKind::BlastFurnace => &BLAST_FURNACE,
        WindowKind::Smoker => &SMOKER,
        WindowKind::Brewing => &BREWING,
        WindowKind::Anvil => &ANVIL,
        WindowKind::Enchanting => &ENCHANTING,
        WindowKind::Grindstone => &GRINDSTONE,
        WindowKind::Loom => &LOOM,
        WindowKind::Smithing => &SMITHING,
        WindowKind::Cartography => &CARTOGRAPHY,
        WindowKind::Stonecutter => &STONECUTTER,
        WindowKind::Beacon => &BEACON,
        WindowKind::Hopper => &HOPPER,
        WindowKind::Dispenser => &DISPENSER,
        WindowKind::Dropper => &DROPPER,
        WindowKind::Crafter => &CRAFTER,
        WindowKind::Horse => mount_kind(mount_slots(None)),
        WindowKind::Storage | WindowKind::Workbench | WindowKind::Lectern => return None,
    })
}

/// What a mount wears besides a saddle.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MountBody {
    None,
    HorseArmor,
    Carpet,
    NautilusArmor,
}

/// A mount's equippable slots: whether it takes a saddle (slot 0) and what
/// it wears in slot 1.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MountSlots {
    pub saddle: bool,
    pub body: MountBody,
}

/// `minecraft:equippable` of the 1.26.50 behavior pack's mounts.
const MOUNTS: [(&str, bool, MountBody); 11] = [
    ("minecraft:horse", true, MountBody::HorseArmor),
    ("minecraft:zombie_horse", true, MountBody::HorseArmor),
    ("minecraft:donkey", true, MountBody::None),
    ("minecraft:mule", true, MountBody::None),
    ("minecraft:camel", true, MountBody::None),
    ("minecraft:camel_husk", true, MountBody::None),
    ("minecraft:skeleton_horse", false, MountBody::None),
    ("minecraft:llama", false, MountBody::Carpet),
    ("minecraft:trader_llama", false, MountBody::Carpet),
    ("minecraft:nautilus", true, MountBody::NautilusArmor),
    ("minecraft:zombie_nautilus", true, MountBody::NautilusArmor),
];

/// The slots of the mount `identifier`; an unknown mount keeps the horse's.
pub fn mount_slots(identifier: Option<&str>) -> MountSlots {
    let (saddle, body) = MOUNTS
        .iter()
        .find(|(id, ..)| Some(*id) == identifier)
        .map_or((true, MountBody::HorseArmor), |(_, saddle, body)| {
            (*saddle, *body)
        });
    MountSlots { saddle, body }
}

/// The horse screen addressing only the equip slots `slots` has.
pub fn mount_kind(slots: MountSlots) -> &'static ContainerKind {
    const fn horse(equip: &'static [(&'static str, &'static [Cell])]) -> ContainerKind {
        kind("horse.horse_screen", "entity.horse.name", equip)
    }
    const BOTH: ContainerKind = horse(&[
        ("horse_equip_items", &[Cell::Storage(0), Cell::Storage(1)]),
        ("container_items", &MOUNT_CHEST),
    ]);
    const SADDLE: ContainerKind = horse(&[
        ("horse_equip_items", &[Cell::Storage(0)]),
        ("container_items", &MOUNT_CHEST),
    ]);
    const BODY: ContainerKind = horse(&[
        ("horse_equip_items", &[Cell::Storage(1)]),
        ("container_items", &MOUNT_CHEST),
    ]);
    const BARE: ContainerKind = horse(&[("container_items", &MOUNT_CHEST)]);
    match (slots.saddle, slots.body == MountBody::None) {
        (true, false) => &BOTH,
        (true, true) => &SADDLE,
        (false, false) => &BODY,
        (false, true) => &BARE,
    }
}

impl ContainerKind {
    /// The cell `index` of `collection` addresses, if it belongs here.
    pub fn cell(&self, collection: &str, index: usize) -> Option<Cell> {
        self.collections
            .iter()
            .find(|(name, _)| *name == collection)
            .and_then(|(_, cells)| cells.get(index).copied())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const STATIONS: [WindowKind; 17] = [
        WindowKind::Furnace,
        WindowKind::BlastFurnace,
        WindowKind::Smoker,
        WindowKind::Enchanting,
        WindowKind::Brewing,
        WindowKind::Anvil,
        WindowKind::Dispenser,
        WindowKind::Dropper,
        WindowKind::Hopper,
        WindowKind::Horse,
        WindowKind::Beacon,
        WindowKind::Loom,
        WindowKind::Grindstone,
        WindowKind::Stonecutter,
        WindowKind::Cartography,
        WindowKind::Smithing,
        WindowKind::Crafter,
    ];

    #[test]
    fn storage_windows_pick_the_chest_by_slot_count() {
        assert_eq!(storage_kind(27, None).screen, "chest.small_chest_screen");
        assert_eq!(storage_kind(54, None).screen, "chest.large_chest_screen");
        assert_eq!(
            storage_kind(27, Some("Barrel")).screen,
            "chest.barrel_screen"
        );
    }

    #[test]
    fn server_chest_menu_rows_map_only_authoritative_cells() {
        let Some(protocol::OpenCells::Generic(lengths)) = WindowKind::Storage.open_cells() else {
            panic!("storage cell contract");
        };
        for &slots in lengths {
            assert!(supported_storage_slots(slots));
            let kind = storage_kind(slots, None);
            for index in 0..slots {
                assert_eq!(
                    kind.cell("container_items", index),
                    Some(Cell::Storage(index as u8))
                );
            }
            assert_eq!(kind.cell("container_items", slots), None);
            assert_eq!(kind.screen == "chest.large_chest_screen", slots == 54);
        }
        for slots in [0, 1, 8, 10, 55, 255, usize::MAX] {
            assert!(!supported_storage_slots(slots));
        }
    }

    // Every station routes to an allow-listed screen and addresses each cell once.
    #[test]
    fn stations_route_to_engine_screens_with_unique_cells() {
        for kind in STATIONS {
            let station = window_kind(kind).unwrap_or_else(|| panic!("{kind:?}"));
            assert!(
                json_ui::is_engine_screen(station.screen),
                "{}",
                station.screen
            );
            let cells: Vec<Cell> = station
                .collections
                .iter()
                .flat_map(|(_, cells)| cells.iter().copied())
                .collect();
            for (index, cell) in cells.iter().enumerate() {
                assert!(!cells[..index].contains(cell), "{kind:?} repeats {cell:?}");
            }
        }
    }

    #[test]
    fn collections_address_their_cells_in_order() {
        let furnace = window_kind(WindowKind::Furnace).unwrap();
        assert_eq!(
            furnace.cell("furnace_fuel_items", 0),
            Some(Cell::Storage(1))
        );
        assert_eq!(furnace.cell("furnace_output_items", 1), None);
        let anvil = window_kind(WindowKind::Anvil).unwrap();
        assert_eq!(anvil.cell("anvil_result_items", 0), Some(Cell::Output));
        assert_eq!(
            anvil.cell("anvil_material_items", 0).map(Cell::hit),
            Some(InventoryCellHit::Craft(2))
        );
    }
}
