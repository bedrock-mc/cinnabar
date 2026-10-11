//! Survival block-destroy progress per simulation tick.
//!
//! Hardness is Bedrock-extracted; tool and harvest classes in the generated table
//! are provisional (see its header). Rows without tool evidence take the slowest
//! rate and unknown blocks yield none, so neither predicts early; rows with
//! provisional tool classes can still predict early where Bedrock differs.

use assets::compiled::block_destroy::{
    BlockDestroyFacts, HarvestRequirement, ProvisionalDestroy, ToolKind,
};

/// Tiered tool materials.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolTier {
    Wood,
    Stone,
    Copper,
    Iron,
    Gold,
    Diamond,
    Netherite,
}

impl ToolTier {
    const fn speed(self) -> f32 {
        match self {
            Self::Wood => 2.0,
            Self::Stone => 4.0,
            Self::Copper => 5.0,
            Self::Iron => 6.0,
            Self::Diamond => 8.0,
            Self::Netherite => 9.0,
            Self::Gold => 12.0,
        }
    }

    const fn harvest_level(self) -> u8 {
        match self {
            Self::Wood | Self::Gold => 0,
            Self::Stone | Self::Copper => 1,
            Self::Iron => 2,
            Self::Diamond => 3,
            Self::Netherite => 4,
        }
    }
}

/// The destroy-relevant class of the held item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HeldTool {
    pub kind: ToolKind,
    /// `None` for untiered tools such as shears.
    pub tier: Option<ToolTier>,
}

impl HeldTool {
    /// Classifies a vanilla tool identifier; `None` for every non-tool item.
    #[must_use]
    pub fn from_identifier(identifier: &str) -> Option<Self> {
        let name = identifier.strip_prefix("minecraft:")?;
        if name == "shears" {
            return Some(Self {
                kind: ToolKind::Shears,
                tier: None,
            });
        }
        let (material, kind) = name.split_once('_')?;
        let tier = match material {
            "wooden" => ToolTier::Wood,
            "stone" => ToolTier::Stone,
            "copper" => ToolTier::Copper,
            "iron" => ToolTier::Iron,
            "golden" => ToolTier::Gold,
            "diamond" => ToolTier::Diamond,
            "netherite" => ToolTier::Netherite,
            _ => return None,
        };
        let kind = ToolKind::parse(kind).filter(|kind| *kind != ToolKind::Shears)?;
        Some(Self {
            kind,
            tier: Some(tier),
        })
    }
}

/// One block's destroy facts from the generated table.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BlockDestroyInfo {
    /// Negative means indestructible.
    pub hardness: f32,
    effective: u8,
    harvest_tools: u8,
    harvest: HarvestRequirement,
    /// A sword speed Bedrock special-cases for this block.
    sword_speed: Option<f32>,
}

impl BlockDestroyInfo {
    /// Projects admitted carrier facts into the existing destroy calculation.
    pub fn from_facts(facts: BlockDestroyFacts) -> Option<Self> {
        match facts {
            BlockDestroyFacts::Unresolved => None,
            BlockDestroyFacts::ProvisionalIndestructible => Some(Self {
                hardness: -1.0,
                effective: 0,
                harvest_tools: 0,
                harvest: HarvestRequirement::Unresolved,
                sword_speed: None,
            }),
            BlockDestroyFacts::Provisional(ProvisionalDestroy {
                hardness_bits,
                effective_tools,
                harvest_tools,
                requirement,
                sword_speed_bits,
            }) => Some(Self {
                hardness: f32::from_bits(hardness_bits),
                effective: effective_tools,
                harvest_tools,
                harvest: requirement,
                sword_speed: (sword_speed_bits != 0).then(|| f32::from_bits(sword_speed_bits)),
            }),
        }
    }

    fn effective_for(&self, kind: ToolKind) -> bool {
        self.effective & kind.bit() != 0
    }

    fn harvestable_with(&self, tool: Option<HeldTool>) -> bool {
        match self.harvest {
            HarvestRequirement::Hand => true,
            HarvestRequirement::Unresolved => false,
            HarvestRequirement::Tool(required) => tool.is_some_and(|tool| {
                self.harvests_with(tool.kind)
                    && tool.tier.map_or(0, ToolTier::harvest_level) >= required
            }),
        }
    }

    fn harvests_with(&self, kind: ToolKind) -> bool {
        self.harvest_tools & kind.bit() != 0
    }
}

/// Player state that scales destroy speed. Effect levels are wire amplifiers.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct DestroyConditions {
    pub tool: Option<HeldTool>,
    pub efficiency_level: u8,
    pub haste_amplifier: Option<i32>,
    pub conduit_power_amplifier: Option<i32>,
    pub mining_fatigue_amplifier: Option<i32>,
    pub on_ground: bool,
    pub flying: bool,
    pub riding: bool,
    pub eyes_in_water: bool,
    pub aqua_affinity: bool,
}

/// Destroy progress gained per tick; `None` for indestructible blocks.
///
/// Zero hardness yields exactly one. The speed/hardness/30-or-100 base follows
/// dragonfly's `block/break_info.go` (MIT). Haste and Mining Fatigue modify both
/// speed and final progress, rounding each result back to float precision.
#[must_use]
pub fn destroy_progress_per_tick(
    block: &BlockDestroyInfo,
    conditions: &DestroyConditions,
) -> Option<f32> {
    if block.hardness < 0.0 {
        return None;
    }
    if block.hardness == 0.0 {
        return Some(1.0);
    }
    let mut speed = tool_speed(block, conditions.tool);
    if speed > 1.0 && conditions.efficiency_level > 0 {
        let level = f32::from(conditions.efficiency_level);
        speed += level * level + 1.0;
    }
    let haste = [
        conditions.haste_amplifier,
        conditions.conduit_power_amplifier,
    ]
    .into_iter()
    .flatten()
    .map(effect_level)
    .max();
    if let Some(level) = haste {
        speed *= 1.0 + 0.2 * level as f32;
    }
    if !speed.is_finite() {
        return Some(0.0);
    }
    let fatigue = if let Some(amplifier) = conditions.mining_fatigue_amplifier {
        // An odd negative amplifier must never yield a faster prediction.
        if amplifier < 0 {
            return Some(0.0);
        }
        let level = effect_level(amplifier);
        speed = (f64::from(speed) * f64::from(0.3_f32).powf(f64::from(level))) as f32;
        Some(level)
    } else {
        None
    };
    if conditions.eyes_in_water && !conditions.aqua_affinity {
        speed /= 5.0;
    }
    if conditions.riding || (!conditions.on_ground && !conditions.flying) {
        speed /= 5.0;
    }
    let divisor = if block.harvestable_with(conditions.tool) {
        30.0
    } else {
        100.0
    };
    let mut progress = speed / block.hardness / divisor;
    if let Some(level) = haste {
        progress = (f64::from(progress) * f64::from(1.2_f32).powf(f64::from(level))) as f32;
    }
    if let Some(level) = fatigue {
        progress = (f64::from(progress) * f64::from(0.7_f32).powf(f64::from(level))) as f32;
    }
    Some(if progress.is_finite() && progress >= 0.0 {
        progress
    } else {
        0.0
    })
}

fn tool_speed(block: &BlockDestroyInfo, tool: Option<HeldTool>) -> f32 {
    let Some(tool) = tool else {
        return 1.0;
    };
    if let (ToolKind::Sword, Some(speed)) = (tool.kind, block.sword_speed) {
        return speed;
    }
    let effective = block.effective_for(tool.kind);
    let harvests = block.harvests_with(tool.kind);
    match (tool.kind, tool.tier) {
        (ToolKind::Sword | ToolKind::Shears, _) if effective && harvests => 15.0,
        // Faster vanilla sword and shears targets are not distinguished by the
        // table; the slowest effective rate never predicts early.
        (ToolKind::Sword, _) if effective => 1.5,
        (ToolKind::Shears, _) if effective => 5.0,
        (_, Some(tier)) if effective => tier.speed(),
        _ => 1.0,
    }
}

const fn effect_level(amplifier: i32) -> i32 {
    if amplifier < 0 {
        0
    } else {
        amplifier.saturating_add(1)
    }
}

#[cfg(test)]
mod tests;
