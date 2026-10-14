//! Controller-consumed resources for attachables without explicit default aliases.

use std::collections::BTreeMap;

use assets::{EntityAssetKind, EntityAssetSource, EntityAssetSymbol, EntityGeometry};
use serde_json::Value;

use super::{AttachableError, bounded_identifier};
use crate::entity::animation::AnimationPayload;

/// Representative controller resources retained for the equipment catalog's static fallback.
#[derive(Default)]
pub(in crate::entity) struct ControllerResources {
    pub(super) geometry: Option<Box<str>>,
    pub(super) texture: Option<Box<str>>,
}

/// Records each attachable source's first consumed geometry and primary-slot image in authored
/// controller order. Runtime controller evaluation still selects the resources actually drawn.
pub(in crate::entity) fn controller_resources(
    animation: &AnimationPayload,
    symbols: &[EntityAssetSymbol],
    sources: &[EntityAssetSource],
    geometries: &[EntityGeometry],
) -> BTreeMap<Box<str>, ControllerResources> {
    let mut resources = BTreeMap::<Box<str>, ControllerResources>::new();
    for layer in &animation.render.layers {
        let Some(rig) = animation.rig_bindings.get(layer.rig as usize) else {
            continue;
        };
        let Some(symbol) = symbols.get(rig.entity_symbol as usize) else {
            continue;
        };
        if symbol.kind != EntityAssetKind::Attachable {
            continue;
        }
        let Some(source) = sources.get(symbol.source_index as usize) else {
            continue;
        };
        let entry = resources.entry(source.path.clone()).or_default();
        if entry.geometry.is_none() && layer.geometry_count > 0 {
            entry.geometry = animation
                .render
                .geometries
                .get(layer.first_geometry as usize)
                .and_then(|choice| geometries.get(choice.geometry as usize))
                .map(|geometry| geometry.identifier.clone());
        }
        if entry.texture.is_none() && layer.slot_count > 0 {
            entry.texture = animation
                .render
                .slots
                .get(layer.first_slot as usize)
                .filter(|slot| slot.candidate_count > 0)
                .and_then(|slot| {
                    animation
                        .render
                        .candidates
                        .get(slot.first_candidate as usize)
                })
                .and_then(|candidate| sources.get(candidate.source as usize))
                .and_then(|source| {
                    source
                        .path
                        .strip_suffix(".png")
                        .or_else(|| source.path.strip_suffix(".tga"))
                })
                .map(Box::from);
        }
    }
    resources
}

/// Keeps an explicit default alias, otherwise uses a resource retained by a compiled controller.
pub(super) fn named_resource(
    description: &Value,
    field: &str,
    fallback: Option<&str>,
) -> Result<Box<str>, AttachableError> {
    let identifier = match description.get(field).and_then(|map| map.get("default")) {
        Some(value) => value.as_str(),
        None => fallback,
    }
    .ok_or(AttachableError::Malformed)?;
    bounded_identifier(identifier)
}

/// Retains every declared resource so native draws can select stages without running scripts.
pub(super) fn aliases(
    description: &Value,
) -> Result<assets::equipment::EquipmentResources, AttachableError> {
    let table = |field: &str| -> Result<BTreeMap<Box<str>, Box<str>>, AttachableError> {
        description
            .get(field)
            .and_then(Value::as_object)
            .into_iter()
            .flatten()
            .map(|(alias, value)| {
                Ok((
                    bounded_identifier(alias)?,
                    bounded_identifier(value.as_str().ok_or(AttachableError::Malformed)?)?,
                ))
            })
            .collect()
    };
    Ok(assets::equipment::EquipmentResources {
        geometry: table("geometry")?,
        textures: table("textures")?,
    })
}

#[cfg(test)]
mod tests {
    use super::named_resource;

    #[test]
    fn malformed_explicit_defaults_do_not_use_controller_resources() {
        for value in [
            serde_json::json!(42),
            serde_json::json!(""),
            serde_json::json!("x".repeat(assets::MAX_EQUIPMENT_IDENTIFIER_BYTES + 1)),
        ] {
            let description = serde_json::json!({"geometry": {"default": value}});
            assert!(named_resource(&description, "geometry", Some("geometry.retained")).is_err());
        }
    }
}
