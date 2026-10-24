use std::collections::{BTreeMap, BTreeSet};

use assets::{
    AssetError, EntityGeometry, EntityRenderCandidate, EntityRenderGeometry, EntityRenderLayer,
    EntityRenderMaterial, EntityRenderMaterialState, EntityRenderSlot, EntityRenderVisibility,
    MAX_ENTITY_RENDER_CANDIDATES, MAX_ENTITY_RENDER_LAYERS, MAX_ENTITY_RENDER_PATTERN_BYTES,
    MAX_ENTITY_RENDER_SLOTS, MAX_ENTITY_RENDER_VISIBILITY, entity_render_pattern_matches,
    validate_entity_geometry_inheritance,
};
use serde_json::{Map, Value};

use super::{super::selection::Selection, materials::MaterialStates};
use crate::entity::{invalid, molang::MolangCompiler};

#[derive(Clone, Copy, Default, PartialEq)]
struct Raster {
    material: EntityRenderMaterial,
    state: Option<EntityRenderMaterialState>,
}

pub(super) struct Group {
    raster: Raster,
    hidden: Vec<Box<str>>,
    selection: Option<Selection<Raster>>,
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
    if let Some(selection) = uniform_selection(description, controller, materials) {
        return Ok(vec![Group {
            raster: Raster::default(),
            hidden: Vec::new(),
            selection: Some(selection),
        }]);
    }
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
            selection: None,
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
            selection: None,
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
            selection: None,
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
            selection: None,
            hidden: assignments
                .iter()
                .filter(|(_, assigned)| *assigned != raster)
                .map(|(name, _)| name.as_str().into())
                .collect(),
        })
        .collect())
}

/// Expands a whole-model material selector without rewriting authored part visibility.
fn uniform_selection(
    description: &Map<String, Value>,
    controller: &Map<String, Value>,
    materials: &MaterialStates,
) -> Option<Selection<Raster>> {
    use super::super::selection::Selector;
    let [rule] = controller.get("materials")?.as_array()?.as_slice() else {
        return None;
    };
    let rule = rule.as_object()?;
    if rule.len() != 1 {
        return None;
    }
    let expression = rule.get("*")?.as_str()?;
    let aliases = super::lowercase_map(description.get("materials"));
    let resolve_alias = |alias: &str| {
        aliases
            .get(alias)
            .map(|target| raster(Some(target), materials))
    };
    let selector = Selector {
        prefix: "material.",
        arrays: controller
            .get("arrays")
            .and_then(|arrays| arrays.get("materials"))
            .and_then(Value::as_object)
            .into_iter()
            .flatten()
            .map(|(name, values)| {
                (
                    name.to_ascii_lowercase(),
                    values
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|value| value.as_str().map(str::to_owned))
                        .collect(),
                )
            })
            .collect(),
        resolve: &resolve_alias,
    };
    match selector.tree(expression)? {
        selection @ (Selection::Branch(..) | Selection::Array(..)) => Some(selection),
        Selection::Leaf(_) | Selection::Empty => None,
    }
}

pub(super) struct Records<'a> {
    pub selections: &'a mut Vec<assets::EntityRenderMaterialSelection>,
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
        selections,
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
        let group = groups.into_iter().next().expect("one group");
        layers.push(EntityRenderLayer {
            material: group.raster.material,
            material_state: group.raster.state,
            material_selection: group
                .selection
                .and_then(|selection| retain_selection(selection, selections, molang)),
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
        layer.material_selection = group
            .selection
            .and_then(|selection| retain_selection(selection, selections, molang));
        layers.push(layer);
    }
    Ok(())
}

/// Drops an unsupported selection without leaving partial nodes or Molang programs behind.
fn retain_selection(
    selection: Selection<Raster>,
    nodes: &mut Vec<assets::EntityRenderMaterialSelection>,
    molang: &mut MolangCompiler,
) -> Option<u32> {
    let start = nodes.len();
    let mark = molang.mark();
    match compile_selection(selection, nodes, molang) {
        Ok(root) => Some(root),
        Err(_) => {
            nodes.truncate(start);
            molang.rollback(mark);
            None
        }
    }
}

/// Lowers one decision tree without cloning the authored controller or repeating its scripts.
fn compile_selection(
    selection: Selection<Raster>,
    nodes: &mut Vec<assets::EntityRenderMaterialSelection>,
    molang: &mut MolangCompiler,
) -> Result<u32, AssetError> {
    use assets::EntityRenderMaterialSelection as Node;
    let root = nodes.len();
    nodes.push(Node::Raster {
        material: EntityRenderMaterial::Default,
        state: None,
    });
    let node = match selection {
        Selection::Empty => Node::Raster {
            material: EntityRenderMaterial::Default,
            state: None,
        },
        Selection::Leaf(raster) => Node::Raster {
            material: raster.material,
            state: raster.state,
        },
        Selection::Branch(condition, yes, no) => Node::Branch {
            condition: molang.compile(&condition)?,
            yes: compile_selection(*yes, nodes, molang)?,
            no: compile_selection(*no, nodes, molang)?,
        },
        Selection::Array(index, choices) if !choices.is_empty() => Node::Array {
            index: molang.compile(&index)?,
            choices: choices
                .into_iter()
                .map(|choice| compile_selection(choice, nodes, molang))
                .collect::<Result<_, _>>()?,
        },
        Selection::Array(..) => return Err(invalid("empty material selector array")),
    };
    nodes[root] = node;
    Ok(root as u32)
}

#[cfg(test)]
#[path = "material_groups_tests.rs"]
mod tests;
