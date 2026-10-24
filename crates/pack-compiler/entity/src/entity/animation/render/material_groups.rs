use std::collections::{BTreeMap, BTreeSet};

use assets::{
    AssetError, EntityGeometry, EntityRenderCandidate, EntityRenderGeometry, EntityRenderLayer,
    EntityRenderMaterial, EntityRenderMaterialState, EntityRenderSlot, EntityRenderVisibility,
    MAX_ENTITY_RENDER_CANDIDATES, MAX_ENTITY_RENDER_LAYERS, MAX_ENTITY_RENDER_PATTERN_BYTES,
    MAX_ENTITY_RENDER_SLOTS, MAX_ENTITY_RENDER_VISIBILITY, entity_render_pattern_matches,
    validate_entity_geometry_inheritance,
};
use serde_json::{Map, Value};

use super::{
    super::selection::{Selector, selection_condition},
    materials::MaterialStates,
};
use crate::entity::{invalid, molang::MolangCompiler};

#[derive(Clone, Copy, Default, PartialEq)]
struct Raster {
    material: EntityRenderMaterial,
    state: Option<EntityRenderMaterialState>,
}

pub(super) struct Group {
    raster: Raster,
    visibility: Vec<(Box<str>, String)>,
}

fn raster(target: Option<&str>, materials: &MaterialStates) -> Raster {
    Raster {
        material: target.map_or(EntityRenderMaterial::Default, |name| {
            assets::facts::pack::rules().raster(name)
        }),
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
    let resolve_alias = |alias: &str| aliases.get(alias).map(|name| raster(Some(name), materials));
    let selector = Selector {
        prefix: "material.",
        arrays: controller
            .get("arrays")
            .and_then(|arrays| arrays.get("materials"))
            .and_then(Value::as_object)
            .into_iter()
            .flatten()
            .map(|(name, members)| {
                (
                    name.to_ascii_lowercase(),
                    members
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|member| member.as_str().map(str::to_owned))
                        .collect(),
                )
            })
            .collect(),
        resolve: &resolve_alias,
    };
    let mut validator = MolangCompiler::default();
    let rules: Vec<_> = controller
        .get("materials")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_object)
        .flatten()
        .filter_map(|(pattern, value)| {
            let choices = selector
                .leaves(value.as_str()?)?
                .into_iter()
                .map(|(path, raster)| {
                    let condition = selection_condition(&path);
                    if let Some(text) = &condition {
                        validator.compile(text).ok()?;
                    }
                    Some(Choice { raster, condition })
                })
                .collect::<Option<Vec<_>>>()?;
            (!choices.is_empty()).then(|| (pattern.to_ascii_lowercase(), choices))
        })
        .collect();
    let default = vec![Choice {
        raster: Raster::default(),
        condition: None,
    }];
    let fallback = rules
        .iter()
        .rev()
        .find(|(pattern, _)| pattern == "*")
        .map_or(&default, |(_, choices)| choices);
    if let [choice] = fallback.as_slice()
        && choice.condition.is_none()
        && rules.iter().all(|(_, choices)| choices == fallback)
    {
        return Ok(vec![Group {
            raster: choice.raster,
            visibility: Vec::new(),
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
            raster: fallback[0].raster,
            visibility: Vec::new(),
        }]);
    }
    let assignments: Vec<_> = names
        .into_iter()
        .map(|name| {
            let choices = rules
                .iter()
                .rev()
                .find(|(pattern, _)| entity_render_pattern_matches(pattern, &name))
                .map_or(fallback, |(_, choices)| choices);
            (name, choices)
        })
        .collect();
    let authored_visibility: Vec<_> = controller
        .get("part_visibility")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_object)
        .flatten()
        .filter_map(|(pattern, value)| {
            let text = super::expression_text(value)?;
            validator.compile(&text).ok()?;
            Some((pattern.to_ascii_lowercase(), text))
        })
        .collect();
    let mut seen = BTreeSet::new();
    let order: Vec<_> = rules
        .iter()
        .flat_map(|(_, choices)| choices)
        .chain(fallback)
        .map(|choice| choice.raster)
        .filter(|raster| {
            assignments
                .iter()
                .any(|(_, choices)| choices.iter().any(|choice| choice.raster == *raster))
        })
        .filter(|raster| seen.insert(raster.material.word(raster.state)))
        .collect();
    if order.len() > MAX_ENTITY_RENDER_LAYERS {
        return Err(invalid(
            "entity material groups exceed admitted render bounds",
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
            visibility: assignments
                .iter()
                .filter_map(|(name, choices)| {
                    let condition = material_condition(choices, raster)?;
                    let authored = authored_visibility
                        .iter()
                        .rev()
                        .find(|(pattern, _)| entity_render_pattern_matches(pattern, name));
                    let condition = match authored {
                        Some((_, authored)) if condition != "0" => {
                            format!("({authored}) && ({condition})")
                        }
                        _ => condition,
                    };
                    Some((name.as_str().into(), condition))
                })
                .collect(),
        })
        .collect())
}

/// A raster leaf and the selector condition that chooses it.
#[derive(Clone, PartialEq)]
struct Choice {
    raster: Raster,
    condition: Option<String>,
}

/// Returns a bone's visibility in one raster group; an unconditional owner needs no mask.
fn material_condition(choices: &[Choice], raster: Raster) -> Option<String> {
    let mut conditions = Vec::new();
    for choice in choices.iter().filter(|choice| choice.raster == raster) {
        let Some(condition) = &choice.condition else {
            return None;
        };
        if !conditions.contains(condition) {
            conditions.push(condition.clone());
        }
    }
    Some(if conditions.is_empty() {
        "0".into()
    } else {
        conditions
            .into_iter()
            .map(|condition| format!("({condition})"))
            .collect::<Vec<_>>()
            .join(" || ")
    })
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
    let added_visibility = groups
        .iter()
        .map(|group| group.visibility.len())
        .sum::<usize>()
        + extra * usize::from(base.visibility_count);
    if layers.len() + groups.len() > MAX_ENTITY_RENDER_LAYERS
        || slots.len() + extra * usize::from(base.slot_count) > MAX_ENTITY_RENDER_SLOTS
        || candidates.len() + extra * candidate_count > MAX_ENTITY_RENDER_CANDIDATES
        || geometries.len() + extra * usize::from(base.geometry_count)
            > MAX_ENTITY_RENDER_CANDIDATES
        || visibility.len() + added_visibility > MAX_ENTITY_RENDER_VISIBILITY
        || groups.iter().any(|group| {
            usize::from(base.visibility_count) + group.visibility.len() > usize::from(u16::MAX)
        })
    {
        return Err(invalid(
            "entity material groups exceed admitted render bounds",
        ));
    }
    if groups.len() == 1 && groups[0].visibility.is_empty() {
        layers.push(EntityRenderLayer {
            material: groups[0].raster.material,
            material_state: groups[0].raster.state,
            ..base
        });
        return Ok(());
    }
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
        for (pattern, condition) in group.visibility {
            visibility.push(EntityRenderVisibility {
                pattern,
                condition: molang.compile(&condition)?,
            });
        }
        layer.visibility_count = (visibility.len() - layer.first_visibility as usize) as u16;
        layer.material = group.raster.material;
        layer.material_state = group.raster.state;
        layers.push(layer);
    }
    Ok(())
}

#[cfg(test)]
#[path = "material_groups_tests.rs"]
mod tests;
