//! Item tooltip text: name, enchantments and lore.

use protocol::NetworkItemStack;

use super::UiRuntime;
use super::hud_layout::TooltipLine;

const NAME_COLOR: [u8; 4] = [255, 255, 255, 255];
const ENCHANT_COLOR: [u8; 4] = [170, 170, 170, 255];
const LORE_COLOR: [u8; 4] = [170, 0, 170, 255];

/// The language key suffix of an enchantment id, in protocol order.
const fn enchantment_key(id: i16) -> Option<&'static str> {
    Some(match id {
        0 => "protect.all",
        1 => "protect.fire",
        2 => "protect.fall",
        3 => "protect.explosion",
        4 => "protect.projectile",
        5 => "thorns",
        6 => "oxygen",
        7 => "waterWalker",
        8 => "waterWorker",
        9 => "damage.all",
        10 => "damage.undead",
        11 => "damage.arthropods",
        12 => "knockback",
        13 => "fire",
        14 => "lootBonus",
        15 => "digging",
        16 => "untouching",
        17 => "durability",
        18 => "lootBonusDigger",
        19 => "arrowDamage",
        20 => "arrowKnockback",
        21 => "arrowFire",
        22 => "arrowInfinite",
        23 => "lootBonusFishing",
        24 => "fishingSpeed",
        25 => "frostwalker",
        26 => "mending",
        27 => "curse.binding",
        28 => "curse.vanishing",
        29 => "tridentImpaling",
        30 => "tridentRiptide",
        31 => "tridentLoyalty",
        32 => "tridentChanneling",
        33 => "crossbowMultishot",
        34 => "crossbowPiercing",
        35 => "crossbowQuickCharge",
        36 => "soul_speed",
        37 => "swift_sneak",
        38 => "wind_burst",
        39 => "density",
        40 => "breach",
        41 => "lunge",
        _ => return None,
    })
}

fn level_text(runtime: &UiRuntime, level: u8) -> String {
    runtime
        .translation(&format!("enchantment.level.{level}"))
        .map_or_else(|| level.to_string(), |text| text.to_string())
}

/// An enchantment's localized name and level numeral (`Sharpness II`).
pub(super) fn enchantment_name(runtime: &UiRuntime, id: i16, level: u8) -> String {
    let label = enchantment_key(id)
        .and_then(|key| runtime.translation(&format!("enchantment.{key}")))
        .map_or_else(|| format!("Enchantment {id}"), |text| text.to_string());
    format!("{label} {}", level_text(runtime, level))
}

/// The stack's name for both the selected-item HUD and inventory tooltip.
/// Response corrections override retained `display.Name`, which overrides the
/// localized item identity. A present, empty NBT name remains a custom name.
pub(super) fn name_line(
    runtime: &UiRuntime,
    identifier: Option<&str>,
    stated_name: Option<&str>,
    display: &protocol::ItemDisplay,
) -> Option<TooltipLine> {
    let custom_name = stated_name.or(display.name.as_deref());
    let mut name = custom_name
        .map(str::to_owned)
        .or_else(|| identifier.map(|id| runtime.localized_item_name(id)))?;
    let formatting = identifier
        .and_then(|id| runtime.item_components(id))
        .and_then(crate::ui_runtime::item_facts::name_format);
    let color = formatting.map_or(NAME_COLOR, |(_, [r, g, b])| [r, g, b, 255]);
    if let Some((code, _)) = formatting {
        // Keep the item's already-resolved native format code, rather than
        // reverse-mapping a duplicated component RGB palette in the UI bridge.
        name.insert(0, code);
        name.insert(0, '§');
    }
    if custom_name.is_some() {
        name.insert_str(0, "§o");
    }
    name.push_str("§r");
    Some(TooltipLine { text: name, color })
}

/// The tooltip for one stack; a server-stated name wins over the item's own.
pub(super) fn tooltip_lines(
    runtime: &UiRuntime,
    stack: &NetworkItemStack,
    identifier: Option<&str>,
    stated_name: Option<&str>,
) -> Vec<TooltipLine> {
    let facts = runtime.stack_facts(stack, identifier);
    let display = &facts.display;
    let mut lines = vec![
        name_line(runtime, identifier, stated_name, &display).unwrap_or_else(|| TooltipLine {
            text: "Unknown Item".to_owned(),
            color: NAME_COLOR,
        }),
    ];
    for (id, level) in &display.enchantments {
        lines.push(TooltipLine {
            text: enchantment_name(runtime, *id, *level),
            color: ENCHANT_COLOR,
        });
    }
    lines.extend(display.lore.iter().map(|line| TooltipLine {
        text: line.to_string(),
        color: LORE_COLOR,
    }));
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_protocol_enchantment_has_a_key() {
        assert!((0..=41).all(|id| enchantment_key(id).is_some()));
        assert!(enchantment_key(42).is_none());
    }
}
