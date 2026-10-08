use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{AssetError, SkinGeometryBounds};

use super::{
    EntityDependencyResolution, EntityGeometryBone, EntityGeometryScalar, RuntimeEntityAssets,
    invalid, validate_scalars,
};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EntityGeometry {
    pub identifier: Box<str>,
    pub inherits: Option<EntityGeometryInheritance>,
    pub source_index: u32,
    pub texture_width: u16,
    pub texture_height: u16,
    pub bones: Box<[EntityGeometryBone]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visible_bounds: Option<EntityGeometryBounds>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EntityGeometryInheritance {
    pub identifier: Box<str>,
    pub resolution: EntityDependencyResolution,
}

/// Authored geometry visibility bounds in blocks, independent of the actor's collision box.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EntityGeometryBounds {
    pub center: [EntityGeometryScalar; 3],
    pub half_extents: [EntityGeometryScalar; 3],
}

impl EntityGeometryBounds {
    #[must_use]
    pub fn from_description(description: &Value) -> Option<Self> {
        let bounds = SkinGeometryBounds::from_description(description)?;
        let scalars = |values: [f32; 3]| {
            let [Some(x), Some(y), Some(z)] = values.map(EntityGeometryScalar::new) else {
                return None;
            };
            Some([x, y, z])
        };
        Some(Self {
            center: scalars(bounds.center)?,
            half_extents: scalars(bounds.half_extents)?,
        })
    }

    #[must_use]
    pub fn as_bounds(self) -> SkinGeometryBounds {
        SkinGeometryBounds {
            center: self.center.map(EntityGeometryScalar::get),
            half_extents: self.half_extents.map(EntityGeometryScalar::get),
        }
    }

    pub(super) fn validate(self) -> Result<(), AssetError> {
        validate_scalars(&self.center)?;
        validate_scalars(&self.half_extents)?;
        if !self.as_bounds().has_valid_dimensions() {
            return Err(invalid("invalid entity geometry visibility bounds"));
        }
        Ok(())
    }
}

impl RuntimeEntityAssets {
    /// Resolves the nearest authored visibility box in the admitted inheritance chain.
    #[must_use]
    pub fn geometry_visible_bounds(&self, geometry: usize) -> Option<SkinGeometryBounds> {
        let mut current = geometry;
        for _ in 0..=super::MAX_ENTITY_GEOMETRY_INHERITANCE_DEPTH {
            if let Some(bounds) = self.geometries.get(current)?.visible_bounds {
                return Some(bounds.as_bounds());
            }
            current = *self.geometry_parents.get(current)?.as_ref()?;
        }
        None
    }
}
