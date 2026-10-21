//! Native equipment and status-effect formatting, with legacy row policies preserved.

use crate::PlayerStateSnapshot;
use ui::mod_hud::{NativeRows, Row, RowSource};

pub(super) const WARNING_RED: [f32; 4] = [1., 0.35, 0.35, 1.];
pub(super) const WARNING_AMBER: [f32; 4] = [1., 0.75, 0.25, 1.];

/// Materializes only observed equipment, keeping compact values free of slot labels and bars.
pub(super) fn equipment(source: &RowSource, snapshot: &PlayerStateSnapshot) -> Vec<Row> {
    let NativeRows::Armor {
        held,
        percent,
        empty,
        compact,
        offhand,
        warning,
    } = source.kind
    else {
        return Vec::new();
    };
    let selected = held
        .then(|| {
            snapshot
                .selected_slot
                .and_then(|index| snapshot.inventory.get(usize::from(index)))
        })
        .flatten();
    let armor = snapshot
        .armor
        .iter()
        .zip(["Helmet", "Chestplate", "Leggings", "Boots"]);
    let held_row = selected.map(|slot| (slot, "Held"));
    let mut slots: Vec<_> = if compact {
        held_row.into_iter().chain(armor).collect()
    } else {
        armor.chain(held_row).collect()
    };
    if offhand {
        slots.push((&snapshot.offhand, "Offhand"));
    }
    slots
        .into_iter()
        .filter_map(|(slot, label)| {
            let label = if compact { "" } else { label };
            let Some(item) = slot
                .item
                .as_ref()
                .filter(|item| slot.known && item.count > 0)
            else {
                return empty.then(|| {
                    super::sources::row(
                        source,
                        label,
                        if slot.known { "Empty" } else { "--" }.into(),
                    )
                });
            };
            let durability = item
                .max_durability
                .zip(item.damage)
                .filter(|(max, _)| *max > 0)
                .map(|(max, damage)| (max.saturating_sub(damage), max));
            let value = match durability {
                Some((left, max)) if percent => format!("{:.0}%", left as f32 / max as f32 * 100.),
                Some((left, _)) if compact => left.to_string(),
                Some((left, max)) => format!("{left}/{max}"),
                None if compact && item.max_durability.is_some() => "?".into(),
                None if compact => String::new(),
                None => item.count.to_string(),
            };
            let mut row = super::sources::row(source, label, value);
            row.item = item
                .identifier
                .clone()
                .filter(|id| super::sources::valid_item_id(id));
            row.metadata = item.metadata;
            row.progress = (!compact)
                .then(|| durability.map(|(left, max)| left as f32 / max as f32))
                .flatten();
            if warning > 0.
                && durability.is_some_and(|(left, max)| left as f32 / max as f32 * 100. <= warning)
            {
                row.color = WARNING_RED;
            }
            Some(row)
        })
        .collect()
}

/// Stacked effects use localized names, Roman levels, ceiling-rounded timers and expiry ink.
pub(super) fn effects(source: &RowSource, snapshot: &PlayerStateSnapshot) -> Vec<Row> {
    let NativeRows::Effects {
        icons,
        stacked,
        warnings,
    } = source.kind
    else {
        return Vec::new();
    };
    let mut effects: Vec<_> = snapshot
        .effects
        .iter()
        .filter(|effect| !stacked || effect.remaining_ticks != Some(0))
        .collect();
    if stacked {
        effects.sort_by_key(|effect| effect.effect_id);
    }
    effects
        .into_iter()
        .map(|effect| {
            let value = effect.remaining_ticks.map_or_else(
                || "∞".into(),
                |ticks| {
                    if stacked {
                        let seconds = ticks / 20 + u64::from(ticks % 20 != 0);
                        format!("{:02}:{:02}", seconds / 60, seconds % 60)
                    } else {
                        format!("{}:{:02}", ticks / 1200, ticks / 20 % 60)
                    }
                },
            );
            let mut row = super::sources::row(
                source,
                &format!(
                    "Effect {} ({})",
                    effect.effect_id,
                    effect.amplifier.saturating_add(1)
                ),
                value,
            );
            row.effect_id =
                (icons && (1..=255).contains(&effect.effect_id)).then_some(effect.effect_id);
            let descriptor = assets::effect_descriptor(effect.effect_id);
            if stacked {
                let level = if effect.amplifier > 0 {
                    format!(" {}", roman(effect.amplifier.saturating_add(1) as u32))
                } else {
                    String::new()
                };
                row.label = descriptor.map_or_else(
                    || format!("Effect {}{level}", effect.effect_id),
                    |d| format!("%{}{level}", d.name_key),
                );
                row.localized = descriptor.is_some();
            } else if let Some(descriptor) = descriptor {
                row.label = descriptor.name_key.into();
                row.localized = true;
                row.value = format!("{} {}", effect.amplifier.saturating_add(1), row.value);
            }
            if warnings {
                row.color = match effect.remaining_ticks {
                    Some(1..=60) => WARNING_RED,
                    Some(61..=200) => WARNING_AMBER,
                    _ => source.color,
                };
            }
            row
        })
        .collect()
}

/// Bounds Roman notation to ordinary effect levels and uses decimal text for larger values.
fn roman(value: u32) -> String {
    if value > 3999 {
        return value.to_string();
    }
    let mut remaining = value;
    let mut text = String::new();
    for (unit, one, five, ten) in [
        (100, "C", "D", "M"),
        (10, "X", "L", "C"),
        (1, "I", "V", "X"),
    ] {
        if unit == 100 {
            text.push_str(&"M".repeat((remaining / 1000) as usize));
            remaining %= 1000;
        }
        let digit = remaining / unit;
        match digit {
            9 => {
                text.push_str(one);
                text.push_str(ten);
            }
            4 => {
                text.push_str(one);
                text.push_str(five);
            }
            _ => {
                if digit >= 5 {
                    text.push_str(five);
                }
                text.push_str(&one.repeat((digit % 5) as usize));
            }
        }
        remaining %= unit;
    }
    text
}
