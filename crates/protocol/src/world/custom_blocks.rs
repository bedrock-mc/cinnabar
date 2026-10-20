mod mining;
mod physical;
pub use mining::{CustomBlockMining, CustomMiningItem, CustomMiningSpeed};
mod placement;
mod shared_states;
use std::sync::Arc;

use bedrock_protocol_1_26_51::packets::StartGame;

use crate::nbt_tree::{Nbt, read_root};
#[cfg(test)]
use physical::box_component;
use physical::{
    collision_components, collision_enabled, friction_component, physical_components,
    random_offset_component, selection_component, transformation_component,
};

/// States one custom block may contribute before it is treated as malformed.
const MAX_STATES_PER_BLOCK: u64 = 1 << 16;

/// Permutations one custom block may define before extras are ignored.
const MAX_PERMUTATIONS: usize = 1024;
/// Bone visibility entries one geometry component may define before extras are ignored.
const MAX_BONE_VISIBILITY: usize = 256;
/// Material instances one component set may define before extras are ignored.
const MAX_MATERIAL_INSTANCES: usize = 64;
/// Collision primitives one custom block may contribute before extras are ignored.
const MAX_COLLISION_BOXES: usize = 256;
/// The namespace of vanilla's own blocks.
const VANILLA_NAMESPACE: &str = "minecraft";

/// One server-defined block from StartGame.
#[derive(Debug, Clone, PartialEq)]
pub struct CustomBlock {
    /// Mining values shared by every state unless a permutation replaces them.
    pub mining: Arc<CustomBlockMining>,
    pub name: Arc<str>,
    /// Server-declared block tags used by target priorities and exclusions.
    pub tags: Arc<[Arc<str>]>,
    /// Sequential palette states: the product of property and trait values.
    pub state_count: u32,
    /// False when the definition disables its collision box.
    pub collides: bool,
    /// Explicit `minecraft:collision_box` shapes; `None` means a full cube when `collides`.
    pub collision_boxes: Option<Arc<[CustomBox]>>,
    /// `minecraft:selection_box`: what the pick ray targets and the outline traces.
    pub selection: CustomSelection,
    /// `minecraft:friction` value; surface friction is one minus it, and `None` keeps the default.
    pub friction: Option<f32>,
    pub visual: Arc<CustomBlockVisuals>,
    /// `minecraft:custom_components` declares a player-interact handler, so using the block
    /// interacts with it rather than placing the held block against it.
    pub player_interact: bool,
    /// Resolved palette-order component sets; empty until session preparation.
    pub state_physics: Arc<[CustomBlockPhysics]>,
    /// Every physical permutation was parsed and evaluated for every state.
    pub state_physics_complete: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub enum CustomSelection {
    /// No component: use the block type's full-cube visual shape.
    #[default]
    Default,
    Box(CustomBox),
    /// Component disabled: the block cannot be targeted.
    Disabled,
}

/// An axis-aligned box in block units (X/Z `0..=1`, Y `0..=1.5`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CustomBox {
    pub min: [f32; 3],
    pub max: [f32; 3],
}

impl CustomBox {
    /// The default local bounds of an enabled box component.
    pub const FULL: Self = Self {
        min: [0.0; 3],
        max: [1.0; 3],
    };
}

/// The render-relevant parts of a custom block definition.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CustomBlockVisuals {
    /// Placement behavior retained alongside the shared state axes.
    pub placement: CustomPlacement,
    pub base: CustomVisualComponents,
    /// Authored inventory geometry and materials, independent of placed-state transforms.
    pub item_visual: Option<CustomVisualComponents>,
    /// In definition order; later matching permutations override earlier ones.
    pub permutations: Box<[CustomPermutation]>,
    /// Physical or placement data was discarded while parsing the definition.
    pub components_incomplete: bool,
    /// Discarded component data or permutations could change this block's mining value.
    pub mining_incomplete: bool,
    /// Block states in palette order: placement trait states, then properties in list order.
    /// The first axis varies fastest across the block's palette run.
    pub state_axes: Box<[CustomStateAxis]>,
    /// A declared state could not be represented by the named axes.
    pub state_identity_incomplete: bool,
    /// The palette run is retained for wire offsets, but its states must not be expanded.
    pub materialization_disabled: bool,
}

/// Authored placement traits and client prediction admission for a custom block.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct CustomPlacement {
    /// Leading state axes supplied by enabled placement traits rather than properties.
    pub trait_state_count: usize,
    /// Degrees subtracted from player yaw before choosing a direction state.
    pub y_rotation_offset: f32,
    /// A server-side placing handler owns the resulting block state.
    pub server_placing: bool,
    /// Placement filters require support checks before local prediction is admitted.
    pub has_placement_filter: bool,
}

/// Visual components present in one component set; `None` means absent.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CustomVisualComponents {
    /// Position-dependent component; an explicit zero value overrides inherited displacement.
    pub random_offset: Option<block_transform::random_offset::RandomOffsetComponent>,
    pub geometry: Option<Arc<str>>,
    /// Geometry's legacy `useBlockTypeLightAbsorption` flag; absent means false.
    pub geometry_use_block_type_light_absorption: bool,
    /// `bone_visibility` of the same geometry component: bone name and Molang expression.
    pub bone_visibility: Box<[(Arc<str>, Arc<str>)]>,
    pub materials: Option<Box<[CustomMaterialInstance]>>,
    pub transformation: Option<CustomTransformation>,
    /// `minecraft:light_dampening`, the sky/block light a full block filters (0..=15).
    pub light_dampening: Option<u8>,
    /// `minecraft:light_emission` (0..=15).
    pub light_emission: Option<u8>,
}

impl CustomVisualComponents {
    /// Resolves absorption when an explicit component is absent.
    pub fn effective_light_dampening(&self) -> u8 {
        self.light_dampening
            .unwrap_or_else(|| {
                if self.geometry.is_some() && !self.geometry_use_block_type_light_absorption {
                    0
                } else {
                    15
                }
            })
            .min(15)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CustomMaterialInstance {
    /// `*`, a face name, or a named instance a geometry face refers to.
    pub name: Arc<str>,
    pub texture: Arc<str>,
    pub render_method: Option<Arc<str>>,
    /// `tint_method`: `default_foliage`, `birch_foliage`, `evergreen_foliage`, `dry_foliage`, `grass`, `water`, or `none`.
    pub tint_method: Option<Arc<str>>,
    pub ambient_occlusion: Option<f32>,
    pub face_dimming: Option<bool>,
}

/// Rotation in quarter turns about each axis, then scale and translation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CustomTransformation {
    pub rotation: [i32; 3],
    pub scale: [f32; 3],
    pub translation: [f32; 3],
}

impl CustomTransformation {
    /// Transforms a point given in pixels from the block centre; geometry and boxes share it.
    #[must_use]
    pub fn apply(&self, mut point: [f32; 3]) -> [f32; 3] {
        for (value, scale) in point.iter_mut().zip(self.scale) {
            *value *= scale;
        }
        for (axis, turns) in self.rotation.into_iter().enumerate() {
            for _ in 0..turns.rem_euclid(4) {
                point = block_transform::rotation::rotate_quarter(point, axis);
            }
        }
        for (value, offset) in point.iter_mut().zip(self.translation) {
            *value += offset * 16.0;
        }
        point
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CustomPermutation {
    pub condition: Arc<str>,
    pub components: CustomVisualComponents,
    pub physical: CustomPhysicalComponents,
}

/// State components present in a permutation; absent fields inherit.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CustomPhysicalComponents {
    pub mining: Option<Arc<CustomBlockMining>>,
    pub random_offset: Option<block_transform::random_offset::RandomOffsetComponent>,
    pub collision: Option<CustomCollision>,
    pub selection: Option<CustomSelection>,
    /// Transforms the block's geometry, collision box and selection box together.
    pub transformation: Option<CustomTransformation>,
    pub friction: Option<f32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CustomCollision {
    pub enabled: bool,
    pub boxes: Option<Arc<[CustomBox]>>,
}

/// Effective physical components for one palette state.
#[derive(Debug, Clone, PartialEq)]
pub struct CustomBlockPhysics {
    /// `None` when a mining permutation could not be resolved for this state.
    pub mining: Option<Arc<CustomBlockMining>>,
    pub random_offset: Option<block_transform::random_offset::RandomOffsetComponent>,
    /// Transform this state's boxes inherit, about the block centre.
    pub transformation: Option<CustomTransformation>,
    pub collides: bool,
    pub collision_boxes: Option<Arc<[CustomBox]>>,
    pub selection: CustomSelection,
    /// Effective `minecraft:friction` value, as on [`CustomBlock::friction`].
    pub friction: Option<f32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustomStateAxis {
    pub name: Arc<str>,
    pub values: Box<[CustomStateValue]>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CustomStateValue {
    String(Arc<str>),
    Int(i64),
    Bool(bool),
}

/// One block state a hashed-id session can send: its network hash and its value
/// on each of the block's `state_axes`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustomHashedState {
    pub hash: u32,
    pub values: Arc<[CustomStateValue]>,
}

impl CustomBlock {
    /// Falls back to base shapes while keeping unprepared mining permutations unresolved.
    #[must_use]
    pub fn physics_for_state(&self, index: u32) -> CustomBlockPhysics {
        let mut physics = self
            .state_physics
            .get(index as usize)
            .cloned()
            .unwrap_or_else(|| {
                let mut physics = self.base_physics();
                physics.mining = self.mining_for_state(index).cloned();
                physics
            });
        if self.visual.mining_incomplete {
            physics.mining = None;
        }
        physics
    }

    /// Borrows resolved mining data; an unprepared mining permutation cannot use the base value.
    pub fn mining_for_state(&self, index: u32) -> Option<&Arc<CustomBlockMining>> {
        if self.visual.mining_incomplete {
            None
        } else if let Some(physics) = self.state_physics.get(index as usize) {
            physics.mining.as_ref()
        } else if self
            .visual
            .permutations
            .iter()
            .any(|permutation| permutation.physical.mining.is_some())
        {
            None
        } else {
            Some(&self.mining)
        }
    }

    #[must_use]
    pub fn base_physics(&self) -> CustomBlockPhysics {
        CustomBlockPhysics {
            mining: (!self.visual.mining_incomplete).then(|| Arc::clone(&self.mining)),
            random_offset: self.visual.base.random_offset,
            transformation: self.visual.base.transformation,
            collides: self.collides,
            collision_boxes: self.collision_boxes.clone(),
            selection: self.selection,
            friction: self.friction,
        }
    }

    /// Every state in palette order with its network block hash, for sessions whose block
    /// ids are hashes.
    #[must_use]
    pub fn hashed_states(&self) -> shared_states::SharedStates {
        shared_states::resolve(self)
    }

    /// Compiles the canonical palette identities for one immutable definition.
    fn compile_states(&self) -> Arc<[CustomHashedState]> {
        let axes = &self.visual.state_axes;
        if self.visual.state_identity_incomplete || self.visual.materialization_disabled {
            return Arc::default();
        }
        let Some(total) = axis_combinations(axes) else {
            return Arc::default();
        };
        (0..total)
            .filter_map(|index| {
                let values = decode_state(axes, index)?;
                Some(CustomHashedState {
                    hash: block_state_network_hash(
                        &self.name,
                        axes.iter()
                            .map(|axis| axis.name.as_ref())
                            .zip(values.iter()),
                    ),
                    values: values.into(),
                })
            })
            .collect()
    }

    /// The value on each of `state_axes` of the state at `index` in the block's palette run;
    /// `None` when the axes do not account for every state.
    #[must_use]
    pub fn state_values(&self, index: u32) -> Option<Arc<[CustomStateValue]>> {
        let axes = &self.visual.state_axes;
        if self.visual.state_identity_incomplete || self.visual.materialization_disabled {
            return None;
        }
        if axis_combinations(axes)? != u64::from(self.state_count) {
            return None;
        }
        self.hashed_states()
            .get(index as usize)
            .map(|state| Arc::clone(&state.values))
    }

    /// Vanilla orders the sequential block palette by FNV-1 64 of the name, then the name.
    #[must_use]
    pub fn sort_key(&self) -> u64 {
        block_name_sort_key(&self.name)
    }
}

/// The number of states the axes span, or `None` past the per-block bound or for an empty axis.
fn axis_combinations(axes: &[CustomStateAxis]) -> Option<u64> {
    axes.iter()
        .try_fold(1_u64, |total, axis| {
            (!axis.values.is_empty()).then(|| total.saturating_mul(axis.values.len() as u64))
        })
        .filter(|&total| total <= MAX_STATES_PER_BLOCK)
}

/// Mixed-radix decode of a palette index: the first axis varies fastest.
fn decode_state(axes: &[CustomStateAxis], mut index: u64) -> Option<Box<[CustomStateValue]>> {
    let values = axes
        .iter()
        .map(|axis| {
            let len = axis.values.len() as u64;
            let pick = (index % len) as usize;
            index /= len;
            axis.values[pick].clone()
        })
        .collect();
    (index == 0).then_some(values)
}

/// Returns vanilla's sequential palette sort key for a block name.
#[must_use]
pub fn block_name_sort_key(name: &str) -> u64 {
    name.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        hash.wrapping_mul(0x0000_0100_0000_01b3) ^ u64::from(byte)
    })
}

/// FNV-1a 32 of the little-endian NBT `{name, states}` with state keys sorted:
/// the id a hashed-palette server sends for a block state.
#[must_use]
pub fn block_state_network_hash<'a>(
    name: &str,
    states: impl IntoIterator<Item = (&'a str, &'a CustomStateValue)>,
) -> u32 {
    let mut states: Vec<_> = states.into_iter().collect();
    states.sort_by(|left, right| left.0.cmp(right.0));
    let mut data = vec![10, 0, 0];
    let push_string = |data: &mut Vec<u8>, text: &str| {
        data.extend_from_slice(&(text.len() as u16).to_le_bytes());
        data.extend_from_slice(text.as_bytes());
    };
    data.push(8);
    push_string(&mut data, "name");
    push_string(&mut data, name);
    data.push(10);
    push_string(&mut data, "states");
    for (key, value) in states {
        match value {
            CustomStateValue::String(text) => {
                data.push(8);
                push_string(&mut data, key);
                push_string(&mut data, text);
            }
            CustomStateValue::Bool(flag) => {
                data.push(1);
                push_string(&mut data, key);
                data.push(u8::from(*flag));
            }
            CustomStateValue::Int(number) => {
                data.push(3);
                push_string(&mut data, key);
                data.extend_from_slice(&(*number as i32).to_le_bytes());
            }
        }
    }
    data.extend_from_slice(&[0, 0]);
    data.iter().fold(0x811c_9dc5_u32, |hash, &byte| {
        (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193)
    })
}

/// StartGame custom blocks in sequential palette order; malformed definitions are skipped.
/// Budget-rejected runs retain their names and counts without materialized states.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CustomBlocks {
    /// Materialized definitions form a prefix; budget-rejected runs follow it.
    pub blocks: Arc<[CustomBlock]>,
    /// Vanilla data-driven types admitted by the server's definitions.
    pub vanilla_blocks: Arc<[Arc<str>]>,
    pub skipped: usize,
}

impl CustomBlocks {
    pub(crate) fn from_start_game(start_game: &StartGame) -> Self {
        let properties = &start_game.block_properties;
        let names: Vec<_> = properties
            .iter()
            .map(|property| property.block_name.to_string_lossy())
            .collect();
        Self::from_definitions(
            names
                .iter()
                .zip(properties)
                .map(|(name, property)| (&**name, &property.block_definition.0[..])),
        )
    }

    /// Parses `(name, network NBT)` block definitions as StartGame carries them.
    /// Retains advertised vanilla definitions separately from custom visual overlays.
    #[must_use]
    pub fn from_definitions<'a>(
        definitions: impl IntoIterator<Item = (&'a str, &'a [u8])>,
    ) -> Self {
        let mut blocks = Vec::new();
        let mut vanilla_blocks = Vec::new();
        let mut skipped = 0;
        for (name, bytes) in definitions {
            if name.is_empty() {
                skipped += 1;
                continue;
            }
            let Some(root) = read_root(bytes) else {
                skipped += 1;
                continue;
            };
            if name
                .split_once(':')
                .is_some_and(|(namespace, _)| namespace == VANILLA_NAMESPACE)
            {
                vanilla_blocks.push(Arc::from(name));
                continue;
            }
            match parse_definition(&root) {
                Some(definition) => {
                    skipped += definition.skipped;
                    blocks.push(CustomBlock {
                        mining: definition.mining,
                        name: Arc::from(name),
                        tags: definition.tags,
                        state_count: definition.state_count,
                        collides: definition.collides,
                        collision_boxes: definition.collision_boxes,
                        selection: definition.selection,
                        friction: definition.friction,
                        player_interact: definition.player_interact,
                        visual: Arc::new(definition.visual),
                        state_physics_complete: false,
                        state_physics: Arc::default(),
                    });
                }
                None => skipped += 1,
            }
        }
        blocks.sort_by(|left, right| {
            (left.sort_key(), &left.name).cmp(&(right.sort_key(), &right.name))
        });
        let mut state_budget = shared_states::AdmissionBudget::default();
        let mut exhausted = false;
        for block in &mut blocks {
            let admitted =
                !exhausted && state_budget.admit(&block.name, &block.visual, block.state_count);
            if !admitted {
                exhausted = true;
                skipped += 1;
                block.tags = Arc::default();
                block.collision_boxes = None;
                block.visual = Arc::new(CustomBlockVisuals {
                    state_identity_incomplete: true,
                    materialization_disabled: true,
                    ..Default::default()
                });
            }
        }
        for block in &blocks {
            let _ = block.hashed_states();
        }
        Self {
            blocks: blocks.into(),
            vanilla_blocks: vanilla_blocks.into(),
            skipped,
        }
    }

    #[must_use]
    pub fn total_states(&self) -> u32 {
        self.blocks.iter().fold(0_u32, |total, block| {
            total.saturating_add(block.state_count)
        })
    }
}

struct Definition {
    mining: Arc<CustomBlockMining>,
    tags: Arc<[Arc<str>]>,
    skipped: usize,
    state_count: u32,
    collides: bool,
    collision_boxes: Option<Arc<[CustomBox]>>,
    selection: CustomSelection,
    friction: Option<f32>,
    player_interact: bool,
    visual: CustomBlockVisuals,
}

/// Reads the wire tag list, counting unsupported entries without discarding the block.
fn block_tags(root: &Nbt) -> (Arc<[Arc<str>]>, usize) {
    const MAX_TAGS: usize = 256;
    const MAX_TAG_BYTES: usize = 256;
    let entries = match root.field("blockTags") {
        Some(Nbt::List(entries)) => entries.as_slice(),
        Some(_) => return (Arc::default(), 1),
        None => return (Arc::default(), 0),
    };
    let mut skipped = entries.len().saturating_sub(MAX_TAGS);
    let mut tags: Vec<Arc<str>> = Vec::new();
    for entry in entries.iter().take(MAX_TAGS) {
        match entry
            .as_str()
            .filter(|tag| !tag.is_empty() && tag.len() <= MAX_TAG_BYTES)
        {
            Some(tag) if !tags.iter().any(|old| old.as_ref() == tag) => tags.push(tag.into()),
            Some(_) => {}
            None => skipped += 1,
        }
    }
    (tags.into(), skipped)
}

fn parse_definition(root: &Nbt) -> Option<Definition> {
    let mut states = 1_u64;
    let mut state_axes = Vec::new();
    let mut state_identity_incomplete = false;
    let traits = match root.field("traits") {
        Some(Nbt::List(traits)) => traits.iter().map(|value| (None, value)).collect::<Vec<_>>(),
        Some(Nbt::Compound(traits)) => traits
            .iter()
            .map(|(name, value)| (Some(name.as_str()), value))
            .collect(),
        _ => Vec::new(),
    };
    let enabled = traits
        .iter()
        .flat_map(|(name, trait_)| enabled_flags(trait_, *name))
        .collect::<Vec<_>>();
    // Trait states come first, in vanilla's fixed order, whatever order the traits are listed in.
    for (name, values) in TRAIT_STATES {
        if enabled.iter().any(|state| trait_key(state) == name) {
            states = states.checked_mul(values.len() as u64)?;
            state_axes.push(CustomStateAxis {
                name: format!("minecraft:{name}").into(),
                values: values
                    .iter()
                    .map(|value| CustomStateValue::String((*value).into()))
                    .collect(),
            });
        }
    }
    // The connection trait enables cardinal fence connections: four
    // neighbor-derived booleans the server keeps updated as neighbours
    // change. They read as ordinary block states, so permutations and bone
    // visibility resolve per palette state with the existing machinery.
    // Accept the `cardinal_connections` group or the individual directions;
    // axes run north, east, south, west, matching Dragonfly's bit layout.
    // The order is provisional: no wire capture confirms the server palette
    // order yet, and hashed sessions hash sorted names, so only sequential
    // palettes could observe a mismatch.
    let cardinal = enabled
        .iter()
        .any(|state| trait_key(state) == CARDINAL_CONNECTIONS);
    for direction in CONNECTION_DIRECTIONS
        .into_iter()
        .filter(|direction| cardinal || enabled.iter().any(|state| trait_key(state) == *direction))
    {
        states = states.checked_mul(2)?;
        state_axes.push(CustomStateAxis {
            name: format!("minecraft:{direction}").into(),
            values: [CustomStateValue::Bool(false), CustomStateValue::Bool(true)].into(),
        });
    }
    state_identity_incomplete |= enabled.iter().any(|state| {
        let key = trait_key(state);
        !TRAIT_STATES.iter().any(|(name, _)| *name == key)
            && key != CARDINAL_CONNECTIONS
            && !CONNECTION_DIRECTIONS.contains(&key)
    });
    let mut placement =
        placement::traits(traits.iter().map(|(_, trait_)| *trait_), state_axes.len());
    for property in root.list("properties") {
        let values = property.list("enum");
        states = states.checked_mul(values.len().max(1) as u64)?;
        if let Some(Nbt::String(name)) = property.field("name") {
            let parsed_values = values.iter().filter_map(state_value).collect::<Vec<_>>();
            state_identity_incomplete |= values.is_empty() || parsed_values.len() != values.len();
            state_axes.push(CustomStateAxis {
                name: name.as_str().into(),
                values: parsed_values.into_boxed_slice(),
            });
        } else {
            state_identity_incomplete = true;
        }
    }
    if states > MAX_STATES_PER_BLOCK {
        return None;
    }
    let components = root.field("components");
    let (mining, mining_skips) =
        mining::component(components.and_then(|set| set.field(mining::COMPONENT)));
    placement::components(&mut placement, components);
    let collision_component = components.and_then(|set| set.field("minecraft:collision_box"));
    let collides = collision_enabled(collision_component);
    let (collision_boxes, collision_skips) = collision_components(collision_component);
    let selection =
        selection_component(components.and_then(|set| set.field("minecraft:selection_box")));
    let (friction, friction_skips) =
        friction_component(components.and_then(|set| set.field("minecraft:friction")));
    let mut permutation_skips = 0;
    let mut physical_permutation_skips = 0;
    let mut mining_incomplete = mining_skips > 0;
    let permutations = root
        .list("permutations")
        .iter()
        .enumerate()
        .filter_map(|(index, permutation)| {
            let components = permutation.field("components");
            let changes_mining = components
                .and_then(|set| set.field(mining::COMPONENT))
                .is_some();
            if index >= MAX_PERMUTATIONS {
                permutation_skips += 1;
                physical_permutation_skips += usize::from(!mining::only_component(components));
                mining_incomplete |= changes_mining;
                return None;
            }
            placement::components(&mut placement, components);
            let Some(Nbt::String(condition)) = permutation.field("condition") else {
                permutation_skips += 1;
                physical_permutation_skips += usize::from(!mining::only_component(components));
                mining_incomplete |= changes_mining;
                return None;
            };
            let (physical, skipped, mining_skips) = physical_components(components);
            permutation_skips += skipped + mining_skips;
            physical_permutation_skips += skipped;
            mining_incomplete |= mining_skips > 0;
            Some(CustomPermutation {
                condition: condition.as_str().into(),
                components: visual_components(permutation.field("components")),
                physical,
            })
        })
        .collect();
    let (tags, tag_skips) = block_tags(root);
    let physical_skips = collision_skips
        + friction_skips
        + usize::from(
            components
                .and_then(|set| set.field("minecraft:random_offset"))
                .is_some_and(|value| random_offset_component(value).is_none()),
        )
        + usize::from(
            components
                .and_then(|set| set.field("minecraft:transformation"))
                .is_some_and(|_| transformation_component(components).is_none()),
        );
    Some(Definition {
        mining: mining.unwrap_or_default(),
        tags,
        skipped: tag_skips + mining_skips + physical_skips + permutation_skips,
        state_count: u32::try_from(states).ok()?,
        collides,
        collision_boxes,
        selection,
        friction,
        player_interact: player_interact(components),
        visual: CustomBlockVisuals {
            placement,
            base: visual_components(components),
            item_visual: components
                .and_then(|components| components.field("minecraft:item_visual"))
                .filter(|item| matches!(item, Nbt::Compound(_)))
                .map(|item| {
                    visual_descriptions(
                        None,
                        item.field("geometryDescription"),
                        item.field("materialInstancesDescription"),
                    )
                }),
            permutations,
            components_incomplete: physical_skips > 0 || physical_permutation_skips > 0,
            mining_incomplete,
            state_axes: state_axes.into_boxed_slice(),
            state_identity_incomplete,
            materialization_disabled: false,
        },
    })
}

/// Whether `minecraft:custom_components` declares a player-interact handler.
fn player_interact(components: Option<&Nbt>) -> bool {
    let flag = components
        .and_then(|set| set.field("minecraft:custom_components"))
        .and_then(|component| component.field("hasPlayerInteract"));
    matches!(flag, Some(Nbt::Byte(value)) if *value != 0)
}

/// Reads geometry, material instances, and transformation; odd values are
/// treated as absent so the block keeps its other visuals.
fn visual_components(components: Option<&Nbt>) -> CustomVisualComponents {
    let Some(components) = components else {
        return CustomVisualComponents::default();
    };
    visual_descriptions(
        Some(components),
        components.field("minecraft:geometry"),
        components.field("minecraft:material_instances"),
    )
}

/// Reads geometry and material descriptions shared by placed and inventory visuals.
fn visual_descriptions(
    components: Option<&Nbt>,
    geometry_component: Option<&Nbt>,
    material_component: Option<&Nbt>,
) -> CustomVisualComponents {
    let geometry = match geometry_component {
        Some(Nbt::String(identifier)) => Some(identifier.as_str().into()),
        Some(compound) => match compound.field("identifier") {
            Some(Nbt::String(identifier)) => Some(identifier.as_str().into()),
            _ => None,
        },
        None => None,
    };
    let bone_visibility =
        match geometry_component.and_then(|geometry| geometry.field("bone_visibility")) {
            // Versioned Molang nodes carry an expression field; scalar server variants remain valid.
            Some(Nbt::Compound(bones)) => bones
                .iter()
                .take(MAX_BONE_VISIBILITY)
                .filter_map(|(bone, value)| {
                    let expression: Arc<str> = match value {
                        Nbt::String(expression) => expression.as_str().into(),
                        Nbt::Compound(_) => value.field("expression")?.as_str()?.into(),
                        value => value
                            .number()
                            .filter(|value| value.is_finite())?
                            .to_string()
                            .into(),
                    };
                    Some((bone.as_str().into(), expression))
                })
                .collect(),
            _ => Box::default(),
        };
    let materials = material_component
        .and_then(|instances| match instances.field("materials") {
            Some(Nbt::Compound(fields)) => Some(fields),
            _ => None,
        })
        .map(|fields| {
            fields
                .iter()
                .take(MAX_MATERIAL_INSTANCES)
                .filter_map(|(name, material)| {
                    let Some(Nbt::String(texture)) = material.field("texture") else {
                        return None;
                    };
                    let render_method = match material.field("render_method") {
                        Some(Nbt::String(method)) => Some(method.as_str().into()),
                        _ => None,
                    };
                    let tint_method = match material.field("tint_method") {
                        Some(Nbt::String(method)) => Some(method.as_str().into()),
                        _ => None,
                    };
                    Some(CustomMaterialInstance {
                        name: name.as_str().into(),
                        texture: texture.as_str().into(),
                        render_method,
                        tint_method,
                        ambient_occlusion: material
                            .field("ambient_occlusion")
                            .and_then(Nbt::number)
                            .map(|value| value as f32)
                            .filter(|value| value.is_finite() && *value >= 0.0),
                        face_dimming: material
                            .field("packed_bools")
                            .and_then(Nbt::number)
                            .filter(|value| value.is_finite())
                            .map(|value| value as i64 & 1 != 0)
                            .or_else(|| {
                                material
                                    .field("face_dimming")
                                    .and_then(Nbt::number)
                                    .filter(|value| matches!(value, 0.0 | 1.0))
                                    .map(|value| value != 0.0)
                            }),
                    })
                })
                .collect()
        });
    let transformation = transformation_component(components);
    let nibble = |key: &str, field: &str| {
        components
            .and_then(|components| components.field(key))
            // Native descriptions serialize a compound with a named byte field.
            // Keep scalar definitions supported for servers using the JSON shape.
            .and_then(|component| component.field(field).unwrap_or(component).number())
            .filter(|value| value.is_finite())
            .map(|value| value.clamp(0.0, 15.0) as u8)
    };
    CustomVisualComponents {
        random_offset: components
            .and_then(|components| components.field("minecraft:random_offset"))
            .and_then(random_offset_component),
        geometry,
        geometry_use_block_type_light_absorption: matches!(
            geometry_component.and_then(|geometry| geometry.field("useBlockTypeLightAbsorption")),
            Some(Nbt::Byte(value)) if *value != 0
        ),
        bone_visibility,
        materials,
        transformation,
        light_dampening: nibble("minecraft:light_dampening", "lightLevel")
            .or_else(|| nibble("minecraft:block_light_filter", "lightLevel")),
        light_emission: nibble("minecraft:light_emission", "emission"),
    }
}

/// Placement trait states in the order vanilla adds them to a block (`placement_position`, then
/// `placement_direction`), each with its values in palette order.
const TRAIT_STATES: [(&str, &[&str]); 4] = [
    (
        "block_face",
        &["down", "up", "north", "south", "west", "east"],
    ),
    ("vertical_half", &["bottom", "top"]),
    ("cardinal_direction", &["south", "west", "north", "east"]),
    (
        "facing_direction",
        &["down", "up", "north", "south", "west", "east"],
    ),
];

/// Group state enabling the four cardinal fence connections at once.
const CARDINAL_CONNECTIONS: &str = "cardinal_connections";
/// Cardinal fence connections in palette order: north varies fastest.
const CONNECTION_DIRECTIONS: [&str; 4] = [
    "connection_north",
    "connection_east",
    "connection_south",
    "connection_west",
];

/// Treats namespaced and unnamespaced trait state spellings as the same key.
fn trait_key(name: &str) -> &str {
    name.strip_prefix("minecraft:").unwrap_or(name)
}

fn state_value(value: &Nbt) -> Option<CustomStateValue> {
    match value {
        Nbt::String(value) => Some(CustomStateValue::String(value.as_str().into())),
        Nbt::Byte(value) => Some(CustomStateValue::Bool(*value != 0)),
        Nbt::Short(value) | Nbt::Int(value) | Nbt::Long(value) => {
            Some(CustomStateValue::Int(*value))
        }
        _ => None,
    }
}

/// Reads states and flags using the container key or the trait's own name as its identity.
fn enabled_flags(trait_: &Nbt, name: Option<&str>) -> Vec<String> {
    match trait_.field("enabled_states") {
        Some(Nbt::Int(flags))
            if name
                .or_else(|| trait_.field("name").and_then(Nbt::as_str))
                .is_some_and(|name| trait_key(name) == "connection") =>
        {
            if flags & 1 != 0 {
                vec![CARDINAL_CONNECTIONS.to_owned()]
            } else {
                Vec::new()
            }
        }
        Some(Nbt::List(states)) => states
            .iter()
            .filter_map(Nbt::as_str)
            .map(str::to_owned)
            .collect(),
        Some(Nbt::Compound(fields)) => fields
            .iter()
            .filter(|(_, value)| value.number().is_some_and(|flag| flag != 0.0))
            .map(|(key, _)| key.clone())
            .collect(),
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod compatibility_tests;

#[cfg(test)]
mod lighting_tests;

#[cfg(test)]
mod collision_tests;

#[cfg(test)]
mod random_offset_tests;
