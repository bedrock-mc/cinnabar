use assets::RegistryRecord;
use assets::facts::pack::{TextureSelector as S, rules};
use serde::Deserialize;
use serde_json::{Map, Value};

/// The admitted state selector of a model texture array.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ModelTextureSelector {
    Unresolved,
    Single,
    Mushroom {
        block: &'static str,
    },
    LightningRod,
    /// Charge-indexed anchor art; charges past the array fall back to its first entry.
    RespawnAnchor {
        count: usize,
    },
    Door,
    Flower,
    Sapling,
    Powered,
    Index {
        property: &'static str,
        count: usize,
    },
    Stem,
    CropStages,
    Torchflower,
}

impl ModelTextureSelector {
    /// Admits a static array or an existing selector with its full output range.
    pub(super) fn admit(key: &str, count: usize) -> Self {
        let selector = match rules().selector(key) {
            Some(S::Door) => Self::Door,
            Some(S::Flower) => Self::Flower,
            Some(S::Sapling) => Self::Sapling,
            Some(S::Powered) => Self::Powered,
            Some(S::FrostedIce) => Self::Index {
                property: assets::facts::BlockProperty::Age.name(),
                count: 4,
            },
            Some(S::Wheat) => Self::Index {
                property: assets::facts::BlockProperty::Growth.name(),
                count: 8,
            },
            Some(S::Stem) => Self::Stem,
            Some(S::CropStages) => Self::CropStages,
            Some(S::Torchflower) => Self::Torchflower,
            None => match Self::mushroom_block(key) {
                Some(block) => Self::Mushroom { block },
                None => return Self::Single,
            },
        };
        if count == selector.variant_count() {
            selector
        } else {
            Self::Unresolved
        }
    }

    /// Binds rod and anchor selectors to their pack texture keys, including custom keys.
    pub(super) fn bind_record(self, record: &RegistryRecord, count: usize) -> Self {
        if crate::respawn_anchor::is_record(record) {
            if count > 0 {
                Self::RespawnAnchor { count }
            } else {
                Self::Unresolved
            }
        } else if crate::lightning_rod::is_record(record) {
            if count == Self::LightningRod.variant_count() {
                Self::LightningRod
            } else {
                Self::Unresolved
            }
        } else {
            self
        }
    }

    /// Identifies only the retained huge-mushroom face bindings.
    pub(super) fn mushroom_block(key: &str) -> Option<&'static str> {
        rules().mushroom_block(key)
    }

    /// Returns the array width required by this selector's declared outputs.
    fn variant_count(self) -> usize {
        match self {
            Self::Mushroom { .. } => 16,
            Self::Door => 7,
            Self::Flower => 11,
            Self::Sapling => 6,
            Self::LightningRod | Self::Powered | Self::Stem | Self::Torchflower => 2,
            Self::Index { count, .. } | Self::RespawnAnchor { count } => count,
            Self::CropStages => 4,
            Self::Single => 1,
            Self::Unresolved => 0,
        }
    }

    /// Resolves only the state properties and range declared by this binding.
    pub(super) fn select(self, record: &RegistryRecord) -> Option<usize> {
        let count = self.variant_count();
        let selected = match self {
            Self::Mushroom { block } => {
                if record.name.as_ref() != block {
                    return None;
                }
                mushroom_variant_index(record)?
            }
            Self::LightningRod => crate::lightning_rod::state(record)?.1,
            Self::RespawnAnchor { count } => {
                let charge = crate::respawn_anchor::charge(record)?;
                if charge < count { charge } else { 0 }
            }
            Self::Door => rules().variant(S::Door, &record.name)?,
            Self::Flower => rules().variant(S::Flower, &record.name).or_else(|| {
                canonical_u32(
                    &record.canonical_state,
                    assets::facts::BlockProperty::FlowerType.name(),
                )
                .map(|value| value as usize)
            })?,
            Self::Sapling => rules().variant(S::Sapling, &record.name).or_else(|| {
                sapling_variant(canonical_string(
                    &record.canonical_state,
                    assets::facts::BlockProperty::SaplingType.name(),
                )?)
            })?,
            Self::Powered => usize::from(
                rules().is_powered(&record.name)
                    || canonical_u32(
                        &record.canonical_state,
                        assets::facts::BlockProperty::OutputLitBit.name(),
                    ) == Some(1),
            ),
            Self::Index { property, .. } => {
                canonical_u32(&record.canonical_state, property)? as usize
            }
            Self::Stem => usize::from(
                canonical_u32(
                    &record.canonical_state,
                    assets::facts::BlockProperty::FacingDirection.name(),
                )? != 0,
            ),
            Self::CropStages => {
                const STAGES: [usize; 8] = [0, 0, 1, 1, 2, 2, 2, 3];
                *STAGES.get(canonical_u32(
                    &record.canonical_state,
                    assets::facts::BlockProperty::Growth.name(),
                )? as usize)?
            }
            Self::Torchflower => usize::from(
                canonical_u32(
                    &record.canonical_state,
                    assets::facts::BlockProperty::Growth.name(),
                )? >= 4,
            ),
            Self::Single => 0,
            Self::Unresolved => return None,
        };
        (selected < count).then_some(selected)
    }
}

/// Reads a nonnegative integer from a plain or typed canonical state property.
fn canonical_u32(state: &str, property: &str) -> Option<u32> {
    let document = serde_json::from_str::<Map<String, Value>>(state).ok()?;
    canonical_value(document.get(property)?)?
        .as_u64()
        .and_then(|value| u32::try_from(value).ok())
}

/// Reads the supported legacy sapling strings from a canonical state property.
fn canonical_string<'a>(state: &'a str, property: &str) -> Option<&'a str> {
    let document = serde_json::from_str::<Map<String, Value>>(state).ok()?;
    let value = canonical_value(document.get(property)?)?
        .as_str()?
        .to_owned();
    // This helper cannot return data owned by the parsed document, so the only
    // legacy generic selector is handled by its stable canonical spelling.
    match value.as_str() {
        "oak" => Some("oak"),
        "spruce" => Some("spruce"),
        "birch" => Some("birch"),
        "jungle" => Some("jungle"),
        "acacia" => Some("acacia"),
        "dark_oak" => Some("dark_oak"),
        "roofed_oak" => Some("roofed_oak"),
        _ => None,
    }
}

/// Unwraps the value member of a typed property, retaining plain properties.
fn canonical_value(value: &Value) -> Option<&Value> {
    value
        .as_object()
        .and_then(|object| object.get("value"))
        .or(Some(value))
}

/// Maps the retained legacy sapling spellings to their declared array index.
fn sapling_variant(name: &str) -> Option<usize> {
    Some(match name {
        "oak" => 0,
        "spruce" => 1,
        "birch" => 2,
        "jungle" => 3,
        "acacia" => 4,
        "dark_oak" | "roofed_oak" => 5,
        _ => return None,
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HugeMushroomState {
    huge_mushroom_bits: HugeMushroomSelector,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HugeMushroomSelector {
    #[serde(rename = "type")]
    kind: Box<str>,
    value: u8,
}

/// Reads the retained typed huge-mushroom selector, rejecting unsupported values.
pub(super) fn mushroom_variant_index(record: &RegistryRecord) -> Option<usize> {
    let state = serde_json::from_str::<HugeMushroomState>(&record.canonical_state).ok()?;
    (state.huge_mushroom_bits.kind.as_ref() == "int")
        .then_some(usize::from(state.huge_mushroom_bits.value))
        .filter(|&bits| bits <= 15)
}

/// The terrain array entry a built-in block's native class draws, for every key it uses.
pub(super) fn class_variant(record: &RegistryRecord) -> Option<usize> {
    use assets::facts::{StateValue, pack::TextureVariant};
    match assets::facts::pack::texture_variant(&record.name)? {
        TextureVariant::Fixed(index) => Some(index),
        TextureVariant::Flag(property) => Some(usize::from(matches!(
            record.property(property)?,
            StateValue::Bool(true) | StateValue::Int(1)
        ))),
    }
}
