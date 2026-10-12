//! Behavior seats compile once, without guessing group activation from its spelling.

use super::*;
use assets::entity_seats::{
    CompiledRideSeat, CompiledSeatLayout, EntitySeatAdmission, EntitySeatDescriptor, SeatRotation,
    SeatSelector,
};

/// Compiles authored layouts and joins them to the independently collected resource inventory.
pub(super) fn compile(
    symbols: &[EntityAssetSymbol],
    sources: &[EntityAssetSource],
    payloads: &SourcePayloads,
    molang: &mut molang::MolangCompiler,
) -> Result<Box<[EntitySeatDescriptor]>, AssetError> {
    let mut definitions = BTreeMap::new();
    for (index, source) in sources
        .iter()
        .enumerate()
        .filter(|(_, source)| source.path.starts_with("behavior/entities/"))
    {
        let Some(bytes) = payloads.get(&source.path) else {
            continue;
        };
        let document = parse_unique_json(Path::new(source.path.as_ref()), bytes)?;
        let Some(entity) = document.get("minecraft:entity") else {
            continue;
        };
        let Some(identifier) = entity
            .pointer("/description/identifier")
            .and_then(Value::as_str)
        else {
            continue;
        };
        if definitions.contains_key(identifier) {
            return Err(invalid("duplicate behavior seat definition"));
        }
        let mark = molang.mark();
        let admission = match layouts(entity, molang) {
            Some(layouts) => EntitySeatAdmission::Provisional {
                source: index as u32,
                layouts,
            },
            None => {
                molang.rollback(mark);
                EntitySeatAdmission::Unresolved
            }
        };
        definitions.insert(identifier.to_owned(), admission);
    }
    Ok(assets::entity_seats::unresolved_inventory(symbols)
        .into_vec()
        .into_iter()
        .map(|entry| EntitySeatDescriptor {
            admission: definitions
                .remove(entry.identifier.as_ref())
                .unwrap_or_default(),
            ..entry
        })
        .collect())
}

/// Retains typed unresolved group selectors regardless of names or flag-like tokens.
fn layouts(
    entity: &Value,
    molang: &mut molang::MolangCompiler,
) -> Option<Box<[CompiledSeatLayout]>> {
    let mut layouts = Vec::new();
    if let Some(rideable) = entity.pointer("/components/minecraft:rideable") {
        layouts.push(CompiledSeatLayout {
            selector: SeatSelector::Base,
            seats: seats(rideable, molang)?,
        });
    }
    if let Some(groups) = entity.get("component_groups").and_then(Value::as_object) {
        for (index, (_, group)) in groups.iter().enumerate() {
            if let Some(rideable) = group.get("minecraft:rideable") {
                layouts.push(CompiledSeatLayout {
                    selector: SeatSelector::UnresolvedComponentGroup {
                        group: u32::try_from(index).ok()?,
                    },
                    seats: seats(rideable, molang)?,
                });
            }
        }
    }
    (layouts.len() <= assets::entity_seats::MAX_SEAT_LAYOUTS).then(|| layouts.into())
}

/// Rejects a malformed seat list as one unresolved admission rather than dropping individual seats.
fn seats(rideable: &Value, molang: &mut molang::MolangCompiler) -> Option<Box<[CompiledRideSeat]>> {
    let raw = rideable.get("seats")?;
    let values: Vec<_> = match raw {
        Value::Array(values) => values.iter().collect(),
        single => vec![single],
    };
    if values.len() > assets::entity_seats::MAX_LAYOUT_SEATS {
        return None;
    }
    values
        .into_iter()
        .map(|seat| {
            let raw_position = seat.get("position")?.as_array()?;
            if raw_position.len() != 3 {
                return None;
            }
            let scalar = |value: &Value| assets::ItemDisplayScalar::new(value.as_f64()? as f32);
            let count = |name, default| match seat.get(name) {
                Some(value) => u32::try_from(value.as_u64()?).ok(),
                None => Some(default),
            };
            let min_riders = count("min_rider_count", 0)?;
            let max_riders = count("max_rider_count", u32::MAX)?;
            if min_riders > max_riders {
                return None;
            }
            let rotation = match seat.get("rotate_rider_by") {
                None => SeatRotation::Absent,
                Some(value) if value.is_number() => SeatRotation::Literal(scalar(value)?),
                Some(Value::String(expression)) => molang
                    .compile(expression)
                    .ok()
                    .map_or(SeatRotation::Unresolved, |expression| {
                        SeatRotation::Expression { expression }
                    }),
                Some(_) => return None,
            };
            let lock_degrees = match seat.get("lock_rider_rotation") {
                Some(value) => Some(scalar(value)?),
                None => None,
            };
            if lock_degrees.is_some_and(|degrees| degrees.get() < 0.0) {
                return None;
            }
            Some(CompiledRideSeat {
                position: [
                    scalar(&raw_position[0])?,
                    scalar(&raw_position[1])?,
                    scalar(&raw_position[2])?,
                ],
                min_riders,
                max_riders,
                rotation,
                lock_degrees,
            })
        })
        .collect::<Option<Vec<_>>>()
        .map(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unadmitted_component_group_does_not_become_an_active_base_layout() {
        let entity = serde_json::json!({"component_groups": {
            "fixture:unknown_activation": {"minecraft:rideable": {"seats": {"position": [0, 1, 0]}}},
            "fixture:saddled": {"minecraft:rideable": {"seats": {"position": [0, 2, 0]}}}
        }});
        let mut molang = molang::MolangCompiler::default();
        let layouts = layouts(&entity, &mut molang).unwrap();
        assert_eq!(layouts.len(), 2);
        assert!(layouts.iter().all(|layout| matches!(
            layout.selector,
            SeatSelector::UnresolvedComponentGroup { .. }
        )));
    }

    #[test]
    fn seats_retain_expression_rotation_and_reject_count_overflow() {
        let entity = serde_json::json!({"components": {"minecraft:rideable": {"seats": [
            {"position": [0, 1, 0], "rotate_rider_by": "query.is_on_ground ? -90 : 90", "min_rider_count": 1, "max_rider_count": 2},
            {"position": [0, 2, 0], "rotate_rider_by": -90, "lock_rider_rotation": 90}
        ]}}});
        let mut compiler = molang::MolangCompiler::default();
        let layouts = layouts(&entity, &mut compiler).unwrap();
        assert!(matches!(
            layouts[0].seats[0].rotation,
            SeatRotation::Expression { .. }
        ));
        assert!(matches!(
            layouts[0].seats[1].rotation,
            SeatRotation::Literal(_)
        ));
        let program = compiler.finish().unwrap();
        assert!(!program.expressions.is_empty());
        let unknown = serde_json::json!({"seats": {"position": [0, 1, 0], "rotate_rider_by": "query.fixture_unknown()"}});
        let unknown = seats(&unknown, &mut molang::MolangCompiler::default()).unwrap();
        assert_eq!(unknown[0].rotation, SeatRotation::Unresolved);
        let invalid = serde_json::json!({"seats": {"position": [0, 1, 0], "min_rider_count": 4294967296_u64}});
        assert!(seats(&invalid, &mut molang::MolangCompiler::default()).is_none());
        let invalid = serde_json::json!({"seats": {"position": [0, 1, 0], "min_rider_count": 3, "max_rider_count": 2}});
        assert!(seats(&invalid, &mut molang::MolangCompiler::default()).is_none());
    }
}
