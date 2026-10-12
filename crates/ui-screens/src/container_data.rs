//! Station bindings the vanilla container screens read beyond their item
//! cells: progress ratios, empty-slot art, mount grid shapes, and the station
//! controls (enchant options, the anvil name, stonecutter recipes, beacon
//! powers) with the widgets their hit regions press. A bound `#clip_ratio` is
//! the fraction clipped away, so a full bar binds `0`.

use json_ui::{CollectionItem, DataSource, HitKind, HitRegion, Scalar};
use protocol::WindowKind;

use crate::container_actions::{BEACON_LEVEL_FOR, LOOM_PATTERNS, STONECUTTER_CELLS, Widget};
use crate::container_source::{ContainerFrame, ContainerSource};
use assets::entity::mount::{MountBody, MountSlots};
use inventory::inventory_ledger::InventoryTarget;
use ui::IconRef;

/// Beacon power buttons by collection: `(name, effect id, secondary)`.
const BEACON_POWERS: [(&str, i32, bool); 6] = [
    ("speed", 1, false),
    ("haste", 3, false),
    ("resist", 11, false),
    ("jump", 8, false),
    ("strength", 5, false),
    ("regen", 10, true),
];
/// The legacy `id << 16 | aux` values the beacon's payment row names.
const BEACON_PAYMENTS: [(i64, &str); 5] = [
    (742 << 16, "minecraft:netherite_ingot"),
    (388 << 16, "minecraft:emerald"),
    (264 << 16, "minecraft:diamond"),
    (266 << 16, "minecraft:gold_ingot"),
    (265 << 16, "minecraft:iron_ingot"),
];
/// The `#item_id_aux` key the crafter's previewed result draws under; stonecutter keys are negative.
const CRAFTER_OUTPUT_KEY: i64 = i64::MAX;
const CRAFTER_ARROW_POWERED: &str = "textures/ui/redstone_arrow_powered";
const CRAFTER_ARROW_UNPOWERED: &str = "textures/ui/redstone_arrow_unpowered";
/// `$pressed_button_name` prefix of the crafter's disabled-slot buttons.
const CRAFTER_DISABLED_BUTTON: &str = "disabled_slot_";
const TOGGLABLE_SLOT_KEY: &str = "gui.togglable_slot";
const CELL: &str = "textures/ui/cell_image";
const CELL_NORMAL: &str = "textures/ui/cell_image_normal";
const CELL_SELECTED: &str = "textures/ui/cell_image_invert";

const FURNACE_COOK_TICKS: f64 = 200.0;
const FAST_COOK_TICKS: f64 = 100.0;
const BREW_TICKS: f64 = 400.0;
const DEFAULT_FUEL_TOTAL: f64 = 20.0;
/// Bubble heights of the brewing cycle, of the 29 px column (provisional: Java's cycle).
const BUBBLE_HEIGHTS: [f64; 7] = [29.0, 24.0, 20.0, 16.0, 11.0, 6.0, 0.0];

/// Globals for the open station's progress and layout.
pub(crate) fn station_globals(
    source: &dyn ContainerSource,
    data: &mut DataSource,
    kind: WindowKind,
) {
    let ledger = source.player().inventory.ledger();
    let property = |id: i32| ledger.window_data(id).map(f64::from);
    let mut clip = |name: &str, shown: f64| {
        data.set_global(name, Scalar::Num(1.0 - shown.clamp(0.0, 1.0)));
    };
    match kind {
        WindowKind::Furnace | WindowKind::BlastFurnace | WindowKind::Smoker => {
            let total = if kind == WindowKind::Furnace {
                FURNACE_COOK_TICKS
            } else {
                FAST_COOK_TICKS
            };
            clip(
                "#furnace_arrow_ratio",
                property(0).map_or(0.0, |ticks| ticks / total),
            );
            let lit = match (property(1), property(2)) {
                (Some(remaining), Some(duration)) if duration > 0.0 => remaining / duration,
                _ => 0.0,
            };
            clip("#furnace_flame_ratio", lit);
        }
        WindowKind::Brewing => {
            let remaining = property(0).filter(|ticks| *ticks > 0.0);
            clip(
                "#brewing_arrow_ratio",
                remaining.map_or(0.0, |ticks| 1.0 - ticks / BREW_TICKS),
            );
            let bubbles = remaining.map_or(0.0, |ticks| {
                BUBBLE_HEIGHTS[(ticks as usize / 2) % BUBBLE_HEIGHTS.len()] / 29.0
            });
            clip("#brewing_bubbles_ratio", bubbles);
            let total = property(2)
                .filter(|total| *total > 0.0)
                .unwrap_or(DEFAULT_FUEL_TOTAL);
            clip(
                "#brewing_fuel_ratio",
                property(1).map_or(0.0, |fuel| fuel / total),
            );
        }
        WindowKind::Horse => {
            let chest = ledger.storage_slot_count().unwrap_or(2).saturating_sub(2);
            let equipment = source.screen().mount_equipment;
            let slots = equipment.slots().unwrap_or(MountSlots::EMPTY);
            let worn = slots.body != MountBody::None;
            let equip = u32::from(slots.saddle) + u32::from(worn);
            data.set_grid_dimensions("#equip_grid_dimensions", [1, equip]);
            data.set_grid_dimensions("#inv_grid_dimensions", [(chest / 3) as u32, 3]);
            data.set_global("#is_chested", Scalar::Bool(chest > 0));
            let body = |kind: MountBody| slots.body == kind;
            for (name, shown) in [
                ("#has_saddle_slot", slots.saddle),
                (
                    "#has_only_horse_armor_slot",
                    !slots.saddle && body(MountBody::HorseArmor),
                ),
                (
                    "#has_only_carpet_slot",
                    !slots.saddle && body(MountBody::Carpet),
                ),
                (
                    "#has_only_nautilus_armor_slot",
                    !slots.saddle && body(MountBody::NautilusArmor),
                ),
                (
                    "#has_horse_armor_and_saddle_slot",
                    slots.saddle && body(MountBody::HorseArmor),
                ),
                (
                    "#has_carpet_and_saddle_slot",
                    slots.saddle && body(MountBody::Carpet),
                ),
                (
                    "#has_nautilus_armor_and_saddle_slot",
                    slots.saddle && body(MountBody::NautilusArmor),
                ),
            ] {
                data.set_global(name, Scalar::Bool(shown));
            }
        }
        _ => {}
    }
}

/// Collections and globals of the open station's controls.
pub(crate) fn station_controls(
    source: &dyn ContainerSource,
    data: &mut DataSource,
    frame: &ContainerFrame<'_>,
    kind: WindowKind,
) {
    match kind {
        WindowKind::Enchanting => data.set_collection("#enchant_buttons", enchant_buttons(source)),
        WindowKind::Anvil => {
            let name = source.screen().anvil_name.to_owned();
            data.set_global("#text_box_item_name", Scalar::Text(name));
        }
        WindowKind::Stonecutter => data.set_collection("stones", stones(source, frame)),
        WindowKind::Beacon => beacon_buttons(data, source),
        WindowKind::Loom => loom_controls(source, data),
        WindowKind::Cartography => data.set_global("#is_none_mode", Scalar::Bool(true)),
        WindowKind::Crafter => crafter_controls(data, source, frame),
        _ => {}
    }
}

/// Icons for `#item_id_aux` renderers: the beacon's payment row by its legacy
/// ids, and the stonecutter's recipes by the negative keys `stones` binds.
pub fn id_aux_icons(
    source: &dyn ContainerSource,
    frame: &ContainerFrame<'_>,
    icon: impl Fn(&str) -> Option<IconRef>,
) -> Vec<(i64, IconRef)> {
    match source.player().inventory.ledger().window_kind() {
        Some(WindowKind::Beacon) => BEACON_PAYMENTS
            .iter()
            .filter_map(|(key, id)| Some((*key, icon(id)?)))
            .collect(),
        Some(WindowKind::Crafter) => frame
            .recipe_output
            .and_then(|(icon, _)| *icon)
            .map(|icon| vec![(CRAFTER_OUTPUT_KEY, icon)])
            .unwrap_or_default(),
        Some(WindowKind::Stonecutter) => frame
            .recipe_icons
            .iter()
            .enumerate()
            .filter_map(|(index, icon)| Some((stone_key(index), (*icon)?)))
            .collect(),
        _ => Vec::new(),
    }
}

/// The crafter screen's disabled-slot buttons, powered arrow and previewed result.
fn crafter_controls(
    data: &mut DataSource,
    source: &dyn ContainerSource,
    frame: &ContainerFrame<'_>,
) {
    let screen = source.screen();
    for slot in 0..9u8 {
        data.set_global(
            format!("#button_visible{slot}"),
            Scalar::Bool(screen.crafter_disabled[usize::from(slot)]),
        );
    }
    let arrow = if screen.crafter_powered == Some(true) {
        CRAFTER_ARROW_POWERED
    } else {
        CRAFTER_ARROW_UNPOWERED
    };
    data.set_global("#redstone_arrow_texture", Scalar::Text(arrow.to_owned()));
    let output = frame.recipe_output;
    let shown = output.filter(|(icon, _)| icon.is_some());
    data.set_global(
        "#crafter_output_item",
        Scalar::Num(if shown.is_some() {
            CRAFTER_OUTPUT_KEY as f64
        } else {
            0.0
        }),
    );
    let count = output.map_or(0, |(_, stack)| stack.count);
    data.set_global(
        "#output_stack_count",
        Scalar::Text(match count {
            0 | 1 => String::new(),
            2..=99 => count.to_string(),
            _ => "99+".to_owned(),
        }),
    );
    let name = output
        .and_then(|(_, stack)| frame.item_names.get(&(stack.network_id, stack.metadata)))
        .map_or_else(String::new, |name| name.to_string());
    data.set_global("#crafting_preview_info", Scalar::Text(name));
}

fn stone_key(index: usize) -> i64 {
    -1 - index as i64
}

/// The station widget a hit region presses, for regions that are not item cells.
pub(crate) fn widget_hit(screen: &str, region: &HitRegion) -> Option<Widget> {
    let index = region.collection_index.unwrap_or(0);
    let collection = region.collection.as_deref();
    Some(match (screen, collection) {
        ("enchanting.enchanting_screen", Some("#enchant_buttons"))
            if region.name == "selectable_button" =>
        {
            Widget::EnchantOption(u8::try_from(index).ok()?)
        }
        ("anvil.anvil_screen", _) if region.kind == HitKind::EditBox => Widget::AnvilName,
        ("loom.loom_screen", Some("patterns")) => Widget::LoomPatternAt(u8::try_from(index).ok()?),
        ("stonecutter.stonecutter_screen", Some("stones")) if index < STONECUTTER_CELLS => {
            Widget::StonecutterRecipe(u8::try_from(index).ok()?)
        }
        ("redstone.crafter_screen", None) => {
            let slot = region
                .name
                .strip_prefix(CRAFTER_DISABLED_BUTTON)?
                .strip_suffix("_button")?;
            Widget::CrafterSlot(slot.parse().ok()?)
        }
        ("beacon.beacon_screen", Some("extra")) => Widget::BeaconUpgrade,
        ("beacon.beacon_screen", Some("confirm")) => Widget::BeaconConfirm,
        ("beacon.beacon_screen", Some(name)) => {
            let (_, id, secondary) = BEACON_POWERS.iter().find(|power| power.0 == name)?;
            Widget::BeaconEffect {
                id: *id,
                secondary: *secondary,
            }
        }
        _ => return None,
    })
}

/// The three option rows: selectable when the player has the levels and lapis
/// (creative always does), with the vanilla clue and cost hover text.
fn enchant_buttons(source: &dyn ContainerSource) -> Vec<CollectionItem> {
    let player_runtime = source.player();
    let ledger = player_runtime.inventory.ledger();
    let options = ledger.enchant_options().unwrap_or(&[]);
    let level = source.experience_level();
    let lapis = ledger
        .target_stack(InventoryTarget::Craft(15))
        .map_or(0, |stack| u32::from(stack.count));
    let creative =
        player_runtime.facts.player_game_mode() == Some(protocol::PlayerGameMode::Creative);
    (0..3u32)
        .map(|row| {
            let item = CollectionItem::default();
            let Some(option) = options.get(row as usize) else {
                return item
                    .with("#selectable_button_visibility", Scalar::Bool(false))
                    .with("#unselectable_button_visibility", Scalar::Bool(false));
            };
            let has_levels = creative || level >= u32::from(option.cost);
            let has_lapis = creative || lapis > row;
            let selectable = has_levels && has_lapis;
            item.with("#selectable_button_visibility", Scalar::Bool(selectable))
                .with("#unselectable_button_visibility", Scalar::Bool(!selectable))
                .with("#selectable_dust_is_visible", Scalar::Bool(selectable))
                .with("#unselectable_dust_is_visible", Scalar::Bool(!selectable))
                .with("#cost", Scalar::Text(option.cost.to_string()))
                // The rune font is not carried; the galactic text stays unset.
                .with("#runes", Scalar::Text(String::new()))
                .with(
                    "#hover_text",
                    Scalar::Text(enchant_hover(
                        source,
                        option,
                        row + 1,
                        has_levels,
                        has_lapis,
                        creative,
                    )),
                )
        })
        .collect()
}

fn enchant_hover(
    source: &dyn ContainerSource,
    option: &protocol::EnchantOption,
    levels: u32,
    has_levels: bool,
    has_lapis: bool,
    creative: bool,
) -> String {
    let text = |key: &str| {
        source
            .translation(key)
            .map_or_else(|| key.to_owned(), |text| text.to_string())
    };
    let clue = option
        .enchants
        .first()
        .map(|(id, level)| {
            let name = source.enchantment_name(i16::from(*id), *level);
            text("container.enchant.clue").replacen("%s", &name, 1)
        })
        .unwrap_or_default();
    let mut lines = vec![clue];
    if !creative {
        if has_levels {
            let (lapis, level) = if levels == 1 {
                ("container.enchant.lapis.one", "container.enchant.level.one")
            } else {
                (
                    "container.enchant.lapis.many",
                    "container.enchant.level.many",
                )
            };
            let lapis_color = if has_lapis { "§7" } else { "§c" };
            lines.push(format!(
                "{lapis_color}{}",
                text(lapis).replacen("%d", &levels.to_string(), 1)
            ));
            lines.push(format!(
                "§7{}",
                text(level).replacen("%d", &levels.to_string(), 1)
            ));
        } else {
            let requirement = text("container.enchant.levelrequirement");
            lines.push(format!(
                "§c{}",
                requirement.replacen("%d", &option.cost.to_string(), 1)
            ));
        }
    }
    lines.join("\n")
}

/// The stonecutter's recipe cells for the input, the chosen one inverted.
fn stones(source: &dyn ContainerSource, frame: &ContainerFrame<'_>) -> Vec<CollectionItem> {
    let inventory = &source.player().inventory;
    let chosen = inventory
        .active_screen_recipe(source.screen().recipe_choice)
        .map(|recipe| recipe.id);
    let options = inventory.stonecutter_options();
    let total = options.len().min(STONECUTTER_CELLS);
    options
        .iter()
        .take(STONECUTTER_CELLS)
        .enumerate()
        .map(|(index, recipe)| {
            let output = recipe.output;
            let name = output
                .and_then(|output| {
                    frame
                        .item_names
                        .get(&(output.network_id, u32::from(output.aux)))
                })
                .map_or_else(String::new, |name| name.to_string());
            let count = output.map_or(0, |output| output.count);
            let texture = if chosen == Some(recipe.id) {
                CELL_SELECTED
            } else {
                CELL_NORMAL
            };
            CollectionItem::default()
                .with("#item_id_aux", Scalar::Num(stone_key(index) as f64))
                .with(
                    "#item_stack_count",
                    Scalar::Text(if count > 1 {
                        count.to_string()
                    } else {
                        String::new()
                    }),
                )
                .with(
                    "#stone_cell_background_texture",
                    Scalar::Text(texture.to_owned()),
                )
                .with("#stone_selector_total_items", Scalar::Num(total as f64))
                .with("#hover_text", Scalar::Text(name))
        })
        .collect()
}

/// Publishes pattern-cell artwork and the selected banner result from the current inventory.
fn loom_controls(source: &dyn ContainerSource, data: &mut DataSource) {
    let ledger = source.player().inventory.ledger();
    let banner = ledger
        .target_stack(InventoryTarget::Craft(9))
        .filter(|stack| {
            ledger
                .negotiated_item_entry(stack.network_id)
                .is_some_and(|entry| entry.identifier.as_ref() == "minecraft:banner")
        });
    let dye = ledger
        .target_stack(InventoryTarget::Craft(10))
        .and_then(|stack| {
            let identifier = ledger
                .negotiated_item_entry(stack.network_id)?
                .identifier
                .as_ref();
            assets::banner::dye_color(identifier).or_else(|| {
                (identifier == "minecraft:dye")
                    .then(|| {
                        u8::try_from(stack.metadata)
                            .ok()
                            .filter(|color| *color < 16)
                    })
                    .flatten()
            })
        });
    let chosen = source.screen().loom_pattern;
    let mut cells = Vec::new();
    let mut result = (String::new(), String::new());
    if let (Some(banner), Some(dye)) = (banner, dye) {
        let base = u8::try_from(banner.metadata)
            .ok()
            .filter(|color| *color < 16)
            .unwrap_or(15);
        let current = protocol::item_banner_patterns(&banner.extra_data);
        result = banner_bindings(base, &current, chosen.map(|code| (code, dye)));
        for pattern in LOOM_PATTERNS {
            let texture = if chosen == Some(pattern) {
                CELL_SELECTED
            } else {
                CELL_NORMAL
            };
            let (patterns, colors) = banner_bindings(15, &[], Some((pattern, 0)));
            cells.push(
                CollectionItem::default()
                    .with(
                        "#pattern_cell_background_texture",
                        Scalar::Text(texture.to_owned()),
                    )
                    .with(
                        "#pattern_selector_total_items",
                        Scalar::Num(LOOM_PATTERNS.len() as f64),
                    )
                    .with("#banner_patterns", Scalar::Text(patterns))
                    .with("#banner_colors", Scalar::Text(colors)),
            );
        }
    }
    data.set_collection("patterns", cells);
    data.set_global("#result_patterns", Scalar::Text(result.0));
    data.set_global("#result_colors", Scalar::Text(result.1));
}

/// Encodes the base, existing overlays and selected overlay in native UI order.
fn banner_bindings(
    base: u8,
    current: &[protocol::ItemBannerPattern],
    selected: Option<(&str, u8)>,
) -> (String, String) {
    let mut patterns = vec!["0".to_owned()];
    let mut colors = vec![base.to_string()];
    for (code, color) in current
        .iter()
        .map(|layer| (layer.pattern.as_ref(), layer.color))
        .chain(selected)
        .take(assets::banner::MAX_BANNER_LAYERS)
    {
        if let Some(id) = assets::banner::pattern_ui_id(code) {
            patterns.push(id.to_string());
            colors.push(color.to_string());
        }
    }
    (patterns.join("_"), colors.join("_"))
}

#[cfg(test)]
mod banner_tests {
    use std::{
        collections::{BTreeMap, BTreeSet},
        sync::Arc,
    };

    use super::*;
    use crate::container_source::{NameSources, ScreenView};

    static EXPANDED: BTreeSet<u32> = BTreeSet::new();
    static SCROLL: BTreeMap<String, f64> = BTreeMap::new();

    /// Answers only what the loom bindings read.
    struct LoomSource {
        player: player_state::PlayerState,
        screen: ScreenView<'static>,
    }

    impl ContainerSource for LoomSource {
        fn player(&self) -> &player_state::PlayerState {
            &self.player
        }
        fn screen(&self) -> &ScreenView<'_> {
            &self.screen
        }
        fn translation(&self, _: &str) -> Option<Arc<str>> {
            None
        }
        fn experience_level(&self) -> u32 {
            0
        }
        fn enchantment_name(&self, _: i16, _: u8) -> String {
            String::new()
        }
        fn recipe_filtering(&self) -> bool {
            false
        }
        fn item_name_generation(&self) -> u64 {
            0
        }
        fn book_entries(&self) -> Vec<crate::recipe_book::BookEntry<'_>> {
            Vec::new()
        }
        fn furnace_entries(&self) -> crate::furnace_book::Entries<'_> {
            unreachable!("the loom reads no furnace entries")
        }
        fn furnace_projection(&self) -> Arc<[usize]> {
            Arc::from([])
        }
        fn furnace_category(&self, _: &protocol::ScreenRecipe) -> u8 {
            0
        }
        fn name_sources(&self) -> NameSources {
            NameSources {
                components: None,
                lang: None,
            }
        }
    }

    /// Supplies authoritative loom input slots and negotiated item identities.
    fn loom_fixture() -> LoomSource {
        let mut player = player_state::PlayerState::new(1);
        player
            .inventory
            .publish_inventory_authority(protocol::InventoryAuthority::Server);
        let entries = [
            "minecraft:banner",
            "minecraft:yellow_dye",
            "minecraft:stone",
        ]
        .into_iter()
        .enumerate()
        .map(|(index, name)| protocol::ItemRegistryEntry {
            components: None,
            identifier: name.into(),
            network_id: index as i32 + 1,
            component_based: false,
            version: protocol::ItemRegistryVersion::None,
            component_digest: [0; 32],
            negotiated_max_stack_size: Some(64),
            canonical_empty_component_data: true,
            item_tags: Arc::from([]),
        })
        .collect::<Vec<_>>();
        let ledger = player.inventory.ledger_mut();
        ledger.apply_registry(&protocol::ItemRegistryEvent {
            entries: entries.into(),
        });
        ledger.apply(&protocol::InventoryEvent::Open(
            protocol::ContainerOpenEvent {
                container: protocol::ContainerIdentity::window(7),
                window_type: protocol::WINDOW_TYPE_LOOM,
                position: [0, 64, 0],
                runtime_entity_id: -1,
            },
        ));
        LoomSource {
            player,
            screen: ScreenView {
                mount_equipment: Default::default(),
                anvil_name: "",
                recipe_choice: None,
                beacon: (0, 0),
                beacon_level: None,
                loom_pattern: Some("bo"),
                crafter_disabled: [false; 9],
                crafter_powered: None,
                furnace_book_open: false,
                furnace_tab: None,
                creative_tab: 0,
                creative_wide: false,
                creative_expanded: &EXPANDED,
                search: "",
                container_scroll: &SCROLL,
                text_focused: false,
                book: None,
            },
        }
    }

    #[test]
    fn loom_preview_requires_negotiated_banner_and_dye_inputs() {
        let mut source = loom_fixture();
        let mut empty = DataSource::new();
        loom_controls(&source, &mut empty);
        for (banner, dye, shown) in [(1, 2, true), (3, 2, false), (1, 3, false), (0, 2, false)] {
            let mut slots = vec![protocol::NetworkItemStack::empty(); protocol::UI_SLOT_COUNT];
            for (slot, network_id) in [(9, banner), (10, dye)] {
                slots[slot] = protocol::NetworkItemStack {
                    network_id,
                    count: u16::from(network_id != 0),
                    metadata: 15,
                    ..protocol::NetworkItemStack::empty()
                };
            }
            source
                .player
                .inventory
                .ledger_mut()
                .apply(&protocol::InventoryEvent::Content(
                    protocol::InventoryContentEvent {
                        container: protocol::ContainerIdentity {
                            window_id: Some(protocol::UI_INVENTORY_WINDOW_ID),
                            slot_type: Some(0),
                            dynamic_id: None,
                        },
                        slots: slots.into(),
                        storage_item: protocol::NetworkItemStack::empty(),
                    },
                ));
            let mut data = DataSource::new();
            loom_controls(&source, &mut data);
            assert_eq!(data != empty, shown, "banner={banner} dye={dye}");
        }
    }

    #[test]
    fn loom_result_preserves_existing_layers_and_adds_the_selected_pattern() {
        let current = [protocol::ItemBannerPattern {
            pattern: "bs".into(),
            color: 1,
        }];
        assert_eq!(
            banner_bindings(15, &current, Some(("bo", 4))),
            ("0_28_1".into(), "15_1_4".into())
        );
        assert_eq!(banner_bindings(0, &[], None), ("0".into(), "0".into()));
        let full = vec![current[0].clone(); assets::banner::MAX_BANNER_LAYERS];
        let (patterns, colors) = banner_bindings(15, &full, Some(("bo", 4)));
        assert_eq!(patterns.split('_').count(), full.len() + 1);
        assert_eq!(colors.split('_').count(), full.len() + 1);
        assert_eq!(
            banner_bindings(15, &current, Some(("custom:unknown", 4))),
            ("0_28".into(), "15_1".into())
        );
    }
}

/// One collection per beacon button, as the controller names them.
fn beacon_buttons(data: &mut DataSource, source: &dyn ContainerSource) {
    let state = source.screen();
    let (primary, secondary) = state.beacon;
    let unlocked = |needed: u8| state.beacon_level.is_none_or(|have| have >= needed);
    let button = |active: bool, selected: bool, hover: String| {
        vec![
            CollectionItem::default()
                .with("#button_visible", Scalar::Bool(true))
                .with("#active", Scalar::Bool(active && !selected))
                .with("#inactive", Scalar::Bool(!active))
                .with("#selected", Scalar::Bool(active && selected))
                .with("#button_hover", Scalar::Text(hover)),
        ]
    };
    let name = |id: i32| {
        let key = match id {
            1 => "effect.moveSpeed",
            3 => "effect.digSpeed",
            11 => "effect.resistance",
            8 => "effect.jump",
            5 => "effect.damageBoost",
            _ => "effect.regeneration",
        };
        source
            .translation(key)
            .map_or_else(|| key.to_owned(), |text| text.to_string())
    };
    for (collection, id, is_secondary) in BEACON_POWERS {
        let needed = BEACON_LEVEL_FOR
            .iter()
            .find(|(effect, _)| *effect == id)
            .map_or(4, |(_, level)| *level);
        let selected = if is_secondary {
            secondary == id
        } else {
            primary == id
        };
        data.set_collection(collection, button(unlocked(needed), selected, name(id)));
    }
    let upgrade = primary != 0 && unlocked(4);
    data.set_collection(
        "extra",
        button(upgrade, upgrade && secondary == primary, name(primary)),
    );
    data.set_collection("confirm", button(primary != 0, false, String::new()));
    data.set_collection("cancel", button(true, false, String::new()));
}

/// How many cells of `collection` the open window fills; the mount chest shows
/// only the columns the mount carries.
pub(crate) fn collection_len(
    player_runtime: &player_state::PlayerState,
    collection: &str,
    cells: usize,
) -> usize {
    let ledger = player_runtime.inventory.ledger();
    match (ledger.window_kind(), collection) {
        (Some(WindowKind::Horse), "container_items") => ledger
            .storage_slot_count()
            .map_or(0, |count| count.saturating_sub(2))
            .min(cells),
        _ => cells,
    }
}

/// Empty-slot silhouettes and cell art the brewing stand and loom bind per cell.
pub(crate) fn decorate(
    source: &dyn ContainerSource,
    collection: &str,
    empty: bool,
    item: CollectionItem,
) -> CollectionItem {
    let crafter = source.player().inventory.ledger().window_kind() == Some(WindowKind::Crafter);
    match collection {
        // An empty crafter slot offers to disable itself.
        "container_items" if crafter && empty => item.with(
            "#hover_text",
            Scalar::Text(
                source
                    .translation(TOGGLABLE_SLOT_KEY)
                    .map_or_else(|| TOGGLABLE_SLOT_KEY.to_owned(), |text| text.to_string()),
            ),
        ),
        "brewing_result_items" => item.with("#empty_bottle_image_visible", Scalar::Bool(empty)),
        "brewing_fuel_item" => item.with("#empty_fuel_image_visible", Scalar::Bool(empty)),
        "loom_input_items" | "loom_dye_items" | "loom_material_items" => {
            item.with("#empty_image_visible", Scalar::Bool(empty)).with(
                "#container_cell_background_texture",
                Scalar::Text(CELL.to_owned()),
            )
        }
        _ => item,
    }
}
