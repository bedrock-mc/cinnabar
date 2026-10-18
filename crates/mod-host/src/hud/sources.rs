//! Host-owned row binding uses admitted session facts without crossing the component boundary.

use super::super::State;
use crate::{PlayerStateSnapshot, SidebarSnapshot};
use ui::mod_hud::{Hud, NativeRows, Row, RowSource};

const _: () = assert!(ui::mod_hud::MAX_NATIVE_ACTOR_LABELS == mod_api::MAX_OBSERVED_ACTORS);

/// Rejects a native read binding when its independent observation grant is absent.
pub(super) fn admit(state: &State, hud: &Hud) -> Result<(), String> {
    if hud
        .actor_style
        .as_ref()
        .is_some_and(|style| style.totems.is_some())
        && (!state.grants.player_state || !state.grants.players)
    {
        return Err("totem annotations require player-state and players capabilities".into());
    }
    if let Some(bounds) = &hud.actor_bounds
        && (!state.grants.render
            || bounds.players && !state.grants.players
            || bounds.mobs && !state.grants.entities)
    {
        return Err(
            "native actor bounds require render and the selected actor capabilities".into(),
        );
    }
    if !state.grants.player_state
        && hud.cards.iter().any(|card| {
            card.source
                .as_ref()
                .is_some_and(|s| !matches!(s.kind, NativeRows::Clock { .. }))
                || card.compass.is_some()
        })
    {
        return Err("native HUD sources require player-state capability".into());
    }
    if let Some(health) = &hud.actor_health
        && (health.players && !state.grants.players || health.mobs && !state.grants.entities)
    {
        return Err("native actor health requires the selected actor capabilities".into());
    }
    Ok(())
}

/// Refreshes only changed native facts or a newly committed source policy.
pub(in super::super) fn refresh(state: &mut State) {
    let clock = state.hud.content.as_ref().and_then(super::clock::observe);
    refresh_with_clock(state, clock);
}

/// Resolves current native facts with an explicit local minute for deterministic callers.
pub(super) fn refresh_with_clock(state: &mut State, clock: Option<super::clock::LocalMinute>) {
    let bound = state.hud.content.as_ref().is_some_and(|hud| {
        hud.actor_health.is_some()
            || hud
                .cards
                .iter()
                .any(|card| card.source.is_some() || card.compass.is_some())
    });
    if !bound {
        state.hud.source_snapshot = None;
        state.hud.source_sidebar = None;
        state.hud.source_actors.clear();
        state.hud.source_world = None;
        state.hud.source_motion = None;
        state.hud.source_clock = None;
        state.hud.source_totem_uses = None;
        state.hud.resolved = None;
        return;
    }
    let content = state.hud.content.as_ref().expect("bound content");
    let mut needs_snapshot = false;
    let mut needs_sidebar = false;
    let mut needs_totems = false;
    for card in &content.cards {
        match card.source.as_ref().map(|source| &source.kind) {
            Some(NativeRows::Clock { .. }) | None => {}
            Some(NativeRows::TotemUses { .. }) => needs_totems = true,
            Some(NativeRows::Sidebar { .. }) => needs_sidebar = true,
            Some(_) => needs_snapshot = true,
        }
    }
    let snapshot = if needs_snapshot && state.grants.player_state {
        state.player_state.snapshot.as_ref()
    } else {
        None
    };
    let sidebar = if needs_sidebar && state.grants.player_state {
        state.player_state.sidebar.as_ref()
    } else {
        None
    };
    let empty_actors = Vec::new();
    let actors = if content.actor_health.is_some() {
        &state.player_state.observations.actors
    } else {
        &empty_actors
    };
    let has_compass = content.cards.iter().any(|card| card.compass.is_some());
    let world = if has_compass || content.actor_health.is_some() {
        state.player_state.world.as_ref()
    } else {
        None
    };
    let motion = if has_compass && state.grants.player_state {
        state.player_state.observations.motion
    } else {
        None
    };
    let totem_uses = (needs_totems && state.grants.player_state)
        .then_some(state.player_state.observations.local_totem_uses)
        .flatten();
    let facts_changed = state.hud.sources_dirty
        || state.hud.source_totem_uses != totem_uses
        || state.hud.source_snapshot.as_ref() != snapshot
        || state.hud.source_sidebar.as_ref() != sidebar
        || state.hud.source_actors != *actors
        || state.hud.source_world.as_ref() != world;
    if !facts_changed && state.hud.source_motion == motion && state.hud.source_clock == clock {
        return;
    }
    let mut resolved = if facts_changed || state.hud.source_clock != clock {
        let mut hud = state.hud.content.clone().expect("bound content");
        super::clock::hydrate(&mut hud, clock);
        hydrate_totems(&mut hud, totem_uses);
        hydrate(&mut hud, snapshot, sidebar);
        hydrate_health(&mut hud, actors, world);
        hud
    } else {
        state
            .hud
            .resolved
            .clone()
            .expect("unchanged source facts are resolved")
    };
    super::compass::hydrate(&mut resolved, world, motion.as_ref());
    state.hud.resolved = Some(resolved);
    if facts_changed {
        state.hud.source_snapshot = snapshot.cloned();
        state.hud.source_sidebar = sidebar.cloned();
        state.hud.source_actors.clone_from(actors);
        state.hud.source_world = world.cloned();
    }
    state.hud.source_motion = motion;
    state.hud.source_clock = clock;
    state.hud.source_totem_uses = totem_uses;
    state.hud.sources_dirty = false;
}

/// Projects only admitted current-session actors, leaving authored labels in their own slots.
pub(super) fn hydrate_health(
    hud: &mut Hud,
    actors: &[crate::ObservedActor],
    world: Option<&crate::WorldObservation>,
) {
    let Some(style) = hud.actor_health.take() else {
        return;
    };
    let Some(world) = world else { return };
    for observed in actors.iter().take(mod_api::MAX_OBSERVED_ACTORS) {
        let actor = &observed.actor;
        if actor.session != world.session
            || actor.dimension != world.dimension
            || actor.invisible
            || if actor.player {
                !style.players
            } else {
                !style.mobs
            }
        {
            continue;
        }
        let Some(health) = actor.health else { continue };
        let [x, y, z] = [actor.position.0, actor.position.1, actor.position.2];
        let distance = (x - world.position.0).powi(2)
            + (y - world.position.1).powi(2)
            + (z - world.position.2).powi(2);
        if distance > style.range.powi(2) {
            continue;
        }
        let top = actor
            .boxes
            .iter()
            .map(|b| b.max.1)
            .max_by(f32::total_cmp)
            .unwrap_or(y + 1.);
        hud.world_labels.push(ui::mod_hud::WorldLabel {
            position: [x, top + 0.3, z],
            text: format!("{health:.0} HP"),
            color: style.color,
            ..Default::default()
        });
    }
}

/// Materializes bounded native rows, clearing unavailable or old-session observations.
pub(super) fn hydrate(
    hud: &mut Hud,
    snapshot: Option<&PlayerStateSnapshot>,
    sidebar: Option<&SidebarSnapshot>,
) {
    bind_rows(hud, snapshot, sidebar, false);
}

/// Keeps an empty bound widget named and selectable while editing its layout.
pub(super) fn hydrate_preview(
    hud: &mut Hud,
    snapshot: Option<&PlayerStateSnapshot>,
    sidebar: Option<&SidebarSnapshot>,
) {
    bind_rows(hud, snapshot, sidebar, true);
}

/// Resolves source policies without retaining them on materialized output.
fn bind_rows(
    hud: &mut Hud,
    snapshot: Option<&PlayerStateSnapshot>,
    sidebar: Option<&SidebarSnapshot>,
    preview: bool,
) {
    for card in &mut hud.cards {
        let Some(source) = &card.source else { continue };
        let mut rows = Vec::new();
        match &source.kind {
            NativeRows::Clock { .. } => {
                continue;
            }
            NativeRows::TotemUses { .. } => {
                continue;
            }
            NativeRows::ItemCount {
                label,
                items,
                blocks,
                potion_effect,
            } => {
                let value = snapshot.map_or_else(
                    || "--".into(),
                    |snapshot| {
                        snapshot
                            .inventory
                            .iter()
                            .chain(std::iter::once(&snapshot.offhand))
                            .filter(|slot| slot.known)
                            .filter_map(|slot| slot.item.as_ref())
                            .filter(|item| {
                                *blocks && item.block
                                    || item
                                        .identifier
                                        .as_ref()
                                        .is_some_and(|id| items.contains(id))
                                    || potion_effect
                                        .as_ref()
                                        .is_some_and(|effect| has_potion_effect(item, effect))
                            })
                            .map(|item| u32::from(item.count))
                            .sum::<u32>()
                            .to_string()
                    },
                );
                rows.push(row(source, label, value));
            }
            NativeRows::Inventory { hotbar } => {
                if let Some(snapshot) = snapshot {
                    for slot in snapshot.inventory.iter().skip(if *hotbar { 0 } else { 9 }) {
                        let mut row = row(
                            source,
                            "",
                            if slot.known {
                                String::new()
                            } else {
                                "?".into()
                            },
                        );
                        if let Some(item) = slot.item.as_ref().filter(|_| slot.known) {
                            row.item = item.identifier.clone().filter(|id| valid_item_id(id));
                            row.metadata = item.metadata;
                            row.value = if item.count > 1 {
                                item.count.to_string()
                            } else {
                                String::new()
                            };
                        }
                        rows.push(row);
                    }
                }
            }
            NativeRows::Armor {
                held,
                percent,
                empty,
            } => {
                if let Some(snapshot) = snapshot {
                    let armor =
                        snapshot
                            .armor
                            .iter()
                            .zip(["Helmet", "Chestplate", "Leggings", "Boots"]);
                    let selected = held
                        .then(|| {
                            snapshot
                                .selected_slot
                                .and_then(|i| snapshot.inventory.get(usize::from(i)))
                        })
                        .flatten();
                    for (slot, label) in armor.chain(selected.map(|slot| (slot, "Held"))) {
                        if let Some(item) = slot.item.as_ref().filter(|_| slot.known) {
                            let durability = item
                                .max_durability
                                .zip(item.damage)
                                .filter(|(max, _)| *max != 0)
                                .map(|(max, damage)| (max.saturating_sub(damage), max));
                            let value = durability.map_or_else(
                                || item.count.to_string(),
                                |(left, max)| {
                                    if *percent {
                                        format!("{:.0}%", left as f32 / max as f32 * 100.)
                                    } else {
                                        format!("{left}/{max}")
                                    }
                                },
                            );
                            let mut row = row(source, label, value);
                            row.item = item.identifier.clone().filter(|id| valid_item_id(id));
                            row.metadata = item.metadata;
                            row.progress = durability.map(|(left, max)| left as f32 / max as f32);
                            rows.push(row);
                        } else if *empty {
                            rows.push(row(
                                source,
                                label,
                                if slot.known { "Empty" } else { "--" }.into(),
                            ));
                        }
                    }
                }
            }
            NativeRows::Effects { icons } => {
                if let Some(snapshot) = snapshot {
                    for effect in &snapshot.effects {
                        let value = effect.remaining_ticks.map_or_else(
                            || "∞".into(),
                            |ticks| format!("{}:{:02}", ticks / 1200, ticks / 20 % 60),
                        );
                        let mut row = row(
                            source,
                            &format!(
                                "Effect {} ({})",
                                effect.effect_id,
                                effect.amplifier.saturating_add(1)
                            ),
                            value,
                        );
                        row.effect_id = (*icons && (1..=255).contains(&effect.effect_id))
                            .then_some(effect.effect_id);
                        if let Some(effect_type) = assets::effect_descriptor(effect.effect_id) {
                            row.label = effect_type.name_key.into();
                            row.localized = true;
                            row.value =
                                format!("{} {}", effect.amplifier.saturating_add(1), row.value);
                        }
                        rows.push(row);
                    }
                }
            }
            NativeRows::Sidebar { scores } => {
                if let Some(sidebar) = sidebar {
                    let mut title = row(source, "", sidebar.title.clone());
                    title.formatted = true;
                    rows.push(title);
                    for entry in &sidebar.rows {
                        let mut row = row(
                            source,
                            &entry.label,
                            if *scores {
                                entry.score.to_string()
                            } else {
                                String::new()
                            },
                        );
                        row.formatted = true;
                        rows.push(row);
                    }
                }
            }
        }
        rows.truncate(usize::from(source.limit));
        if card.rows != rows {
            card.rows = rows;
        }
        if preview && card.rows.is_empty() && card.title.is_empty() {
            card.title.clone_from(if card.editor_label.is_empty() {
                &card.id
            } else {
                &card.editor_label
            });
        }
        card.source = None;
    }
}

/// Materializes the local counter without exposing or polling native activation history from WASM.
pub(super) fn hydrate_totems(hud: &mut Hud, count: Option<u32>) {
    for card in &mut hud.cards {
        let Some(source) = &card.source else { continue };
        let NativeRows::TotemUses { flip, count_color } = source.kind else {
            continue;
        };
        let style = ui::mod_hud::TotemCounterStyle {
            flip,
            count_color,
            ..Default::default()
        };
        let mut value = row(
            source,
            "",
            count.map_or_else(|| "--".into(), |count| style.count_text(count)),
        );
        if let Some(count) = count
            && count_color
        {
            value.color = style.count_rgba(count).map(|v| f32::from(v) / 255.);
        }
        card.rows = vec![value];
        card.source = None;
    }
}

/// Matches auxiliary values through the admitted item form and native potion descriptor facts.
fn has_potion_effect(item: &crate::PlayerStateItem, effect: &str) -> bool {
    let Some(identifier) = item.identifier.as_deref() else {
        return false;
    };
    if !assets::facts::ItemFacts::builtin().is_ok_and(|facts| {
        facts
            .by_identifier(identifier)
            .is_some_and(|fact| fact.potion_form.is_some())
    }) {
        return false;
    }
    i16::try_from(item.metadata)
        .ok()
        .and_then(assets::potion::potion_descriptor)
        .is_some_and(|potion| potion.effects.iter().any(|entry| entry.effect == effect))
}

/// Builds text within the same byte and color contract as guest-authored rows.
pub(super) fn row(source: &RowSource, label: &str, value: String) -> Row {
    Row {
        label: clip(label),
        value: clip(&if source.brackets && !value.is_empty() {
            format!("[ {value} ]")
        } else {
            value
        }),
        item: None,
        effect_id: None,
        metadata: 0,
        progress: None,
        color: source.color,
        formatted: false,
        localized: false,
    }
}

/// Truncates on a UTF-8 boundary and removes controls before native UI admission.
fn clip(value: &str) -> String {
    let mut output = String::new();
    for ch in value.chars().filter(|ch| !ch.is_control()) {
        if output.len() + ch.len_utf8() > ui::mod_hud::MAX_TEXT_BYTES {
            break;
        }
        output.push(ch);
    }
    if output.ends_with('§') {
        output.pop();
    }
    output
}

/// Filters unresolved item identities through the HUD's existing atlas identifier contract.
fn valid_item_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"_:-./".contains(&b))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The native fact filter must stay exactly aligned with ordinary guest item admission.
    #[test]
    fn source_item_filter_matches_the_hud_identifier_contract() {
        let mut hud: Hud =
            serde_json::from_str(r#"{"cards":[{"id":"item","rows":[{"label":"","value":""}]}]}"#)
                .unwrap();
        let identifiers = (0..=127_u8)
            .map(|byte| format!("test:item{}", char::from(byte)))
            .chain([String::new(), "a".repeat(128), "a".repeat(129), "é".into()]);
        for id in identifiers {
            hud.cards[0].rows[0].item = Some(id.clone());
            assert_eq!(valid_item_id(&id), hud.validate().is_ok(), "item {id:?}");
        }
    }
}
