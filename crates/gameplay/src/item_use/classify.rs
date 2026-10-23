//! Item use reads numerical admissions; identifiers are resolved by the world owner.

use assets::item_use::ItemUseKind;
use assets::item_use::ItemUseRequirement as Needs;
use client_world::EffectiveItemUse;
use client_world::ResolvedUseCooldown as Cooldown;

/// The unloaded use retained even while a crossbow's current action is instant.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CrossbowUse {
    pub max_ticks: u32,
    pub slowdown: f64,
    pub cooldown: Option<Cooldown>,
}

/// Action facts copied from the admitted item definition into one gameplay frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AirUse {
    Hold {
        max_ticks: u32,
        needs: Needs,
        slowdown: f64,
        cooldown: Option<Cooldown>,
    },
    Throw {
        cooldown: Option<Cooldown>,
    },
    /// A use transaction that only starts the local arm swing.
    Swing,
    /// A gliding rocket swings without starting a held use or consuming a stack locally.
    GlideRocket,
    Instant {
        unloaded: CrossbowUse,
    },
}

impl AirUse {
    /// Returns the admitted category timer independently from charge state.
    pub const fn cooldown(self) -> Option<Cooldown> {
        match self {
            Self::Swing | Self::GlideRocket => None,
            Self::Throw { cooldown } | Self::Hold { cooldown, .. } => cooldown,
            Self::Instant { unloaded } => unloaded.cooldown,
        }
    }
    /// Crossbow and swing-only actions consume fresh presses; other admitted actions may repeat.
    pub const fn repeats_while_held(self) -> bool {
        !matches!(
            self,
            Self::Instant { .. }
                | Self::Swing
                | Self::Hold {
                    needs: Needs::ArrowOrOffhandRocket,
                    ..
                }
        )
    }
    /// Selects the predicted crossbow charge state while preserving all definition facts.
    pub const fn with_crossbow_charge(self, charged: bool) -> Self {
        let unloaded = match self {
            Self::Instant { unloaded } => unloaded,
            Self::Hold {
                max_ticks,
                needs: Needs::ArrowOrOffhandRocket,
                slowdown,
                cooldown,
            } => CrossbowUse {
                max_ticks,
                slowdown,
                cooldown,
            },
            _ => return self,
        };
        if charged {
            Self::Instant { unloaded }
        } else {
            Self::Hold {
                max_ticks: unloaded.max_ticks,
                needs: Needs::ArrowOrOffhandRocket,
                slowdown: unloaded.slowdown,
                cooldown: unloaded.cooldown,
            }
        }
    }
}

/// Applies only stack-sensitive charge facts to an already resolved item definition.
pub fn classify(profile: EffectiveItemUse, charged: bool, quick_charge: u8) -> Option<AirUse> {
    match profile.kind? {
        ItemUseKind::None => None,
        ItemUseKind::Swing => Some(AirUse::Swing),
        ItemUseKind::Throw => Some(AirUse::Throw {
            cooldown: profile.cooldown,
        }),
        ItemUseKind::GlideRocket => Some(AirUse::GlideRocket),
        ItemUseKind::Hold | ItemUseKind::Crossbow => {
            let max_ticks = profile.ticks?;
            let max_ticks = if profile.kind == Some(ItemUseKind::Crossbow) {
                max_ticks
                    .saturating_sub(
                        u32::from(quick_charge) * assets::item_use::QUICK_CHARGE_TICKS_PER_LEVEL,
                    )
                    .max(1)
            } else {
                max_ticks
            };
            let action = AirUse::Hold {
                max_ticks,
                needs: profile.requirement,
                slowdown: profile.movement,
                cooldown: profile.cooldown,
            };
            Some(if profile.kind == Some(ItemUseKind::Crossbow) {
                action.with_crossbow_charge(charged)
            } else {
                action
            })
        }
    }
}
