//! Station bindings the vanilla container screens read beyond their item
//! cells: progress ratios, empty-slot art, mount grid shapes, and the station
//! controls (enchant options, the anvil name, stonecutter recipes, beacon
//! powers) with the widgets their hit regions press. A bound `#clip_ratio` is
//! the fraction clipped away, so a full bar binds `0`.

use json_ui::{CollectionItem, DataSource, HitKind, HitRegion, Scalar};
use protocol::WindowKind;

use super::container_kinds::{MountBody, mount_slots};
use crate::ui_runtime::UiRuntime;
use crate::ui_runtime::presentation::screens::{BEACON_LEVEL_FOR, STONECUTTER_CELLS, Widget};
use crate::ui_runtime::screen_recipes::LOOM_PATTERNS;
use inventory::inventory_ledger::InventoryTarget;
use {super::super::HudFrame, ui::IconRef};

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

/// Reads the open station's block for its screen: the beacon's pyramid level,
/// the crafter's disabled slots and its `triggered_bit`.
pub fn observe_station_block(
    player_runtime: &player_state::PlayerState,
    runtime: &mut UiRuntime,
    stream: Option<&chunk_pipeline::WorldStream>,
    station_triggered: impl FnOnce([i32; 3]) -> Option<bool>,
    now_millis: u64,
) {
    let ledger = runtime.inventory_ledger(player_runtime);
    let (Some(kind), Some(position), Some(stream)) =
        (ledger.window_kind(), ledger.window_position(), stream)
    else {
        return;
    };
    if kind == WindowKind::Horse {
        let identifier = ledger
            .window_actor()
            .and_then(|unique| stream.authority().actor_by_unique_id(unique))
            .and_then(|actor| match &actor.kind {
                protocol::ActorKind::Entity { identifier } => Some(identifier.clone()),
                protocol::ActorKind::Player { .. } => None,
            });
        runtime.screen_state_mut().mount_identifier = identifier;
        return;
    }
    let nbt = stream.block_entity_compound(position);
    let integer = |key: &str| nbt.as_ref().and_then(|nbt| nbt.integer(key));
    match kind {
        WindowKind::Beacon => {
            runtime.screen_state_mut().beacon_level =
                integer("Levels").and_then(|levels| u8::try_from(levels).ok());
        }
        WindowKind::Crafter => {
            let disabled = integer("disabled_slots").map_or(0, |mask| mask as u16);
            let powered = station_triggered(position).unwrap_or(false);
            runtime
                .screen_state_mut()
                .crafter
                .observe(disabled, powered, now_millis);
        }
        _ => {}
    }
}

/// A boolean block state from canonical state JSON, plain or typed.
/// Reads a named boolean state from the canonical block state supplied by the app.
pub fn state_bit(canonical: &str, name: &str) -> Option<bool> {
    let states = serde_json::from_str::<serde_json::Value>(canonical).ok()?;
    let value = states.get(name)?;
    let value = value.get("value").unwrap_or(value);
    value
        .as_bool()
        .or_else(|| value.as_u64().map(|bit| bit != 0))
}

/// Globals for the open station's progress and layout.
pub(super) fn station_globals(
    player_runtime: &player_state::PlayerState,
    data: &mut DataSource,
    runtime: &UiRuntime,
    kind: WindowKind,
) {
    let ledger = runtime.inventory_ledger(player_runtime);
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
            let slots = mount_slots(runtime.screen_state().mount_identifier.as_deref());
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
pub(super) fn station_controls(
    player_runtime: &player_state::PlayerState,
    data: &mut DataSource,
    runtime: &UiRuntime,
    frame: &HudFrame,
    kind: WindowKind,
) {
    match kind {
        WindowKind::Enchanting => {
            data.set_collection("#enchant_buttons", enchant_buttons(player_runtime, runtime))
        }
        WindowKind::Anvil => {
            let name = runtime.screen_state().anvil_name.clone();
            data.set_global("#text_box_item_name", Scalar::Text(name));
        }
        WindowKind::Stonecutter => {
            data.set_collection("stones", stones(player_runtime, runtime, frame))
        }
        WindowKind::Beacon => beacon_buttons(data, runtime),
        WindowKind::Loom => data.set_collection("patterns", patterns(player_runtime, runtime)),
        WindowKind::Cartography => data.set_global("#is_none_mode", Scalar::Bool(true)),
        WindowKind::Crafter => crafter_controls(data, runtime, frame),
        _ => {}
    }
}

/// Icons for `#item_id_aux` renderers: the beacon's payment row by its legacy
/// ids, and the stonecutter's recipes by the negative keys `stones` binds.
pub(super) fn id_aux_icons(
    player_runtime: &player_state::PlayerState,
    runtime: &UiRuntime,
    frame: &HudFrame,
    icon: impl Fn(&str) -> Option<IconRef>,
) -> Vec<(i64, IconRef)> {
    match runtime.inventory_ledger(player_runtime).window_kind() {
        Some(WindowKind::Beacon) => BEACON_PAYMENTS
            .iter()
            .filter_map(|(key, id)| Some((*key, icon(id)?)))
            .collect(),
        Some(WindowKind::Crafter) => frame
            .window_icons
            .recipe_output
            .as_ref()
            .and_then(|(icon, _)| *icon)
            .map(|icon| vec![(CRAFTER_OUTPUT_KEY, icon)])
            .unwrap_or_default(),
        Some(WindowKind::Stonecutter) => frame
            .window_icons
            .recipe
            .iter()
            .enumerate()
            .filter_map(|(index, icon)| Some((stone_key(index), (*icon)?)))
            .collect(),
        _ => Vec::new(),
    }
}

/// The crafter screen's disabled-slot buttons, powered arrow and previewed result.
fn crafter_controls(data: &mut DataSource, runtime: &UiRuntime, frame: &HudFrame) {
    let crafter = &runtime.screen_state().crafter;
    for slot in 0..9u8 {
        data.set_global(
            format!("#button_visible{slot}"),
            Scalar::Bool(crafter.is_disabled(slot)),
        );
    }
    let arrow = if crafter.powered {
        CRAFTER_ARROW_POWERED
    } else {
        CRAFTER_ARROW_UNPOWERED
    };
    data.set_global("#redstone_arrow_texture", Scalar::Text(arrow.to_owned()));
    let output = frame.window_icons.recipe_output.as_ref();
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
pub(super) fn widget_hit(screen: &str, region: &HitRegion) -> Option<Widget> {
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
fn enchant_buttons(
    player_runtime: &player_state::PlayerState,
    runtime: &UiRuntime,
) -> Vec<CollectionItem> {
    let ledger = runtime.inventory_ledger(player_runtime);
    let options = ledger.enchant_options().unwrap_or(&[]);
    let level = runtime.hud().experience().map_or(0, |xp| xp.level);
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
                        runtime,
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
    runtime: &UiRuntime,
    option: &protocol::EnchantOption,
    levels: u32,
    has_levels: bool,
    has_lapis: bool,
    creative: bool,
) -> String {
    let text = |key: &str| {
        runtime
            .translation(key)
            .map_or_else(|| key.to_owned(), |text| text.to_string())
    };
    let clue = option
        .enchants
        .first()
        .map(|(id, level)| {
            let name =
                super::super::inventory_tooltip::enchantment_name(runtime, i16::from(*id), *level);
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
fn stones(
    player_runtime: &player_state::PlayerState,
    runtime: &UiRuntime,
    frame: &HudFrame,
) -> Vec<CollectionItem> {
    let chosen = runtime
        .active_screen_recipe(player_runtime)
        .map(|recipe| recipe.id);
    let options = runtime.stonecutter_options(player_runtime);
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

/// The loom's pattern cells once a banner and a dye are in, the chosen one
/// inverted. The banner preview renderer is not drawn.
fn patterns(
    player_runtime: &player_state::PlayerState,
    runtime: &UiRuntime,
) -> Vec<CollectionItem> {
    let ledger = runtime.inventory_ledger(player_runtime);
    let loaded = |slot: u8| ledger.target_stack(InventoryTarget::Craft(slot)).is_some();
    if !(loaded(9) && loaded(10)) {
        return Vec::new();
    }
    let chosen = runtime.screen_state().loom_pattern.as_deref();
    let total = LOOM_PATTERNS.len() as f64;
    LOOM_PATTERNS
        .iter()
        .map(|pattern| {
            let texture = if chosen == Some(*pattern) {
                CELL_SELECTED
            } else {
                CELL_NORMAL
            };
            CollectionItem::default()
                .with(
                    "#pattern_cell_background_texture",
                    Scalar::Text(texture.to_owned()),
                )
                .with("#pattern_selector_total_items", Scalar::Num(total))
        })
        .collect()
}

/// One collection per beacon button, as the controller names them.
fn beacon_buttons(data: &mut DataSource, runtime: &UiRuntime) {
    let state = runtime.screen_state();
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
        runtime
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
pub(super) fn collection_len(
    player_runtime: &player_state::PlayerState,
    runtime: &UiRuntime,
    collection: &str,
    cells: usize,
) -> usize {
    match (
        runtime.inventory_ledger(player_runtime).window_kind(),
        collection,
    ) {
        (Some(WindowKind::Horse), "container_items") => runtime
            .inventory_ledger(player_runtime)
            .storage_slot_count()
            .map_or(0, |count| count.saturating_sub(2))
            .min(cells),
        _ => cells,
    }
}

/// Empty-slot silhouettes and cell art the brewing stand and loom bind per cell.
pub(super) fn decorate(
    player_runtime: &player_state::PlayerState,
    runtime: &UiRuntime,
    collection: &str,
    empty: bool,
    item: CollectionItem,
) -> CollectionItem {
    let crafter =
        runtime.inventory_ledger(player_runtime).window_kind() == Some(WindowKind::Crafter);
    match collection {
        // An empty crafter slot offers to disable itself.
        "container_items" if crafter && empty => item.with(
            "#hover_text",
            Scalar::Text(
                runtime
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
