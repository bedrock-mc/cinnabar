//! Item tooltip text: name, potion effects, enchantments and lore.

use assets::facts::PotionForm;
use protocol::NetworkItemStack;

use super::UiRuntime;
use super::hud_layout::TooltipLine;

const NAME_COLOR: [u8; 4] = [255, 255, 255, 255];
const ENCHANT_COLOR: [u8; 4] = [170, 170, 170, 255];
const LORE_COLOR: [u8; 4] = [170, 0, 170, 255];
const BENEFICIAL_EFFECT_COLOR: [u8; 4] = [85, 85, 255, 255];
const HARMFUL_EFFECT_COLOR: [u8; 4] = [255, 85, 85, 255];

/// The potion form of a vanilla potion item identifier.
fn potion_form(identifier: &str) -> Option<PotionForm> {
    assets::facts::ItemFacts::builtin()
        .ok()?
        .by_identifier(identifier)?
        .potion_form
}

/// The vanilla potion type and form of a potion item stack.
fn potion(
    identifier: Option<&str>,
    metadata: u32,
) -> Option<(&'static assets::potion::PotionDescriptor, PotionForm)> {
    let form = potion_form(identifier?)?;
    let descriptor = assets::potion::potion_descriptor(i16::try_from(metadata).ok()?)?;
    Some((descriptor, form))
}

/// Formats an effect duration in ticks as `m:ss`, truncating partial seconds.
fn duration_text(ticks: u32) -> String {
    let seconds = ticks / 20;
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

/// One line per potion effect: name, potency numeral and duration, blue for
/// beneficial and red for harmful effects (`Speed II (1:30)`).
fn potion_effect_lines(
    runtime: &UiRuntime,
    descriptor: &assets::potion::PotionDescriptor,
    form: &PotionForm,
) -> impl Iterator<Item = TooltipLine> {
    descriptor.effects.iter().map(move |effect| {
        let facts = assets::effect_descriptor_by_identifier(effect.effect);
        let mut text = facts
            .and_then(|facts| runtime.translation(facts.name_key))
            .map_or_else(|| effect.effect.to_owned(), |name| name.to_string());
        if effect.amplifier > 0 {
            let potency = runtime
                .translation(&format!("potion.potency.{}", effect.amplifier))
                .map_or_else(
                    || (u16::from(effect.amplifier) + 1).to_string(),
                    |text| text.to_string(),
                );
            text.push(' ');
            text.push_str(&potency);
        }
        if let Some(ticks) = effect.duration_ticks {
            text.push_str(&format!(
                " ({})",
                duration_text(ticks / form.duration_divisor())
            ));
        }
        let harmful = facts.is_some_and(|facts| facts.harmful);
        TooltipLine {
            text,
            color: if harmful {
                HARMFUL_EFFECT_COLOR
            } else {
                BENEFICIAL_EFFECT_COLOR
            },
        }
    })
}

fn level_text(runtime: &UiRuntime, level: u8) -> String {
    runtime
        .translation(&format!("enchantment.level.{level}"))
        .map_or_else(|| level.to_string(), |text| text.to_string())
}

/// An enchantment's localized name and level numeral (`Sharpness II`).
pub(super) fn enchantment_name(runtime: &UiRuntime, id: i16, level: u8) -> String {
    let label = inventory::enchantments::enchantment(id)
        .map(|enchantment| enchantment.lang_key)
        .and_then(|key| runtime.translation(key))
        .map_or_else(|| format!("Enchantment {id}"), |text| text.to_string());
    format!("{label} {}", level_text(runtime, level))
}

/// The localized default name, preferring a component name over a potion's type.
pub(crate) fn default_name(runtime: &UiRuntime, identifier: &str, metadata: u32) -> String {
    if runtime
        .item_components(identifier)
        .is_some_and(|components| components.display_name.is_some())
    {
        return runtime.localized_item_name(identifier);
    }
    if identifier == assets::facts::ARROW_IDENTIFIER
        && let Some(name) = assets::potion::tipped_arrow_name_key(metadata)
            .and_then(|key| runtime.translation(&key))
    {
        return name.to_string();
    }
    potion(Some(identifier), metadata)
        .and_then(|(descriptor, form)| {
            runtime.translation(&format!(
                "potion.{}{}",
                descriptor.name_key,
                form.name_suffix()
            ))
        })
        .map_or_else(
            || runtime.localized_item_name(identifier),
            |name| name.to_string(),
        )
}

/// The stack's name for both the selected-item HUD and inventory tooltip.
/// Response corrections override retained `display.Name`, which overrides the
/// localized item identity. A present, empty NBT name remains a custom name.
pub(super) fn name_line(
    runtime: &UiRuntime,
    identifier: Option<&str>,
    metadata: u32,
    stated_name: Option<&str>,
    display: &protocol::ItemDisplay,
) -> Option<TooltipLine> {
    let custom_name = stated_name.or(display.name.as_deref());
    let mut name = custom_name
        .map(str::to_owned)
        .or_else(|| identifier.map(|id| default_name(runtime, id, metadata)))?;
    let formatting = identifier.and_then(|id| runtime.item_name_color(id));
    let color = formatting
        .and_then(ui::BedrockColor::rgb)
        .map_or(NAME_COLOR, |[r, g, b]| [r, g, b, 255]);
    if let Some(descriptor) = formatting.and_then(ui::BedrockColor::descriptor) {
        // Keep the item's already-resolved native format code, rather than
        // reverse-mapping a duplicated component RGB palette in the UI bridge.
        name.insert(0, descriptor.code);
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
    let facts = runtime.stack_facts(stack);
    let display = &facts.display;
    let mut lines = vec![
        name_line(runtime, identifier, stack.metadata, stated_name, display).unwrap_or_else(|| {
            TooltipLine {
                text: "Unknown Item".to_owned(),
                color: NAME_COLOR,
            }
        }),
    ];
    if let Some((descriptor, form)) = potion(identifier, stack.metadata) {
        lines.extend(potion_effect_lines(runtime, descriptor, &form));
    }
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
