//! Normalizes compiled behavior facts at the presentation boundary.

use std::sync::Arc;

/// Uses the simulation's shared tick rate rather than restating it in the carrier compiler.
pub(super) fn timing(entry: &assets::CompiledItemAttackTiming) -> protocol::ItemAttackTiming {
    let ticks = |seconds: assets::ItemDisplayScalar| {
        let ticks = (f64::from(seconds.get()) * f64::from(sim::TICKS_PER_SECOND)).round();
        (ticks > 0.0 && ticks <= f64::from(u32::MAX)).then_some(ticks as u32)
    };
    protocol::ItemAttackTiming {
        swing_duration_ticks: entry.swing_duration_seconds.and_then(ticks),
        attack_cooldown: entry.attack_cooldown.as_ref().and_then(|cooldown| {
            Some(protocol::ItemAttackCooldown {
                category: Arc::from(cooldown.category.as_ref()),
                ticks: ticks(cooldown.duration_seconds)?,
            })
        }),
        piercing_weapon: entry.piercing_weapon,
        is_spear: entry.is_spear,
        kinetic_weapon: entry
            .kinetic_weapon
            .map(|weapon| protocol::KineticWeaponTiming {
                delay_ticks: weapon.delay_ticks,
                dismount_ticks: weapon.dismount_ticks,
                knockback_ticks: weapon.knockback_ticks,
                damage_ticks: weapon.damage_ticks,
            }),
    }
}
