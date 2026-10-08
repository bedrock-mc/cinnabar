use serde::{Deserialize, Deserializer};
use serde_json::{Map, Value};

pub(super) const MAX_BLOCKS: usize = 1_000_000;
pub(super) const MAX_PALETTE: usize = assets::MAX_TEXTURE_LAYERS;
const MAX_JSON_BYTES: usize = 32 * 1024 * 1024;
const MAX_COORDINATE: i32 = 1_000_000;
// Bound each axis as well as total blocks to limit sparse-scene preparation.
const MAX_AXIS_EXTENT: i32 = 1024;

#[derive(Deserialize)]
pub(super) struct Arena {
    pub(super) palette: Vec<PaletteEntry>,
    pub(super) blocks: Vec<[i32; 4]>,
    pub(super) bounds: [i32; 6],
}

#[derive(Deserialize)]
pub(super) struct PaletteEntry {
    pub(super) name: String,
    #[serde(default, deserialize_with = "states_or_empty")]
    pub(super) states: Map<String, Value>,
}

fn states_or_empty<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Map<String, Value>, D::Error> {
    Ok(Option::<Map<String, Value>>::deserialize(deserializer)?.unwrap_or_default())
}

impl Arena {
    pub(super) fn parse(input: &str) -> Result<Self, String> {
        if input.len() > MAX_JSON_BYTES {
            return Err("arena JSON exceeds the 32 MiB browser limit".into());
        }
        let arena: Self =
            serde_json::from_str(input).map_err(|error| format!("invalid arena JSON: {error}"))?;
        arena.validate()?;
        Ok(arena)
    }

    pub(super) fn validate(&self) -> Result<(), String> {
        if self.blocks.len() > MAX_BLOCKS {
            return Err("arena exceeds the one-million-block browser limit".into());
        }
        if self.palette.is_empty() || self.palette.len() > MAX_PALETTE {
            return Err(format!(
                "arena palette must contain 1..={MAX_PALETTE} entries"
            ));
        }
        if !matches!(self.palette[0].name.as_str(), "air" | "minecraft:air") {
            return Err("arena palette index zero must be air".into());
        }
        for entry in &self.palette {
            if entry.name.is_empty() || entry.name.len() > 128 || entry.states.len() > 64 {
                return Err("arena palette entry exceeds the name/state limits".into());
            }
        }
        for axis in 0..3 {
            let minimum = self.bounds[axis];
            let maximum = self.bounds[axis + 3];
            if !coordinate_valid(minimum) || !coordinate_valid(maximum) || minimum > maximum {
                return Err("arena bounds are invalid".into());
            }
            if maximum - minimum + 1 > MAX_AXIS_EXTENT {
                return Err("arena bounds exceed the 1024-block browser extent".into());
            }
        }
        for &[x, y, z, palette] in &self.blocks {
            if palette < 0 || palette as usize >= self.palette.len() {
                return Err("arena block references an unknown palette entry".into());
            }
            for (axis, coordinate) in [x, y, z].into_iter().enumerate() {
                if !coordinate_valid(coordinate)
                    || coordinate < self.bounds[axis]
                    || coordinate > self.bounds[axis + 3]
                {
                    return Err("arena block lies outside its bounded coordinates".into());
                }
            }
        }
        Ok(())
    }
}

fn coordinate_valid(value: i32) -> bool {
    (-MAX_COORDINATE..=MAX_COORDINATE).contains(&value)
}
