use serde::{Deserialize, Serialize};

use crate::AssetError;

use super::super::{CompiledEntityAssets, invalid};
use super::{range_in_bounds, validate_flattened_ranges};

pub const MAX_ENTITY_RENDER_LAYERS: usize = 16_384;
pub const MAX_ENTITY_RENDER_SLOTS: usize = 65_536;
pub const MAX_ENTITY_RENDER_CANDIDATES: usize = 262_144;
pub const MAX_ENTITY_RENDER_VISIBILITY: usize = 262_144;
pub const MAX_ENTITY_RENDER_PATTERN_BYTES: usize = 64;

/// Supported entity shader and depth contracts selected by authored materials.
#[derive(
    Clone, Copy, Debug, Default, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(rename_all = "snake_case")]
#[repr(u32)]
pub enum EntityRenderMaterial {
    #[default]
    Default,
    Dragon,
    DissolveDepth,
    DissolveColor,
}

/// One render controller of a rig: the expressions that pick its textures, hidden parts and
/// colours each tick. Layers of a rig are contiguous and ordered by `rig`.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EntityRenderLayer {
    #[serde(default, skip_serializing_if = "is_zero")]
    pub material: EntityRenderMaterial,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hurt_color: Option<[u32; 4]>,
    /// Index into the rig bindings.
    pub rig: u32,
    /// The entity's activation expression for this controller; absent means always.
    pub condition: Option<u32>,
    pub first_slot: u32,
    pub slot_count: u16,
    pub first_visibility: u32,
    pub visibility_count: u16,
    /// `[r, g, b, a]` expressions multiplying the texture.
    pub color: Option<[u32; 4]>,
    /// `[r, g, b, a]` expressions blended over the texture.
    pub overlay_color: Option<[u32; 4]>,
    pub on_fire_color: Option<[u32; 4]>,
    /// `uv_anim` `[offset u, offset v, scale u, scale v]` expressions; absent is identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uv_anim: Option<[u32; 4]>,
    /// The controller's `geometry` choices; none draws with the rig's own geometry.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub first_geometry: u32,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub geometry_count: u16,
    /// Draws unlit, ignoring world light.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub ignore_lighting: bool,
}

fn is_zero<T: Default + PartialEq>(value: &T) -> bool {
    *value == T::default()
}

/// One leaf of a controller's `geometry` expression: the first whose condition holds is drawn.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EntityRenderGeometry {
    pub condition: Option<u32>,
    /// Index into the catalog geometries.
    pub geometry: u32,
}

/// One authored `textures` entry: the first candidate whose condition holds is drawn.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EntityRenderSlot {
    pub first_candidate: u32,
    pub candidate_count: u16,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EntityRenderCandidate {
    /// Absent means the candidate applies whenever the slot is reached.
    pub condition: Option<u32>,
    /// Index into the catalog sources of the raster this candidate draws.
    pub source: u32,
}

/// A `part_visibility` rule: bones matching `pattern` (lowercase, optional trailing `*`) draw
/// only while `condition` holds; later rules override earlier ones.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EntityRenderVisibility {
    pub pattern: Box<str>,
    pub condition: u32,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EntityRenderData {
    pub layers: Box<[EntityRenderLayer]>,
    pub slots: Box<[EntityRenderSlot]>,
    pub candidates: Box<[EntityRenderCandidate]>,
    pub visibility: Box<[EntityRenderVisibility]>,
    #[serde(default, skip_serializing_if = "<[_]>::is_empty")]
    pub geometries: Box<[EntityRenderGeometry]>,
}

pub(super) fn validate_render_payload(compiled: &CompiledEntityAssets) -> Result<(), AssetError> {
    let render = &compiled.render;
    if render.layers.len() > MAX_ENTITY_RENDER_LAYERS
        || render.slots.len() > MAX_ENTITY_RENDER_SLOTS
        || render.candidates.len() > MAX_ENTITY_RENDER_CANDIDATES
        || render.geometries.len() > MAX_ENTITY_RENDER_CANDIDATES
        || render.visibility.len() > MAX_ENTITY_RENDER_VISIBILITY
    {
        return Err(invalid("entity render payload count exceeds bound"));
    }
    let expressions = compiled.molang_expressions.len();
    let expression = |index: u32| (index as usize) < expressions;
    let colors_valid = |color: &Option<[u32; 4]>| {
        color
            .as_ref()
            .is_none_or(|components| components.iter().all(|index| expression(*index)))
    };
    let mut previous_rig = 0;
    for layer in render.layers.iter() {
        if layer.rig < previous_rig
            || layer.rig as usize >= compiled.rig_bindings.len()
            || layer.condition.is_some_and(|index| !expression(index))
            || !range_in_bounds(
                layer.first_slot,
                u32::from(layer.slot_count),
                render.slots.len(),
            )
            || !range_in_bounds(
                layer.first_visibility,
                u32::from(layer.visibility_count),
                render.visibility.len(),
            )
            || !colors_valid(&layer.color)
            || !colors_valid(&layer.overlay_color)
            || !colors_valid(&layer.on_fire_color)
            || !colors_valid(&layer.hurt_color)
            || !colors_valid(&layer.uv_anim)
            || !range_in_bounds(
                layer.first_geometry,
                u32::from(layer.geometry_count),
                render.geometries.len(),
            )
        {
            return Err(invalid("entity render layer is invalid"));
        }
        previous_rig = layer.rig;
    }
    for slot in render.slots.iter() {
        if slot.candidate_count == 0
            || !range_in_bounds(
                slot.first_candidate,
                u32::from(slot.candidate_count),
                render.candidates.len(),
            )
        {
            return Err(invalid("entity render slot is invalid"));
        }
    }
    for candidate in render.candidates.iter() {
        let raster = compiled
            .sources
            .get(candidate.source as usize)
            .is_some_and(|source| {
                source.path.starts_with("textures/")
                    && (source.path.ends_with(".png") || source.path.ends_with(".tga"))
            });
        if !raster || candidate.condition.is_some_and(|index| !expression(index)) {
            return Err(invalid("entity render texture candidate is invalid"));
        }
    }
    for choice in render.geometries.iter() {
        if choice.geometry as usize >= compiled.geometries.len()
            || choice.condition.is_some_and(|index| !expression(index))
        {
            return Err(invalid("entity render geometry choice is invalid"));
        }
    }
    for rule in render.visibility.iter() {
        if rule.pattern.is_empty()
            || rule.pattern.len() > MAX_ENTITY_RENDER_PATTERN_BYTES
            || rule.pattern.chars().any(|ch| ch.is_ascii_uppercase())
            || {
                let rest = rule.pattern.strip_prefix('*').unwrap_or(&rule.pattern);
                rest.strip_suffix('*').unwrap_or(rest).contains('*')
            }
            || !expression(rule.condition)
        {
            return Err(invalid("entity render visibility rule is invalid"));
        }
    }
    validate_flattened_ranges(
        render
            .layers
            .iter()
            .map(|layer| (layer.first_slot, u32::from(layer.slot_count))),
        render.slots.len(),
        "render slot",
    )?;
    validate_flattened_ranges(
        render
            .layers
            .iter()
            .map(|layer| (layer.first_visibility, u32::from(layer.visibility_count))),
        render.visibility.len(),
        "render visibility",
    )?;
    validate_flattened_ranges(
        render
            .layers
            .iter()
            .map(|layer| (layer.first_geometry, u32::from(layer.geometry_count))),
        render.geometries.len(),
        "render geometry",
    )?;
    validate_flattened_ranges(
        render
            .slots
            .iter()
            .map(|slot| (slot.first_candidate, u32::from(slot.candidate_count))),
        render.candidates.len(),
        "render candidate",
    )
}
