//! Destroy facts retained by a checked world carrier.

use std::{collections::BTreeMap, sync::OnceLock};

use sha2::{Digest, Sha256};

/// The single generated hardness and provisional tool source.
pub const DESTROY_SOURCE: &str = include_str!("../../data/block-destroy-v2193.tsv");

/// Hashes destroy source text with LF endings for carrier encoding and admission.
pub(crate) fn destroy_source_sha256(source: &str) -> [u8; 32] {
    Sha256::digest(source.replace("\r\n", "\n").as_bytes()).into()
}

/// Tool families that change destroy speed or harvestability.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolKind {
    Pickaxe,
    Axe,
    Shovel,
    Hoe,
    Sword,
    Shears,
}

impl ToolKind {
    /// Returns the single carrier bit assigned to this tool family.
    pub const fn bit(self) -> u8 {
        1 << self as u8
    }

    /// Reads a supported tool family name.
    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "pickaxe" => Self::Pickaxe,
            "axe" => Self::Axe,
            "shovel" => Self::Shovel,
            "hoe" => Self::Hoe,
            "sword" => Self::Sword,
            "shears" => Self::Shears,
            _ => return None,
        })
    }
}

/// Every carrier bit defined by the supported tool family enum.
const TOOL_MASK: u8 = ToolKind::Shears.bit() * 2 - 1;

/// The tool requirement used by the existing destroy predictor.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum HarvestRequirement {
    Hand,
    Tool(u8),
    Unresolved,
}

/// A retained prediction whose complete native tool behavior remains incomplete.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ProvisionalDestroy {
    pub hardness_bits: u32,
    pub effective_tools: u8,
    pub harvest_tools: u8,
    pub requirement: HarvestRequirement,
    pub sword_speed_bits: u32,
}

/// Missing evidence is separate from the existing indestructible prediction.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum BlockDestroyFacts {
    #[default]
    Unresolved,
    ProvisionalIndestructible,
    Provisional(ProvisionalDestroy),
    /// A resolved server component value, including negative and instant values.
    Custom {
        seconds_to_destroy_bits: u32,
        effective_tools: u8,
    },
}

impl BlockDestroyFacts {
    /// Rejects non-finite values and unsupported tool or harvest encodings.
    pub(crate) fn validate(self) -> Result<(), crate::AssetError> {
        if let Self::Custom {
            seconds_to_destroy_bits,
            effective_tools,
        } = self
            && (!f32::from_bits(seconds_to_destroy_bits).is_finite()
                || effective_tools & !TOOL_MASK != 0)
        {
            return Err(invalid());
        }
        if let Self::Provisional(facts) = self {
            let hardness = f32::from_bits(facts.hardness_bits);
            let speed = f32::from_bits(facts.sword_speed_bits);
            if !hardness.is_finite()
                || hardness < 0.0
                || !speed.is_finite()
                || speed < 0.0
                || facts.effective_tools & !TOOL_MASK != 0
                || facts.harvest_tools & !TOOL_MASK != 0
                || matches!(facts.requirement, HarvestRequirement::Tool(level) if level > 4)
            {
                return Err(invalid());
            }
        }
        Ok(())
    }

    /// Appends the canonical twelve-byte representation after validation.
    pub(crate) fn write(self, bytes: &mut Vec<u8>) {
        match self {
            Self::Custom {
                seconds_to_destroy_bits,
                effective_tools,
            } => {
                bytes.extend_from_slice(&seconds_to_destroy_bits.to_le_bytes());
                bytes.push(effective_tools);
                bytes.extend_from_slice(&[0; 6]);
                bytes.push(3);
            }
            Self::Unresolved | Self::ProvisionalIndestructible => {
                bytes.extend_from_slice(&[0; 11]);
                bytes.push(u8::from(matches!(self, Self::ProvisionalIndestructible)) * 2);
            }
            Self::Provisional(facts) => {
                bytes.extend_from_slice(&facts.hardness_bits.to_le_bytes());
                bytes.extend_from_slice(&facts.sword_speed_bits.to_le_bytes());
                bytes.push(facts.effective_tools);
                bytes.push(facts.harvest_tools);
                bytes.push(match facts.requirement {
                    HarvestRequirement::Hand => 0,
                    HarvestRequirement::Tool(level) => level + 1,
                    HarvestRequirement::Unresolved => 255,
                });
                bytes.push(1);
            }
        }
    }

    /// Reads a bounded record and rejects noncanonical unsupported payloads.
    pub(crate) fn read(bytes: &[u8]) -> Result<Self, crate::AssetError> {
        if bytes.len() != 12 {
            return Err(invalid());
        }
        let facts = match bytes[11] {
            3 if bytes[5..11] == [0; 6] => Self::Custom {
                seconds_to_destroy_bits: u32::from_le_bytes(bytes[..4].try_into().unwrap()),
                effective_tools: bytes[4],
            },
            0 | 2 if bytes[..11] == [0; 11] => {
                if bytes[11] == 0 {
                    Self::Unresolved
                } else {
                    Self::ProvisionalIndestructible
                }
            }
            1 => Self::Provisional(ProvisionalDestroy {
                hardness_bits: u32::from_le_bytes(bytes[..4].try_into().unwrap()),
                sword_speed_bits: u32::from_le_bytes(bytes[4..8].try_into().unwrap()),
                effective_tools: bytes[8],
                harvest_tools: bytes[9],
                requirement: match bytes[10] {
                    0 => HarvestRequirement::Hand,
                    1..=5 => HarvestRequirement::Tool(bytes[10] - 1),
                    255 => HarvestRequirement::Unresolved,
                    _ => return Err(invalid()),
                },
            }),
            _ => return Err(invalid()),
        };
        facts.validate()?;
        Ok(facts)
    }
}

/// Produces one consistently bounded carrier error for invalid destroy facts.
fn invalid() -> crate::AssetError {
    crate::AssetError::InvalidCompiledAssets {
        detail: "invalid compiled destroy facts".into(),
    }
}

/// Resolves the existing source during compilation, never on a frame or tick path.
pub fn builtin_destroy_facts(identifier: &str) -> BlockDestroyFacts {
    static CATALOG: OnceLock<BTreeMap<&'static str, BlockDestroyFacts>> = OnceLock::new();
    let catalog = CATALOG.get_or_init(|| {
        DESTROY_SOURCE
            .lines()
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .map(|line| parse(line).expect("generated destroy source is valid"))
            .collect()
    });
    catalog.get(identifier).copied().unwrap_or_default()
}

/// Parses a generated row while retaining the existing provisional sword exception.
fn parse(line: &'static str) -> Option<(&'static str, BlockDestroyFacts)> {
    let mut fields = line.split('\t');
    let identifier = fields.next()?;
    let hardness: f32 = fields.next()?.parse().ok()?;
    let effective = fields.next()?;
    let harvest = fields.next()?;
    let level = fields.next()?;
    if fields.next().is_some() || !hardness.is_finite() {
        return None;
    }
    let facts = if hardness < 0.0 {
        BlockDestroyFacts::ProvisionalIndestructible
    } else {
        let (effective_tools, harvest_tools, requirement) =
            if (effective, harvest, level) == ("?", "?", "?") {
                (0, 0, HarvestRequirement::Unresolved)
            } else {
                (
                    tool_set(effective)?,
                    tool_set(harvest)?,
                    if level == "-" {
                        HarvestRequirement::Hand
                    } else {
                        HarvestRequirement::Tool(level.parse().ok()?)
                    },
                )
            };
        BlockDestroyFacts::Provisional(ProvisionalDestroy {
            hardness_bits: hardness.to_bits(),
            effective_tools,
            harvest_tools,
            requirement,
            sword_speed_bits: if crate::facts::block_render_fact(identifier).fast_sword {
                30.0_f32.to_bits()
            } else {
                0
            },
        })
    };
    facts.validate().ok()?;
    Some((identifier, facts))
}

/// Encodes only the six tool families supported by the existing predictor.
fn tool_set(value: &str) -> Option<u8> {
    if value == "-" {
        return Some(0);
    }
    value
        .split(',')
        .try_fold(0, |set, tool| Some(set | ToolKind::parse(tool)?.bit()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn destroy_source_identity_ignores_checkout_line_endings() {
        let lf = DESTROY_SOURCE.replace("\r\n", "\n");
        let crlf = lf.replace('\n', "\r\n");
        let expected: [u8; 32] = Sha256::digest(lf.as_bytes()).into();
        assert_eq!(destroy_source_sha256(&lf), expected);
        assert_eq!(destroy_source_sha256(&crlf), expected);
        assert_eq!(
            destroy_source_sha256(&lf.replacen('\n', "\r\n", 1)),
            expected
        );
        assert_ne!(destroy_source_sha256(&format!("{lf}changed")), expected);
    }

    #[test]
    fn destroy_records_keep_unknown_distinct_and_reject_invalid_facts() {
        for facts in [
            BlockDestroyFacts::Custom {
                seconds_to_destroy_bits: 2.0_f32.to_bits(),
                effective_tools: ToolKind::Pickaxe.bit(),
            },
            BlockDestroyFacts::Custom {
                seconds_to_destroy_bits: (-1.0_f32).to_bits(),
                effective_tools: 0,
            },
            BlockDestroyFacts::Unresolved,
            builtin_destroy_facts("minecraft:stone"),
            builtin_destroy_facts("minecraft:bedrock"),
        ] {
            let mut bytes = Vec::new();
            facts.write(&mut bytes);
            assert_eq!(BlockDestroyFacts::read(&bytes).unwrap(), facts);
        }
        assert_eq!(
            builtin_destroy_facts("custom:stone"),
            BlockDestroyFacts::Unresolved
        );
        assert_eq!(
            builtin_destroy_facts("minecraft:bedrock"),
            BlockDestroyFacts::ProvisionalIndestructible
        );
        let mut bytes = Vec::new();
        builtin_destroy_facts("minecraft:stone").write(&mut bytes);
        bytes[..4].copy_from_slice(&f32::NAN.to_bits().to_le_bytes());
        assert!(BlockDestroyFacts::read(&bytes).is_err());
        assert!(BlockDestroyFacts::read(&[3; 12]).is_err());
    }
}
