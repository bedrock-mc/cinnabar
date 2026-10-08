use std::{path::Path, sync::Arc};

use bevy::log::warn;
use client_world::{RideSeat, SeatDefaults, SeatRequirement};
use serde_json::Value;

/// Seat layouts of every rideable entity in the local behavior pack, loaded once; absent or
/// unreadable data degrades to no defaults, so riders keep their streamed pose.
pub(super) fn seat_defaults() -> Arc<SeatDefaults> {
    static DEFAULTS: std::sync::OnceLock<Arc<SeatDefaults>> = std::sync::OnceLock::new();
    Arc::clone(DEFAULTS.get_or_init(|| {
        let defaults = launcher::install_layout::InstallLayout::discover()
            .ok()
            .and_then(|layout| {
                let entities =
                    assets::vanilla_source().installed_pack_dir("behavior_pack/entities");
                load(&layout.resource_root.join(entities))
            });
        if defaults.is_none() {
            warn!(
                "behavior pack entities not found; riders without a streamed seat keep their pose"
            );
        }
        Arc::new(defaults.unwrap_or_default())
    }))
}

fn load(directory: &Path) -> Option<SeatDefaults> {
    let mut defaults = SeatDefaults::default();
    for entry in std::fs::read_dir(directory).ok()?.flatten() {
        let Ok(bytes) = std::fs::read(entry.path()) else {
            continue;
        };
        let Some(json) = resource_pack::normalize_jsonc(&bytes) else {
            continue;
        };
        let Ok(document) = serde_json::from_slice::<Value>(&json) else {
            continue;
        };
        if let Some((identifier, layouts)) = rideable_layouts(&document) {
            for (requirements, seats) in layouts {
                defaults.insert(identifier.as_str(), requirements, seats);
            }
        }
    }
    Some(defaults)
}

type Layout = (Vec<SeatRequirement>, Vec<RideSeat>);

/// Entity identifier and every `minecraft:rideable` layout: the base components' with no
/// requirements, then each component group's with the state its name spells out.
fn rideable_layouts(document: &Value) -> Option<(String, Vec<Layout>)> {
    let entity = document.get("minecraft:entity")?;
    let identifier = entity
        .pointer("/description/identifier")?
        .as_str()?
        .to_owned();
    let seats_of = |components: &Value| {
        components
            .get("minecraft:rideable")
            .and_then(|rideable| rideable.get("seats"))
            .map(parse_seats)
    };
    let mut layouts: Vec<Layout> = Vec::new();
    layouts.extend(
        entity
            .get("components")
            .and_then(seats_of)
            .map(|seats| (Vec::new(), seats)),
    );
    if let Some(groups) = entity.get("component_groups").and_then(Value::as_object) {
        for (name, group) in groups {
            if let Some(seats) = seats_of(group) {
                layouts.push((SeatRequirement::from_group_name(name), seats));
            }
        }
    }
    (!layouts.is_empty()).then_some((identifier, layouts))
}

/// `seats` is one object or a list of them.
fn parse_seats(seats: &Value) -> Vec<RideSeat> {
    let one = |seat: &Value| {
        let position = seat.get("position")?.as_array()?;
        let axis = |index: usize| position.get(index)?.as_f64().map(|value| value as f32);
        Some(RideSeat {
            position: [axis(0)?, axis(1)?, axis(2)?],
            min_riders: seat
                .get("min_rider_count")
                .and_then(Value::as_u64)
                .map_or(0, |count| count as u32),
            max_riders: seat
                .get("max_rider_count")
                .and_then(Value::as_u64)
                .map_or(u32::MAX, |count| count as u32),
            // Molang expressions (family-dependent angles) have no number to carry.
            rotate_by: seat
                .get("rotate_rider_by")
                .and_then(Value::as_f64)
                .map(|degrees| degrees as f32),
            lock_degrees: seat
                .get("lock_rider_rotation")
                .and_then(Value::as_f64)
                .map(|degrees| degrees as f32),
        })
    };
    match seats {
        Value::Array(list) => list.iter().filter_map(one).collect(),
        single => one(single).into_iter().collect(),
    }
}

#[cfg(test)]
mod tests {
    use client_world::SeatRequirement;

    use super::rideable_layouts;

    #[test]
    fn layouts_keep_group_state_and_rotation_fields() {
        let pig = serde_json::json!({"minecraft:entity": {
            "description": {"identifier": "minecraft:pig"},
            "component_groups": {
                "minecraft:pig_saddled": {"minecraft:rideable": {"seats": {"position": [0, 0.7, 0]}}},
                "minecraft:pig_unsaddled": {"minecraft:rideable": {"seats": {"position": [0, 0.5, 0]}}}
            }
        }});
        let (name, layouts) = rideable_layouts(&pig).unwrap();
        assert_eq!(name, "minecraft:pig");
        let requirements: Vec<_> = layouts.iter().map(|(need, _)| need.clone()).collect();
        assert!(requirements.contains(&vec![SeatRequirement::Saddled(true)]));
        assert!(requirements.contains(&vec![SeatRequirement::Saddled(false)]));
        let boat = serde_json::json!({"minecraft:entity": {
            "description": {"identifier": "minecraft:boat"},
            "components": {"minecraft:rideable": {"seats": [
                {"position": [0, -0.2, 0], "min_rider_count": 0, "max_rider_count": 1,
                 "rotate_rider_by": -90, "lock_rider_rotation": 90},
                {"position": [0.2, -0.2, 0], "min_rider_count": 2, "max_rider_count": 2,
                 "rotate_rider_by": "query.has_any_family('a') ? -90 : 90"}
            ]}}
        }});
        let seats = &rideable_layouts(&boat).unwrap().1[0].1;
        assert_eq!(seats.len(), 2);
        assert_eq!(
            (seats[0].rotate_by, seats[0].lock_degrees),
            (Some(-90.0), Some(90.0))
        );
        assert_eq!(seats[1].rotate_by, None);
    }
}
