use std::collections::{BTreeMap, BTreeSet};

use assets::{
    AssetError, EntityGeometry, EntityRenderCandidate, EntityRenderGeometry, EntityRenderLayer,
    EntityRenderMaterial, EntityRenderMaterialState, EntityRenderSlot, EntityRenderVisibility,
    MAX_ENTITY_RENDER_CANDIDATES, MAX_ENTITY_RENDER_LAYERS, MAX_ENTITY_RENDER_PATTERN_BYTES,
    MAX_ENTITY_RENDER_SLOTS, MAX_ENTITY_RENDER_VISIBILITY, entity_render_pattern_matches,
    validate_entity_geometry_inheritance,
};
use serde_json::{Map, Value};

use super::materials::MaterialStates;
use crate::entity::{invalid, molang::MolangCompiler};

#[derive(Clone, Copy, Default, PartialEq)]
struct Raster {
    material: EntityRenderMaterial,
    state: Option<EntityRenderMaterialState>,
}

pub(super) struct Group {
    raster: Raster,
    hidden: Vec<Box<str>>,
}

fn raster(target: Option<&str>, materials: &MaterialStates) -> Raster {
    Raster {
        material: match target {
            Some("ender_dragon") => EntityRenderMaterial::Dragon,
            Some("entity_dissolve_layer0.skinning" | "entity_dissolve_layer0") => {
                EntityRenderMaterial::DissolveDepth
            }
            Some("entity_dissolve_layer1.skinning" | "entity_dissolve_layer1") => {
                EntityRenderMaterial::DissolveColor
            }
            _ => EntityRenderMaterial::Default,
        },
        state: target.and_then(|target| materials.resolve(target)),
    }
}

pub(super) fn resolve(
    description: &Map<String, Value>,
    controller: &Map<String, Value>,
    materials: &MaterialStates,
    geometries: &[EntityGeometry],
    selected: impl IntoIterator<Item = u32>,
    parents: &mut Option<Box<[Option<usize>]>>,
) -> Result<Vec<Group>, AssetError> {
    let aliases = super::lowercase_map(description.get("materials"));
    let rules: Vec<_> = controller
        .get("materials")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_object)
        .flatten()
        .filter_map(|(pattern, value)| {
            let alias = value.as_str()?.strip_prefix("Material.")?;
            let target = aliases.get(&alias.to_ascii_lowercase())?;
            Some((
                pattern.to_ascii_lowercase(),
                raster(Some(target), materials),
            ))
        })
        .collect();
    let fallback = rules
        .iter()
        .rev()
        .find(|(pattern, _)| pattern == "*")
        .map_or_else(Raster::default, |(_, raster)| *raster);
    if rules.iter().all(|(_, raster)| *raster == fallback) {
        return Ok(vec![Group {
            raster: fallback,
            hidden: Vec::new(),
        }]);
    }
    if parents.is_none() {
        *parents = Some(validate_entity_geometry_inheritance(geometries)?);
    }
    let parents = parents.as_deref().expect("resolved geometry parents");
    let mut names = BTreeSet::new();
    for selected in selected {
        let mut current = Some(selected as usize);
        while let Some(index) = current {
            let geometry = &geometries[index];
            names.extend(
                geometry
                    .bones
                    .iter()
                    .map(|bone| bone.name.to_ascii_lowercase()),
            );
            current = parents[index];
        }
    }
    if names.is_empty() {
        return Ok(vec![Group {
            raster: fallback,
            hidden: Vec::new(),
        }]);
    }
    let assignments: Vec<_> = names
        .into_iter()
        .map(|name| {
            let raster = rules
                .iter()
                .rev()
                .find(|(pattern, _)| entity_render_pattern_matches(pattern, &name))
                .map_or(fallback, |(_, raster)| *raster);
            (name, raster)
        })
        .collect();
    let mut order = Vec::new();
    let mut seen = BTreeSet::new();
    for raster in rules.iter().map(|(_, raster)| *raster).chain([fallback]) {
        if assignments.iter().any(|(_, assigned)| *assigned == raster)
            && seen.insert(raster.material.word(raster.state))
        {
            order.push(raster);
        }
    }
    if order.len() == 1 {
        return Ok(vec![Group {
            raster: order[0],
            hidden: Vec::new(),
        }]);
    }
    if order.len() > MAX_ENTITY_RENDER_LAYERS
        || assignments.len().saturating_mul(order.len() - 1) > MAX_ENTITY_RENDER_VISIBILITY
    {
        return Err(invalid(
            "entity material bone masks exceed admitted render bounds",
        ));
    }
    if assignments
        .iter()
        .any(|(name, _)| name.len() > MAX_ENTITY_RENDER_PATTERN_BYTES || name.contains('*'))
    {
        return Err(invalid(
            "entity material bone mask exceeds admitted pattern bounds",
        ));
    }
    Ok(order
        .into_iter()
        .map(|raster| Group {
            raster,
            hidden: assignments
                .iter()
                .filter(|(_, assigned)| *assigned != raster)
                .map(|(name, _)| name.as_str().into())
                .collect(),
        })
        .collect())
}

pub(super) struct Records<'a> {
    pub layers: &'a mut Vec<EntityRenderLayer>,
    pub slots: &'a mut Vec<EntityRenderSlot>,
    pub candidates: &'a mut Vec<EntityRenderCandidate>,
    pub visibility: &'a mut Vec<EntityRenderVisibility>,
    pub geometries: &'a mut Vec<EntityRenderGeometry>,
}

pub(super) fn append(
    base: EntityRenderLayer,
    groups: Vec<Group>,
    records: Records<'_>,
    molang: &mut MolangCompiler,
) -> Result<(), AssetError> {
    let Records {
        layers,
        slots,
        candidates,
        visibility,
        geometries,
    } = records;
    let extra = groups.len().saturating_sub(1);
    let candidate_count: usize = slots[base.first_slot as usize..]
        .iter()
        .map(|slot| usize::from(slot.candidate_count))
        .sum();
    let added_visibility = groups.iter().map(|group| group.hidden.len()).sum::<usize>()
        + extra * usize::from(base.visibility_count);
    if layers.len() + groups.len() > MAX_ENTITY_RENDER_LAYERS
        || slots.len() + extra * usize::from(base.slot_count) > MAX_ENTITY_RENDER_SLOTS
        || candidates.len() + extra * candidate_count > MAX_ENTITY_RENDER_CANDIDATES
        || geometries.len() + extra * usize::from(base.geometry_count)
            > MAX_ENTITY_RENDER_CANDIDATES
        || visibility.len() + added_visibility > MAX_ENTITY_RENDER_VISIBILITY
        || groups.iter().any(|group| {
            usize::from(base.visibility_count) + group.hidden.len() > usize::from(u16::MAX)
        })
    {
        return Err(invalid(
            "entity material groups exceed admitted render bounds",
        ));
    }
    if groups.len() == 1 {
        layers.push(EntityRenderLayer {
            material: groups[0].raster.material,
            material_state: groups[0].raster.state,
            ..base
        });
        return Ok(());
    }
    let hidden_condition = molang.compile("0.0")?;
    let authored_visibility = visibility[base.first_visibility as usize..].to_vec();
    let authored_slots = slots[base.first_slot as usize..].to_vec();
    let authored_candidates: BTreeMap<_, _> = authored_slots
        .iter()
        .map(|slot| {
            let first = slot.first_candidate as usize;
            (
                slot.first_candidate,
                candidates[first..first + usize::from(slot.candidate_count)].to_vec(),
            )
        })
        .collect();
    let authored_geometries = geometries[base.first_geometry as usize..].to_vec();
    for (index, group) in groups.into_iter().enumerate() {
        let mut layer = base;
        if index != 0 {
            layer.first_slot = slots.len() as u32;
            for slot in &authored_slots {
                let first_candidate = candidates.len() as u32;
                candidates.extend_from_slice(&authored_candidates[&slot.first_candidate]);
                slots.push(EntityRenderSlot {
                    first_candidate,
                    ..*slot
                });
            }
            layer.first_geometry = geometries.len() as u32;
            geometries.extend_from_slice(&authored_geometries);
            layer.first_visibility = visibility.len() as u32;
            visibility.extend_from_slice(&authored_visibility);
        }
        // Material ownership only hides parts; authored visibility can never be undone.
        visibility.extend(
            group
                .hidden
                .into_iter()
                .map(|pattern| EntityRenderVisibility {
                    pattern,
                    condition: hidden_condition,
                }),
        );
        layer.visibility_count = (visibility.len() - layer.first_visibility as usize) as u16;
        layer.material = group.raster.material;
        layer.material_state = group.raster.state;
        layers.push(layer);
    }
    Ok(())
}
