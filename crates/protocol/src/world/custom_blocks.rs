use std::sync::Arc;

use jolyne::GameData;

use crate::nbt_tree::{Nbt, read_root};

/// States one custom block may contribute before it is treated as malformed.
const MAX_STATES_PER_BLOCK: u64 = 1 << 16;

/// Permutations one custom block may define before extras are ignored.
const MAX_PERMUTATIONS: usize = 1024;
/// Material instances one component set may define before extras are ignored.
const MAX_MATERIAL_INSTANCES: usize = 64;
/// The namespace of vanilla's own blocks.
const VANILLA_NAMESPACE: &str = "minecraft";

/// One server-defined block from StartGame.
#[derive(Debug, Clone, PartialEq)]
pub struct CustomBlock {
    pub name: Arc<str>,
    /// Sequential palette states: the product of property and trait values.
    pub state_count: u32,
    /// False when the definition disables its collision box.
    pub collides: bool,
    /// Explicit `minecraft:collision_box` shape; `None` means a full cube when `collides`.
    pub collision_box: Option<CustomBox>,
    /// `minecraft:selection_box`: what the pick ray targets and the outline traces.
    pub selection: CustomSelection,
    pub visual: Arc<CustomBlockVisuals>,
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

/// An axis-aligned box in block units (`0..=1` on each axis).
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
    /// Block states in definition order, properties first, then trait states.
    pub state_axes: Box<[CustomStateAxis]>,
}

/// Visual components present in one component set; `None` means absent.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CustomVisualComponents {
    pub geometry: Option<Arc<str>>,
    pub materials: Option<Box<[CustomMaterialInstance]>>,
    pub transformation: Option<CustomTransformation>,
    /// `minecraft:light_dampening`, the sky/block light a full block filters (0..=15).
    pub light_dampening: Option<u8>,
    /// `minecraft:light_emission` (0..=15).
    pub light_emission: Option<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustomMaterialInstance {
    /// `*`, a face name, or a named instance a geometry face refers to.
    pub name: Arc<str>,
    pub texture: Arc<str>,
    pub render_method: Option<Arc<str>>,
    /// `tint_method`: `default_foliage`, `birch_foliage`, `evergreen_foliage`, `dry_foliage`, `grass`, `water`, or `none`.
    pub tint_method: Option<Arc<str>>,
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
    pub values: Box<[CustomStateValue]>,
}

impl CustomBlock {
    /// Every combination of the named state axes (last axis varies fastest) with
    /// its network block hash, for sessions whose block ids are hashes.
    #[must_use]
    pub fn hashed_states(&self) -> Vec<CustomHashedState> {
        let axes = &self.visual.state_axes;
        if axes.iter().any(|axis| axis.values.is_empty()) {
            return Vec::new();
        }
        let total = axes.iter().fold(1_u64, |total, axis| {
            total.saturating_mul(axis.values.len() as u64)
        });
        if total > MAX_STATES_PER_BLOCK {
            return Vec::new();
        }
        (0..total)
            .map(|mut index| {
                let mut picks = vec![0_usize; axes.len()];
                for (pick, axis) in picks.iter_mut().zip(axes.iter()).rev() {
                    let len = axis.values.len() as u64;
                    *pick = (index % len) as usize;
                    index /= len;
                }
                let values: Box<[CustomStateValue]> = picks
                    .iter()
                    .zip(axes.iter())
                    .map(|(&pick, axis)| axis.values[pick].clone())
                    .collect();
                CustomHashedState {
                    hash: network_block_hash(&self.name, axes, &values),
                    values,
                }
            })
            .collect()
    }

    /// Vanilla orders the sequential block palette by FNV-1 64 of the name, then the name.
    #[must_use]
    pub fn sort_key(&self) -> u64 {
        block_name_sort_key(&self.name)
    }
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
fn network_block_hash(name: &str, axes: &[CustomStateAxis], values: &[CustomStateValue]) -> u32 {
    let mut states: Vec<(&str, &CustomStateValue)> = axes
        .iter()
        .map(|axis| axis.name.as_ref())
        .zip(values.iter())
        .collect();
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
    /// Every definition carries `vanilla_block_data`; vanilla's own data-driven blocks
    /// are the ones in the vanilla namespace (`Util::isVanillaNamespace` in
    /// `BlockDefinitionGroup::digestServerBlockProperties`). The vanilla palette already
    /// holds their states, so they are not server blocks.
    #[must_use]
    pub fn from_definitions<'a>(
        definitions: impl IntoIterator<Item = (&'a str, &'a [u8])>,
    ) -> Self {
        let mut blocks = Vec::new();
        let mut skipped = 0;
        for (name, bytes) in definitions {
            if name
                .split_once(':')
                .is_some_and(|(namespace, _)| namespace == VANILLA_NAMESPACE)
            {
                continue;
            }
            let Some(root) = read_root(bytes) else {
                skipped += 1;
                continue;
            };
            match parse_definition(&root) {
                Some(definition) => blocks.push(CustomBlock {
                    name: Arc::from(name),
                    state_count: definition.state_count,
                    collides: definition.collides,
                    collision_box: definition.collision_box,
                    selection: definition.selection,
                    visual: Arc::new(definition.visual),
                }),
                None => skipped += 1,
            }
        }
        blocks.sort_by(|left, right| {
            (left.sort_key(), &left.name).cmp(&(right.sort_key(), &right.name))
        });
        Self {
            blocks: blocks.into(),
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
    state_count: u32,
    collides: bool,
    collision_box: Option<CustomBox>,
    selection: CustomSelection,
    visual: CustomBlockVisuals,
}

fn parse_definition(root: &Nbt) -> Option<Definition> {
    let mut states = 1_u64;
    let mut state_axes = Vec::new();
    for property in root.list("properties") {
        let values = property.list("enum");
        states = states.checked_mul(values.len().max(1) as u64)?;
        if let Some(Nbt::String(name)) = property.field("name") {
            let values = values.iter().filter_map(state_value).collect();
            state_axes.push(CustomStateAxis {
                name: name.as_str().into(),
                values,
            });
        }
    }
    for name in root.list("traits").iter().flat_map(|trait_| {
        trait_
            .field("enabled_states")
            .map(enabled_flags)
            .unwrap_or_default()
    }) {
        states = states.checked_mul(trait_state_values(&name))?;
        if let Some(values) = trait_state_names(&name) {
            let integer = name == "facing_direction";
            state_axes.push(CustomStateAxis {
                name: format!("minecraft:{name}").into(),
                values: values
                    .iter()
                    .enumerate()
                    .map(|(index, value)| {
                        if integer {
                            CustomStateValue::Int(index as i64)
                        } else {
                            CustomStateValue::String((*value).into())
                        }
                    })
                    .collect(),
            });
        }
    }
    if states > MAX_STATES_PER_BLOCK {
        return None;
    }
    let components = root.field("components");
    let collides =
        match components.and_then(|components| components.field("minecraft:collision_box")) {
            Some(Nbt::Byte(enabled)) => *enabled != 0,
            Some(compound @ Nbt::Compound(_)) => {
                !matches!(compound.field("enabled"), Some(Nbt::Byte(0)))
            }
            _ => true,
        };
    let collision_box = components
        .and_then(|components| components.field("minecraft:collision_box"))
        .and_then(box_component);
    let selection =
        match components.and_then(|components| components.field("minecraft:selection_box")) {
            Some(Nbt::Byte(0)) => CustomSelection::Disabled,
            Some(compound @ Nbt::Compound(_)) => {
                if matches!(compound.field("enabled"), Some(Nbt::Byte(0))) {
                    CustomSelection::Disabled
                } else {
                    box_component(compound).map_or(CustomSelection::Default, CustomSelection::Box)
                }
            }
            _ => CustomSelection::Default,
        };
    let permutations = root
        .list("permutations")
        .iter()
        .take(MAX_PERMUTATIONS)
        .filter_map(|permutation| {
            let Some(Nbt::String(condition)) = permutation.field("condition") else {
                return None;
            };
            Some(CustomPermutation {
                condition: condition.as_str().into(),
                components: visual_components(permutation.field("components")),
            })
        })
        .collect();
    Some(Definition {
        state_count: u32::try_from(states).ok()?,
        collides,
        collision_box,
        selection,
        visual: CustomBlockVisuals {
            base: visual_components(components),
            permutations,
            state_axes: state_axes.into_boxed_slice(),
        },
    })
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
    let geometry = match components.field("minecraft:geometry") {
        Some(Nbt::String(identifier)) => Some(identifier.as_str().into()),
        Some(compound) => match compound.field("identifier") {
            Some(Nbt::String(identifier)) => Some(identifier.as_str().into()),
            _ => None,
        },
        None => None,
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
        geometry,
        materials,
        transformation,
        light_dampening: nibble("minecraft:light_dampening", "lightLevel"),
        light_emission: nibble("minecraft:light_emission", "emission"),
    }
}

/// Values a placement trait state contributes; unknown states contribute one.
fn trait_state_values(state: &str) -> u64 {
    trait_state_names(state).map_or(1, |values| values.len() as u64)
}

/// Trait state values in the order vanilla enumerates the same block states
/// (public canonical block-state data).
fn trait_state_names(state: &str) -> Option<&'static [&'static str]> {
    match state {
        "cardinal_direction" => Some(&["south", "west", "north", "east"]),
        "facing_direction" | "block_face" => {
            Some(&["down", "up", "north", "south", "west", "east"])
        }
        "vertical_half" => Some(&["bottom", "top"]),
        _ => None,
    }
}

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
mod tests {
    use super::{
        CustomBlock, CustomBlockVisuals, CustomSelection, CustomStateAxis, CustomStateValue,
        Definition, block_name_sort_key,
    };

    fn parse_definition(bytes: &[u8]) -> Option<Definition> {
        super::parse_definition(&crate::nbt_tree::read_root(bytes)?)
    }

    // Every state axis combination appears once with a distinct hash.
    #[test]
    fn hashed_states_enumerate_axes_and_hash_distinctly() {
        let block = CustomBlock {
            name: "ns:b".into(),
            state_count: 6,
            collides: true,
            collision_box: None,
            selection: CustomSelection::Default,
            visual: std::sync::Arc::new(CustomBlockVisuals {
                state_axes: Box::new([
                    CustomStateAxis {
                        name: "ns:a".into(),
                        values: Box::new([
                            CustomStateValue::Bool(false),
                            CustomStateValue::Bool(true),
                        ]),
                    },
                    CustomStateAxis {
                        name: "ns:c".into(),
                        values: Box::new([
                            CustomStateValue::Int(0),
                            CustomStateValue::Int(1),
                            CustomStateValue::Int(2),
                        ]),
                    },
                ]),
                ..CustomBlockVisuals::default()
            }),
        };
        let states = block.hashed_states();
        assert_eq!(states.len(), 6);
        assert_eq!(states[1].values[1], CustomStateValue::Int(1));
        let hashes: std::collections::HashSet<_> = states.iter().map(|state| state.hash).collect();
        assert_eq!(hashes.len(), 6);
        let plain = CustomBlock {
            visual: std::sync::Arc::new(CustomBlockVisuals::default()),
            ..block
        };
        assert_eq!(plain.hashed_states().len(), 1);
    }

    fn string(value: &str) -> Vec<u8> {
        let mut bytes = vec![value.len() as u8];
        bytes.extend_from_slice(value.as_bytes());
        bytes
    }

    fn named(tag: u8, name: &str) -> Vec<u8> {
        let mut bytes = vec![tag];
        bytes.extend(string(name));
        bytes
    }

    #[test]
    fn placement_trait_and_enum_properties_multiply_states() {
        let mut nbt = named(10, "");
        nbt.extend(named(9, "properties"));
        nbt.extend([10, 4]);
        for values in [2_u8, 3] {
            nbt.extend(named(9, "enum"));
            nbt.extend([8, values * 2]);
            for index in 0..values {
                nbt.extend(string(&index.to_string()));
            }
            nbt.push(0);
        }
        nbt.extend(named(9, "traits"));
        nbt.extend([10, 2]);
        nbt.extend(named(10, "enabled_states"));
        nbt.extend(named(1, "cardinal_direction"));
        nbt.extend([1, 0, 0]);
        nbt.extend(named(10, "components"));
        nbt.extend(named(1, "minecraft:collision_box"));
        nbt.extend([0, 0, 0]);
        let definition = parse_definition(&nbt).expect("definition");
        assert_eq!(
            (definition.state_count, definition.collides),
            (2 * 3 * 4, false)
        );
        let axes = &definition.visual.state_axes;
        assert_eq!(axes.len(), 1, "unnamed properties carry no axis");
        assert_eq!(axes[0].name.as_ref(), "minecraft:cardinal_direction");
        assert_eq!(
            axes[0].values[0],
            super::CustomStateValue::String("south".into())
        );
    }

    fn string_field(name: &str, value: &str) -> Vec<u8> {
        let mut bytes = named(8, name);
        bytes.extend(string(value));
        bytes
    }

    #[test]
    fn network_light_descriptions_retain_zero_dampening_and_emission() {
        // Native serialization uses byte tags; accept numeric server variants too.
        for dampening_tag in [1, 3] {
            let mut nbt = named(10, "");
            nbt.extend(named(10, "components"));
            for (component, field, tag, level) in [
                (
                    "minecraft:light_dampening",
                    "lightLevel",
                    dampening_tag,
                    0_u8,
                ),
                ("minecraft:light_emission", "emission", 1, 13),
            ] {
                nbt.extend(named(10, component));
                nbt.extend(named(tag, field));
                nbt.extend([level, 0]); // Zero has the same byte/zigzag-int encoding.
            }
            nbt.extend([0, 0]);
            let visual = parse_definition(&nbt).expect("network definition").visual;
            assert_eq!(visual.base.light_dampening, Some(0));
            assert_eq!(visual.base.light_emission, Some(13));
        }
    }

    #[test]
    fn scalar_light_components_remain_lenient_for_odd_values() {
        use crate::nbt_tree::Nbt;
        let components = |value| Nbt::Compound(vec![("minecraft:light_dampening".into(), value)]);
        for (value, expected) in [
            (Nbt::Int(0), Some(0)),
            (Nbt::Int(30), Some(15)),
            (Nbt::Int(-1), Some(0)),
            (Nbt::Float(f64::NAN), None),
            (Nbt::String("unknown".into()), None),
        ] {
            let visual = super::visual_components(Some(&components(value)));
            assert_eq!(visual.light_dampening, expected);
        }
    }

    #[test]
    fn visual_components_and_permutations_are_retained() {
        let mut nbt = named(10, "");
        nbt.extend(named(10, "components"));
        nbt.extend(named(10, "minecraft:geometry"));
        nbt.extend(string_field("identifier", "geometry.ore"));
        nbt.push(0);
        nbt.extend(named(10, "minecraft:material_instances"));
        nbt.extend(named(10, "materials"));
        nbt.extend(named(10, "*"));
        nbt.extend(string_field("texture", "ore_top"));
        nbt.extend([0, 0, 0]);
        nbt.push(0);
        nbt.extend(named(9, "permutations"));
        nbt.extend([10, 2]);
        nbt.extend(string_field("condition", "q.block_state('x') == 'y'"));
        nbt.extend(named(10, "components"));
        nbt.extend(named(10, "minecraft:transformation"));
        nbt.extend(named(3, "RY"));
        nbt.push(4);
        nbt.extend(named(5, "SX"));
        nbt.extend(2.0_f32.to_le_bytes());
        nbt.extend([0, 0, 0]);
        nbt.push(0);
        let visual = parse_definition(&nbt).expect("definition").visual;
        assert_eq!(visual.base.geometry.as_deref(), Some("geometry.ore"));
        let materials = visual.base.materials.as_deref().expect("materials");
        assert_eq!(
            (materials[0].name.as_ref(), materials[0].texture.as_ref()),
            ("*", "ore_top")
        );
        let permutation = &visual.permutations[0];
        assert_eq!(permutation.condition.as_ref(), "q.block_state('x') == 'y'");
        let transform = permutation
            .components
            .transformation
            .expect("transformation");
        assert_eq!(
            transform.rotation,
            [0, 2, 0],
            "zigzag 4 is two quarter turns"
        );
        assert_eq!(transform.scale, [2.0, 1.0, 1.0]);
    }

    // Origin is bottom-centre in sixteenths; a full 16-cube maps to the unit block.
    #[test]
    fn collision_box_maps_sixteenths_to_block_units() {
        use crate::nbt_tree::Nbt;
        let list = |values: [f64; 3]| Nbt::List(values.map(Nbt::Float).into());
        let boxed = |origin, size| {
            Nbt::Compound(vec![
                ("origin".to_owned(), list(origin)),
                ("size".to_owned(), list(size)),
            ])
        };
        let full = super::box_component(&boxed([-8.0, 0.0, -8.0], [16.0, 16.0, 16.0])).unwrap();
        assert_eq!((full.min, full.max), ([0.0; 3], [1.0; 3]));
        let slab = super::box_component(&boxed([-8.0, 0.0, -8.0], [16.0, 8.0, 16.0])).unwrap();
        assert_eq!(slab.max, [1.0, 0.5, 1.0]);
        assert!(super::box_component(&boxed([0.0; 3], [0.0; 3])).is_none());
    }

    #[test]
    fn review_custom_box_rejects_nonfinite_narrowed_and_computed_coordinates() {
        use super::{Nbt, box_component};
        let boxed = |origin: [f64; 3], size: [f64; 3]| {
            Nbt::Compound(vec![
                ("origin".into(), Nbt::List(origin.map(Nbt::Float).into())),
                ("size".into(), Nbt::List(size.map(Nbt::Float).into())),
            ])
        };
        assert!(box_component(&boxed([-1e100, 0.0, 0.0], [1e100, 16.0, 16.0])).is_none());
        assert!(box_component(&boxed([3e38, 0.0, 0.0], [3e38, 16.0, 16.0])).is_none());
    }

    // A disabled selection box makes the block untargetable; a box overrides the default.
    #[test]
    fn selection_box_component_is_parsed() {
        let selection = |body: Vec<u8>| {
            let mut nbt = named(10, "");
            nbt.extend(named(10, "components"));
            nbt.extend(named(10, "minecraft:selection_box"));
            nbt.extend(body);
            nbt.extend([0, 0, 0]);
            parse_definition(&nbt).expect("definition").selection
        };
        assert_eq!(
            selection(named(1, "enabled").into_iter().chain([0]).collect()),
            CustomSelection::Disabled
        );
        assert_eq!(selection(Vec::new()), CustomSelection::Default);
    }

    #[test]
    fn truncated_definition_is_rejected() {
        assert!(parse_definition(&[10, 0, 9]).is_none());
    }

    // Every StartGame definition carries `vanilla_block_data` with its block id: vanilla
    // asserts on a missing one and numbers server blocks from 10000, as Dragonfly does.
    // Vanilla's own data-driven blocks are those in the vanilla namespace; the vanilla
    // palette already holds their states, while a server block adds its own.
    #[test]
    fn only_vanilla_namespace_definitions_are_not_server_blocks() {
        let definition = |block_id: &[u8]| {
            let mut nbt = named(10, "");
            nbt.extend(named(10, "vanilla_block_data"));
            nbt.extend(named(3, "block_id"));
            nbt.extend_from_slice(block_id);
            nbt.extend([0, 0]);
            nbt
        };
        // Zigzag varints of 1464 and 10000.
        let vanilla = definition(&[0xf0, 0x16]);
        let server = definition(&[0xa0, 0x9c, 0x01]);
        let blocks = super::CustomBlocks::from_definitions([
            ("minecraft:light_gray_concrete_stairs", vanilla.as_slice()),
            ("benergistics:controller", server.as_slice()),
        ]);
        let names = blocks
            .blocks
            .iter()
            .map(|block| block.name.as_ref())
            .collect::<Vec<_>>();
        assert_eq!(
            (names, blocks.skipped),
            (vec!["benergistics:controller"], 0)
        );
    }

    #[test]
    fn sort_key_is_fnv1_64_of_the_name() {
        assert_eq!(block_name_sort_key(""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(block_name_sort_key("a"), 0xaf63_bd4c_8601_b7be);
    }
}
