mod shared_states;
pub use shared_states::SharedStates;
use std::sync::Arc;

use jolyne::GameData;

use crate::nbt_tree::{Nbt, read_root};

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
    pub visual: Arc<CustomBlockVisuals>,
    /// Resolved palette-order component sets; empty until session preparation.
    pub state_physics: Arc<[CustomBlockPhysics]>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub enum CustomSelection {
    /// No component: the pick ray uses the collision shape.
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

/// The render-relevant parts of a custom block definition.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CustomBlockVisuals {
    pub base: CustomVisualComponents,
    /// In definition order; later matching permutations override earlier ones.
    pub permutations: Box<[CustomPermutation]>,
    /// Block states in palette order: placement trait states, then properties in list order.
    /// The first axis varies fastest across the block's palette run.
    pub state_axes: Box<[CustomStateAxis]>,
    /// A declared state could not be represented by the named axes.
    pub state_identity_incomplete: bool,
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

#[derive(Debug, Clone, PartialEq)]
pub struct CustomPermutation {
    pub condition: Arc<str>,
    pub components: CustomVisualComponents,
    pub physical: CustomPhysicalComponents,
}

/// Collision and targeting components present in a permutation; absent fields inherit.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CustomPhysicalComponents {
    pub random_offset: Option<block_transform::random_offset::RandomOffsetComponent>,
    pub collision: Option<CustomCollision>,
    pub selection: Option<CustomSelection>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CustomCollision {
    pub enabled: bool,
    pub boxes: Option<Arc<[CustomBox]>>,
}

/// Effective physical components for one palette state.
#[derive(Debug, Clone, PartialEq)]
pub struct CustomBlockPhysics {
    pub random_offset: Option<block_transform::random_offset::RandomOffsetComponent>,
    pub collides: bool,
    pub collision_boxes: Option<Arc<[CustomBox]>>,
    pub selection: CustomSelection,
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
    /// Falls back to the base component set when a state has not been resolved.
    #[must_use]
    pub fn physics_for_state(&self, index: u32) -> CustomBlockPhysics {
        self.state_physics
            .get(index as usize)
            .cloned()
            .unwrap_or_else(|| self.base_physics())
    }

    #[must_use]
    pub fn base_physics(&self) -> CustomBlockPhysics {
        CustomBlockPhysics {
            random_offset: self.visual.base.random_offset,
            collides: self.collides,
            collision_boxes: self.collision_boxes.clone(),
            selection: self.selection,
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
        if self.visual.state_identity_incomplete {
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
        if self.visual.state_identity_incomplete {
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
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CustomBlocks {
    pub blocks: Arc<[CustomBlock]>,
    /// Vanilla data-driven types admitted by the server's definitions.
    pub vanilla_blocks: Arc<[Arc<str>]>,
    pub skipped: usize,
}

impl CustomBlocks {
    #[must_use]
    pub fn from_game_data(game_data: &GameData) -> Self {
        Self::from_definitions(
            game_data
                .start_game
                .block_properties
                .iter()
                .map(|property| {
                    (
                        property.block_name.as_str(),
                        property.block_definition.0.as_ref(),
                    )
                }),
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
                        name: Arc::from(name),
                        tags: definition.tags,
                        state_count: definition.state_count,
                        collides: definition.collides,
                        collision_boxes: definition.collision_boxes,
                        selection: definition.selection,
                        visual: Arc::new(definition.visual),
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
        blocks.retain(|block| {
            // Keep a palette prefix so rejected states cannot shift later sequential IDs.
            let admitted =
                !exhausted && state_budget.admit(&block.name, &block.visual, block.state_count);
            if !admitted {
                exhausted = true;
                skipped += 1;
            }
            admitted
        });
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
    tags: Arc<[Arc<str>]>,
    skipped: usize,
    state_count: u32,
    collides: bool,
    collision_boxes: Option<Arc<[CustomBox]>>,
    selection: CustomSelection,
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
    let enabled = root
        .list("traits")
        .iter()
        .flat_map(|trait_| {
            trait_
                .field("enabled_states")
                .map(enabled_flags)
                .unwrap_or_default()
        })
        .collect::<Vec<_>>();
    // Trait states come first, in vanilla's fixed order, whatever order the traits are listed in.
    for (name, values) in TRAIT_STATES {
        if enabled
            .iter()
            .any(|state| state.strip_prefix("minecraft:").unwrap_or(state) == name)
        {
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
    state_identity_incomplete |= enabled.iter().any(|state| {
        !TRAIT_STATES
            .iter()
            .any(|(name, _)| *name == state.strip_prefix("minecraft:").unwrap_or(state))
    });
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
    let collision_component = components.and_then(|set| set.field("minecraft:collision_box"));
    let collides = collision_enabled(collision_component);
    let (collision_boxes, collision_skips) = collision_components(collision_component);
    let selection =
        selection_component(components.and_then(|set| set.field("minecraft:selection_box")));
    let mut permutation_skips = root
        .list("permutations")
        .len()
        .saturating_sub(MAX_PERMUTATIONS);
    let permutations = root
        .list("permutations")
        .iter()
        .take(MAX_PERMUTATIONS)
        .filter_map(|permutation| {
            let Some(Nbt::String(condition)) = permutation.field("condition") else {
                permutation_skips += 1;
                return None;
            };
            let (physical, skipped) = physical_components(permutation.field("components"));
            permutation_skips += skipped;
            Some(CustomPermutation {
                condition: condition.as_str().into(),
                components: visual_components(permutation.field("components")),
                physical,
            })
        })
        .collect();
    let (tags, tag_skips) = block_tags(root);
    Some(Definition {
        tags,
        skipped: tag_skips
            + collision_skips
            + permutation_skips
            + usize::from(
                components
                    .and_then(|set| set.field("minecraft:random_offset"))
                    .is_some_and(|value| random_offset_component(value).is_none()),
            ),
        state_count: u32::try_from(states).ok()?,
        collides,
        collision_boxes,
        selection,
        visual: CustomBlockVisuals {
            base: visual_components(components),
            permutations,
            state_axes: state_axes.into_boxed_slice(),
            state_identity_incomplete,
        },
    })
}

fn collision_enabled(component: Option<&Nbt>) -> bool {
    match component {
        Some(Nbt::Byte(enabled)) => *enabled != 0,
        Some(compound @ Nbt::Compound(_)) if compound.field("boxes").is_some() => {
            matches!(compound.field("enabled"), Some(Nbt::Byte(enabled)) if *enabled != 0)
        }
        Some(compound @ Nbt::Compound(_)) => {
            !matches!(compound.field("enabled"), Some(Nbt::Byte(0)))
        }
        _ => true,
    }
}

fn selection_component(component: Option<&Nbt>) -> CustomSelection {
    match component {
        Some(Nbt::Byte(0)) => CustomSelection::Disabled,
        Some(compound @ Nbt::Compound(_))
            if matches!(compound.field("enabled"), Some(Nbt::Byte(0))) =>
        {
            CustomSelection::Disabled
        }
        Some(compound @ Nbt::Compound(_)) => {
            box_component(compound).map_or(CustomSelection::Default, CustomSelection::Box)
        }
        _ => CustomSelection::Default,
    }
}

fn physical_components(components: Option<&Nbt>) -> (CustomPhysicalComponents, usize) {
    let collision = components.and_then(|set| set.field("minecraft:collision_box"));
    let (boxes, skipped) = collision_components(collision);
    (
        CustomPhysicalComponents {
            random_offset: components
                .and_then(|set| set.field("minecraft:random_offset"))
                .and_then(random_offset_component),
            collision: collision.map(|component| CustomCollision {
                enabled: collision_enabled(Some(component)),
                boxes,
            }),
            selection: components
                .and_then(|set| set.field("minecraft:selection_box"))
                .map(|component| selection_component(Some(component))),
        },
        skipped
            + usize::from(
                components
                    .and_then(|set| set.field("minecraft:random_offset"))
                    .is_some_and(|value| random_offset_component(value).is_none()),
            ),
    )
}

/// Decodes authored pixel ranges once, keeping absence distinct from an explicit zero component.
fn random_offset_component(
    value: &Nbt,
) -> Option<block_transform::random_offset::RandomOffsetComponent> {
    use block_transform::random_offset::{RandomOffsetAxis, RandomOffsetComponent};
    if !matches!(value, Nbt::Compound(_)) {
        return None;
    }
    let mut result = RandomOffsetComponent::default();
    for (index, key) in ["x", "y", "z"].into_iter().enumerate() {
        let Some(axis) = value.field(key) else {
            continue;
        };
        if !matches!(axis, Nbt::Compound(_)) {
            return None;
        }
        let range = match axis.field("range") {
            None => [0.0; 2],
            Some(range @ Nbt::Compound(_)) => {
                let read = |key| range.field(key).map_or(Some(0.0), Nbt::number);
                [read("min")? as f32, read("max")? as f32]
            }
            Some(_) => return None,
        };
        let steps = axis.field("steps").map_or(Some(0.0), Nbt::number)?;
        if !steps.is_finite() || steps < 0.0 || steps > f64::from(u32::MAX) || steps.fract() != 0.0
        {
            return None;
        }
        result.axes[index] = RandomOffsetAxis::from_pixels(range, steps as u32);
    }
    result.is_valid().then_some(result)
}

fn collision_components(component: Option<&Nbt>) -> (Option<Arc<[CustomBox]>>, usize) {
    let Some(component) = component else {
        return (None, 0);
    };
    match component.field("boxes") {
        Some(Nbt::List(entries)) if !entries.is_empty() => {
            let mut skipped = entries.len().saturating_sub(MAX_COLLISION_BOXES);
            let boxes = entries
                .iter()
                .take(MAX_COLLISION_BOXES)
                .filter_map(|entry| {
                    let shape = native_collision_box(entry);
                    skipped += usize::from(shape.is_none());
                    shape
                })
                .collect::<Arc<[CustomBox]>>();
            ((!boxes.is_empty()).then_some(boxes), skipped)
        }
        Some(Nbt::List(_)) => (None, 0),
        Some(_) => (None, 1),
        None => (box_component(component).map(|shape| Arc::from([shape])), 0),
    }
}

/// Wire bounds use corner coordinates in sixteenths; X faces opposite world X.
fn native_collision_box(entry: &Nbt) -> Option<CustomBox> {
    if !matches!(entry, Nbt::Compound(_)) {
        return None;
    }
    let mut coordinates = [0.0_f32; 6];
    for (coordinate, name) in coordinates
        .iter_mut()
        .zip(["minX", "minY", "minZ", "maxX", "maxY", "maxZ"])
    {
        *coordinate = match entry.field(name) {
            Some(Nbt::Float(value)) => *value as f32,
            _ => 0.0,
        };
        if !coordinate.is_finite() {
            return None;
        }
    }
    let mut min = [0.0; 3];
    let mut max = [0.0; 3];
    for axis in 0..3 {
        let ceiling = if axis == 1 { 24.0 } else { 16.0 };
        let first = coordinates[axis].clamp(0.0, ceiling) / 16.0;
        let second = coordinates[axis + 3].clamp(0.0, ceiling) / 16.0;
        min[axis] = first.min(second);
        max[axis] = first.max(second);
    }
    (min[0], max[0]) = (1.0 - max[0], 1.0 - min[0]);
    Some(CustomBox { min, max })
}

/// Reads a `{origin, size}` box given in sixteenths from the block's bottom
/// centre, clamped to the block; `None` for anything malformed or empty.
fn box_component(component: &Nbt) -> Option<CustomBox> {
    let triple = |name: &str| -> Option<[f32; 3]> {
        let values = component.list(name);
        if values.len() != 3 {
            return None;
        }
        let mut out = [0.0_f32; 3];
        for (slot, value) in out.iter_mut().zip(values) {
            let number = value.number()?;
            let narrowed = number as f32;
            if !narrowed.is_finite() {
                return None;
            }
            *slot = narrowed;
        }
        Some(out)
    };
    let (origin, size) = (triple("origin")?, triple("size")?);
    let shift = [8.0, 0.0, 8.0];
    let mut min = [0.0_f32; 3];
    let mut max = [0.0_f32; 3];
    for axis in 0..3 {
        let low = origin[axis] + shift[axis];
        let high = low + size[axis];
        if !low.is_finite() || !high.is_finite() {
            return None;
        }
        min[axis] = (low / 16.0).clamp(0.0, 1.0);
        max[axis] = (high / 16.0).clamp(0.0, 1.0);
        if max[axis] <= min[axis] {
            return None;
        }
    }
    Some(CustomBox { min, max })
}

/// Reads geometry, material instances, and transformation; odd values are
/// treated as absent so the block keeps its other visuals.
fn visual_components(components: Option<&Nbt>) -> CustomVisualComponents {
    let Some(components) = components else {
        return CustomVisualComponents::default();
    };
    let geometry_component = components.field("minecraft:geometry");
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
    let materials = components
        .field("minecraft:material_instances")
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
                                    .filter(|value| matches!(*value, 0.0 | 1.0))
                                    .map(|value| value != 0.0)
                            }),
                    })
                })
                .collect()
        });
    let transformation = components
        .field("minecraft:transformation")
        .map(|transform| {
            let number = |key: &str, default: f64| {
                transform
                    .field(key)
                    .and_then(Nbt::number)
                    .unwrap_or(default)
            };
            let quarter = |key: &str| (number(key, 0.0).round() as i64).rem_euclid(4) as i32;
            CustomTransformation {
                rotation: [quarter("RX"), quarter("RY"), quarter("RZ")],
                scale: [
                    number("SX", 1.0) as f32,
                    number("SY", 1.0) as f32,
                    number("SZ", 1.0) as f32,
                ],
                translation: [
                    number("TX", 0.0) as f32,
                    number("TY", 0.0) as f32,
                    number("TZ", 0.0) as f32,
                ],
            }
        })
        .filter(|transform| {
            transform
                .scale
                .iter()
                .chain(&transform.translation)
                .all(|value| value.is_finite())
        });
    let nibble = |key: &str, field: &str| {
        components
            .field(key)
            // Native descriptions serialize a compound with a named byte field.
            // Keep scalar definitions supported for servers using the JSON shape.
            .and_then(|component| component.field(field).unwrap_or(component).number())
            .filter(|value| value.is_finite())
            .map(|value| value.clamp(0.0, 15.0) as u8)
    };
    CustomVisualComponents {
        random_offset: components
            .field("minecraft:random_offset")
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

fn state_value(value: &Nbt) -> Option<CustomStateValue> {
    match value {
        Nbt::String(value) => Some(CustomStateValue::String(value.as_str().into())),
        Nbt::Byte(value) => Some(CustomStateValue::Bool(*value != 0)),
        Nbt::Int(value) => Some(CustomStateValue::Int(*value)),
        _ => None,
    }
}

fn enabled_flags(value: &Nbt) -> Vec<String> {
    match value {
        Nbt::Compound(fields) => fields
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
