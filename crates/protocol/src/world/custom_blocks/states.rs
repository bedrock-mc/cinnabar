//! A custom block's state axes from its traits and properties, packed as the client packs them.

use std::collections::HashSet;

use super::{Arc, CustomStateAxis, CustomStateValue, Nbt};

/// Bits the client packs one block's states into; a state that does not fit always reads its
/// first value.
pub(super) const STATE_BITS: u32 = 16;
/// The most states one custom block can have.
pub(super) const MAX_STATES_PER_BLOCK: u64 = 1 << STATE_BITS;

const FACES: [&str; 6] = ["down", "up", "north", "south", "west", "east"];
const CARDINALS: [&str; 4] = ["south", "west", "north", "east"];
const HALVES: [&str; 2] = ["bottom", "top"];
const CORNERS: [&str; 5] = [
    "none",
    "inner_left",
    "inner_right",
    "outer_left",
    "outer_right",
];
/// Rotations the sixteen-way state takes.
const SIXTEEN_WAY_ROTATIONS: usize = 16;
/// Multi-block parts the client serializes distinctly; later parts serialize as part 0.
const SERIALIZED_MULTI_BLOCK_PARTS: i64 = 4;

/// Group state enabling the four cardinal fence connections at once.
const CARDINAL_CONNECTIONS: &str = "cardinal_connections";
/// Cardinal fence connections in palette order: north varies fastest.
const CONNECTION_DIRECTIONS: [&str; 4] = [
    "connection_north",
    "connection_east",
    "connection_south",
    "connection_west",
];
/// Enabled-state flags whose states the traits below add.
const KNOWN_FLAGS: [&str; 7] = [
    "block_face",
    "vertical_half",
    "cardinal_direction",
    "corner_and_cardinal_direction",
    "facing_direction",
    "sixteen_way_rotation",
    "multi_block",
];

/// A block's states in palette order and how many palette states they span.
pub(super) struct StateAxes {
    pub(super) axes: Vec<CustomStateAxis>,
    /// Leading axes supplied by traits rather than properties.
    pub(super) trait_count: usize,
    pub(super) count: u32,
    /// A declared state could not be represented by the named axes.
    pub(super) identity_incomplete: bool,
}

/// Reads trait states first, in the order the client adds them, then properties in list order.
/// A repeated state name is ignored, as the client refuses to add a state twice.
pub(super) fn state_axes(root: &Nbt, traits: &[(Option<&str>, &Nbt)]) -> StateAxes {
    let mut enabled = Vec::new();
    let mut multi_block_parts = None;
    for (name, trait_) in traits {
        let flags = enabled_flags(trait_, *name);
        let trait_name = name.or_else(|| trait_.field("name").and_then(Nbt::as_str));
        if trait_name.is_some_and(|name| trait_key(name) == "multi_block")
            && flags.iter().any(|flag| trait_key(flag) == "multi_block")
        {
            multi_block_parts = match trait_.field("part_count") {
                Some(Nbt::Byte(parts)) if *parts != 0 => Some(i64::from(*parts as u8)),
                _ => None,
            };
        }
        enabled.extend(flags);
    }
    let flag = |key: &str| enabled.iter().any(|state| trait_key(state) == key);
    let mut identity_incomplete = enabled.iter().any(|state| {
        let key = trait_key(state);
        !KNOWN_FLAGS.contains(&key)
            && key != CARDINAL_CONNECTIONS
            && !CONNECTION_DIRECTIONS.contains(&key)
    });
    let strings = |values: &[&str]| -> Box<[CustomStateValue]> {
        values
            .iter()
            .map(|value| CustomStateValue::String((*value).into()))
            .collect()
    };
    let mut axes = Vec::new();
    let mut push = |name: &str, values: Box<[CustomStateValue]>| {
        axes.push(CustomStateAxis {
            name: format!("minecraft:{name}").into(),
            values,
        });
    };
    if flag("block_face") {
        push("block_face", strings(&FACES));
    }
    if flag("vertical_half") {
        push("vertical_half", strings(&HALVES));
    }
    let corner = flag("corner_and_cardinal_direction");
    if flag("cardinal_direction") || corner {
        push("cardinal_direction", strings(&CARDINALS));
    }
    if corner {
        push("corner", strings(&CORNERS));
    }
    if flag("facing_direction") {
        push("facing_direction", strings(&FACES));
    }
    if flag("sixteen_way_rotation") {
        // The client serializes these rotations through its facing names, so rotations past the
        // sixth share the first name.
        let names = (0..SIXTEEN_WAY_ROTATIONS)
            .map(|rotation| FACES.get(rotation).copied().unwrap_or(FACES[0]))
            .collect::<Vec<_>>();
        push("sixteen_way_rotation", strings(&names));
    }
    let cardinal = flag(CARDINAL_CONNECTIONS);
    for direction in CONNECTION_DIRECTIONS {
        if cardinal || flag(direction) {
            push(
                direction,
                [CustomStateValue::Bool(false), CustomStateValue::Bool(true)].into(),
            );
        }
    }
    if let Some(parts) = multi_block_parts {
        let values = (0..parts)
            .map(|part| {
                CustomStateValue::Int(if part < SERIALIZED_MULTI_BLOCK_PARTS {
                    part
                } else {
                    0
                })
            })
            .collect();
        push("multi_block_part", values);
    }
    let trait_count = axes.len();
    let mut counts = axes
        .iter()
        .map(|axis| axis.values.len())
        .collect::<Vec<_>>();
    let mut names = axes
        .iter()
        .map(|axis| Arc::clone(&axis.name))
        .collect::<HashSet<_>>();
    for property in root.list("properties") {
        // A property without an enum list adds no state; a missing name is the empty name.
        let Some(Nbt::List(values)) = property.field("enum") else {
            continue;
        };
        if values.is_empty() {
            continue;
        }
        let name: Arc<str> = property
            .field("name")
            .and_then(Nbt::as_str)
            .unwrap_or_default()
            .into();
        if !names.insert(Arc::clone(&name)) {
            continue;
        }
        let parsed = values.iter().filter_map(state_value).collect::<Box<[_]>>();
        identity_incomplete |= parsed.len() != values.len();
        axes.push(CustomStateAxis {
            name,
            values: parsed,
        });
        counts.push(values.len());
    }
    let count = pack(&mut axes, &counts);
    StateAxes {
        axes,
        trait_count,
        count,
        identity_incomplete,
    }
}

/// Gives each state `ceil(log2(values))` bits (at least one) in order; a state that does not fit
/// in the block's state bits keeps only its first value. Returns the palette state count, from
/// the declared value counts.
fn pack(axes: &mut [CustomStateAxis], counts: &[usize]) -> u32 {
    let mut offset = 0;
    let mut count = 1_u32;
    for (axis, &values) in axes.iter_mut().zip(counts) {
        let bits = values.next_power_of_two().trailing_zeros().max(1);
        if offset + bits > STATE_BITS {
            axis.values = axis.values.iter().take(1).cloned().collect();
        } else {
            count *= values as u32;
        }
        offset += bits;
    }
    count
}

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
