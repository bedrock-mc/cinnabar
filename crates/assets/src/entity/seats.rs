//! Compiled seat layouts retain authored selectors and rotation expressions.

use super::{CompiledEntityAssets, EntityAssetKind, RuntimeEntityAssets, invalid};
use crate::{AssetError, ItemDisplayScalar};
use serde::{Deserialize, Serialize};

pub const MAX_SEAT_LAYOUTS: usize = 128;
pub const MAX_LAYOUT_SEATS: usize = 64;

/// A group has no runtime activation admission until its actual component state is available.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum SeatSelector {
    Base,
    UnresolvedComponentGroup { group: u32 },
}

/// Rotation is independent from offset; an unevaluated expression is never treated as zero.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub enum SeatRotation {
    #[default]
    Absent,
    Literal(ItemDisplayScalar),
    Expression {
        expression: u32,
    },
    Unresolved,
}

/// One authored rider-count interval and its mount-local position.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CompiledRideSeat {
    pub position: [ItemDisplayScalar; 3],
    pub min_riders: u32,
    pub max_riders: u32,
    pub rotation: SeatRotation,
    pub lock_degrees: Option<ItemDisplayScalar>,
}

/// An authored layout is selected by state, never by a component-group name.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CompiledSeatLayout {
    pub selector: SeatSelector,
    pub seats: Box<[CompiledRideSeat]>,
}

/// Missing behavior definitions remain unresolved rather than manufacturing an empty layout.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub enum EntitySeatAdmission {
    #[default]
    Unresolved,
    /// Static authored facts; dynamic component activation and rotation evaluation remain open.
    Provisional {
        source: u32,
        layouts: Box<[CompiledSeatLayout]>,
    },
}

/// One independently collected resource entity and its explicit seat admission.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EntitySeatDescriptor {
    pub identifier: Box<str>,
    pub admission: EntitySeatAdmission,
}

/// Emits every independently collected identity even when its behavior source is unavailable.
pub fn unresolved_inventory(symbols: &[super::EntityAssetSymbol]) -> Box<[EntitySeatDescriptor]> {
    symbols
        .iter()
        .filter(|symbol| symbol.kind == EntityAssetKind::Entity)
        .map(|symbol| symbol.identifier.clone())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .map(|identifier| EntitySeatDescriptor {
            identifier,
            admission: EntitySeatAdmission::Unresolved,
        })
        .collect()
}

/// Checks coverage, source ownership and expression bounds before runtime admission.
pub(super) fn validate(compiled: &CompiledEntityAssets) -> Result<(), AssetError> {
    let inventory = unresolved_inventory(&compiled.symbols);
    if compiled.seat_layouts.len() != inventory.len() {
        return Err(invalid(
            "seat inventory coverage mismatch; rebuild with make assets",
        ));
    }
    for (entry, expected) in compiled.seat_layouts.iter().zip(inventory.iter()) {
        if entry.identifier != expected.identifier {
            return Err(invalid("seat identity coverage mismatch"));
        }
        let EntitySeatAdmission::Provisional { source, layouts } = &entry.admission else {
            continue;
        };
        if compiled.sources.get(*source as usize).is_none_or(|source| {
            !source.path.starts_with("behavior/entities/") || !source.path.ends_with(".json")
        }) || layouts.len() > MAX_SEAT_LAYOUTS
        {
            return Err(invalid("invalid compiled seat source or layout count"));
        }
        let mut base = false;
        let mut previous_group = None;
        for layout in layouts {
            match layout.selector {
                SeatSelector::Base if !base && previous_group.is_none() => base = true,
                SeatSelector::UnresolvedComponentGroup { group }
                    if previous_group.is_none_or(|previous| previous < group) =>
                {
                    previous_group = Some(group)
                }
                _ => return Err(invalid("duplicate or unordered seat selector")),
            }
            if layout.seats.len() > MAX_LAYOUT_SEATS {
                return Err(invalid("seat count exceeds bound"));
            }
            for seat in &layout.seats {
                let canonical =
                    |value: ItemDisplayScalar| ItemDisplayScalar::new(value.get()) == Some(value);
                if seat.min_riders > seat.max_riders
                    || !seat.position.into_iter().all(canonical)
                    || seat
                        .lock_degrees
                        .is_some_and(|value| !canonical(value) || value.get() < 0.0)
                    || match seat.rotation {
                        SeatRotation::Literal(value) => !canonical(value),
                        SeatRotation::Expression { expression } => {
                            expression as usize >= compiled.molang_expressions.len()
                        }
                        SeatRotation::Absent | SeatRotation::Unresolved => false,
                    }
                {
                    return Err(invalid("invalid compiled seat facts"));
                }
            }
        }
    }
    Ok(())
}

impl CompiledEntityAssets {
    /// Gives original unsupported fixtures explicit independently collected seat entries.
    pub fn with_unresolved_seats(mut self) -> Self {
        self.seat_layouts = unresolved_inventory(&self.symbols);
        self
    }
}

impl RuntimeEntityAssets {
    /// Borrows compiled layouts when copying a carrier without changing its seat admissions.
    pub fn seat_layouts(&self) -> &[EntitySeatDescriptor] {
        &self.seat_layouts
    }

    /// Resolves a numerical seat descriptor once at entity admission.
    pub fn seat_descriptor_index(&self, identifier: &str) -> Option<u32> {
        let identifier = identifier.split('<').next().unwrap_or(identifier);
        let identifier = if identifier.contains(':') {
            std::borrow::Cow::Borrowed(identifier)
        } else {
            std::borrow::Cow::Owned(format!("minecraft:{identifier}"))
        };
        self.seat_layouts
            .binary_search_by(|entry| entry.identifier.as_ref().cmp(identifier.as_ref()))
            .ok()
            .and_then(|index| u32::try_from(index).ok())
    }

    /// Reads an admitted descriptor by number without resolving names.
    pub fn seat_descriptor(&self, index: u32) -> Option<&EntitySeatAdmission> {
        self.seat_layouts
            .get(index as usize)
            .map(|entry| &entry.admission)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Retains a bounded original behavior source and one literal authored layout.
    fn fixture() -> CompiledEntityAssets {
        let mut compiled = crate::carriers::startup::test_carriers::synthetic_entity_assets(1);
        compiled.sources = Box::from([
            crate::EntityAssetSource {
                path: "behavior/entities/fixture.json".into(),
                source_bytes: 1,
                source_sha256: [2; 32],
            },
            compiled.sources[0].clone(),
        ]);
        compiled.symbols[0].source_index = 1;
        compiled.seat_layouts[0].admission = EntitySeatAdmission::Provisional {
            source: 0,
            layouts: Box::from([CompiledSeatLayout {
                selector: SeatSelector::Base,
                seats: Box::from([CompiledRideSeat {
                    position: [0.0, 1.0, 0.5].map(|value| ItemDisplayScalar::new(value).unwrap()),
                    min_riders: 1,
                    max_riders: 2,
                    rotation: SeatRotation::Literal(ItemDisplayScalar::new(-90.0).unwrap()),
                    lock_degrees: None,
                }]),
            }]),
        };
        compiled
    }

    #[test]
    fn seat_carrier_roundtrip_preserves_authored_layout_and_numeric_binding() {
        let compiled = fixture();
        let runtime =
            RuntimeEntityAssets::decode(&crate::encode_entity_blob(&compiled).unwrap()).unwrap();
        assert_eq!(runtime.seat_descriptor_index("allay"), Some(0));
        assert_eq!(
            runtime.seat_descriptor_index("minecraft:allay<fixture>"),
            Some(0)
        );
        assert_eq!(
            runtime.seat_descriptor(0),
            Some(&compiled.seat_layouts[0].admission)
        );
        assert_eq!(runtime.seat_descriptor_index("fixture:unknown"), None);
    }

    #[test]
    fn seat_coverage_cannot_omit_or_replace_an_independently_collected_entity() {
        let mut compiled = fixture();
        compiled.seat_layouts = Box::new([]);
        assert!(crate::encode_entity_blob(&compiled).is_err());
        compiled.seat_layouts = fixture().seat_layouts;
        compiled.seat_layouts[0].identifier = "fixture:forged".into();
        assert!(crate::encode_entity_blob(&compiled).is_err());
        let mut compiled = crate::carriers::startup::test_carriers::synthetic_entity_assets(1);
        compiled.seat_layouts = Box::new([]);
        assert!(crate::encode_entity_blob(&compiled).is_err());
    }

    #[test]
    fn seat_carrier_rejects_wrong_source_and_unbound_rotation_expression() {
        let mut compiled = fixture();
        let EntitySeatAdmission::Provisional { source, .. } =
            &mut compiled.seat_layouts[0].admission
        else {
            panic!("original authored fixture");
        };
        *source = 1;
        assert!(crate::encode_entity_blob(&compiled).is_err());
        let EntitySeatAdmission::Provisional { source, layouts } =
            &mut compiled.seat_layouts[0].admission
        else {
            panic!("original authored fixture");
        };
        *source = 0;
        layouts[0].seats[0].rotation = SeatRotation::Expression { expression: 0 };
        assert!(crate::encode_entity_blob(&compiled).is_err());
    }
}
