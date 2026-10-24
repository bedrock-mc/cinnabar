//! Registration validation for custom actors and their pack resources.
use crate::{
    host::cinnabar::experience_server::types as w, limits::*, manifest::Manifest, protocol as p,
};
use anyhow::{Result, ensure};
use std::{collections::HashSet, path::Path};

/// Checks finite coordinates without imposing a vanilla content range.
pub(crate) fn finite(v: p::Position) -> bool {
    v.x.is_finite() && v.y.is_finite() && v.z.is_finite()
}

/// Checks names, seats, properties and indexed resources before any server registration.
pub(crate) fn validate(
    dir: &Path,
    manifest: &Manifest,
    definitions: Vec<w::EntityDefinition>,
) -> Result<Vec<p::EntityDefinition>> {
    ensure!(
        definitions.len() <= MAX_ENTITY_TYPES,
        "too many entity types"
    );
    let root = std::path::absolute(dir)?;
    let mut names = HashSet::new();
    let mut out = Vec::new();
    for d in definitions {
        ensure!(
            super::worlds::owned_name(&d.id.value, &manifest.id)
                && names.insert(d.id.value.clone()),
            "invalid or duplicate entity type"
        );
        let collision = p::EntityBox {
            min: d.collision.min.into(),
            max: d.collision.max.into(),
        };
        ensure!(
            finite(collision.min)
                && finite(collision.max)
                && collision.min.x < collision.max.x
                && collision.min.y < collision.max.y
                && collision.min.z < collision.max.z,
            "invalid entity collision box"
        );
        ensure!(
            d.seats.len() <= MAX_ENTITY_SEATS && d.seats.iter().filter(|s| s.steers).count() <= 1,
            "invalid entity seats"
        );
        let mut seats = Vec::new();
        for s in d.seats {
            let position = s.position.into();
            ensure!(
                finite(position) && s.rotation_limit.is_none_or(|r| r.is_finite() && r >= 0.0),
                "invalid entity seat"
            );
            seats.push(p::EntitySeat {
                position,
                rotation_limit: s.rotation_limit,
                steers: s.steers,
            });
        }
        ensure!(
            d.properties.len() <= MAX_ENTITY_PROPERTIES,
            "too many entity properties"
        );
        let mut property_names = HashSet::new();
        let mut properties = Vec::new();
        for property in d.properties {
            ensure!(
                super::worlds::owned_name(&property.id.value, &manifest.id)
                    && property_names.insert(property.id.value.clone()),
                "invalid or duplicate entity property"
            );
            ensure!(
                property.min.is_finite()
                    && property.max.is_finite()
                    && property.initial.is_finite()
                    && property.min <= property.initial
                    && property.initial <= property.max,
                "invalid entity property range"
            );
            properties.push(p::EntityPropertyDef {
                id: property.id.value,
                min: property.min,
                max: property.max,
                initial: property.initial,
            });
        }
        let appearance = match d.appearance {
            w::EntityAppearance::Vanilla(id) => {
                ensure!(
                    id.value
                        .strip_prefix("minecraft:")
                        .is_some_and(super::is_name),
                    "invalid vanilla entity identity"
                );
                p::EntityAppearance::Vanilla { id: id.value }
            }
            w::EntityAppearance::Custom(art) => p::EntityAppearance::Custom {
                art: p::EntityArt {
                    geometry: super::asset_path(&root, &manifest.files, &art.geometry)?,
                    texture: super::asset_path(&root, &manifest.files, &art.texture)?,
                    render_controller: super::asset_path(
                        &root,
                        &manifest.files,
                        &art.render_controller,
                    )?,
                },
            },
        };
        out.push(p::EntityDefinition {
            id: d.id.value,
            collision,
            appearance,
            seats,
            properties,
        });
    }
    Ok(out)
}

/// Validates mutable actor state against its immutable registered definition.
pub(crate) fn valid_state(state: &p::EntityState, definition: &p::EntityDefinition) -> bool {
    finite(state.position)
        && finite(state.velocity)
        && state.rotation.yaw.is_finite()
        && state.rotation.pitch.is_finite()
        && state.scale.is_finite()
        && state.scale > 0.0
        && state.name_tag.len() <= MAX_NAME_BYTES
        && !state.name_tag.chars().any(char::is_control)
        && state.kind == definition.id
        && state.properties.len() == definition.properties.len()
        && state
            .properties
            .iter()
            .zip(&definition.properties)
            .all(|(value, property)| {
                value.id == property.id
                    && value.value.is_finite()
                    && (property.min..=property.max).contains(&value.value)
            })
}
