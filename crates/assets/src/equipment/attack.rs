//! Optional behavior-pack attack facts carried beside item use durations.

use serde::{Deserialize, Serialize};

use crate::{AssetError, ItemDisplayScalar};

/// One item's authored attack behavior; seconds are normalized by the runtime tick rate.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CompiledItemAttackTiming {
    pub identifier: Box<str>,
    pub swing_duration_seconds: Option<ItemDisplayScalar>,
    pub attack_cooldown: Option<CompiledItemAttackCooldown>,
    #[serde(default)]
    pub is_spear: bool,
    pub piercing_weapon: bool,
    pub kinetic_weapon: Option<CompiledKineticWeaponTiming>,
}

/// A cooldown explicitly declared for attacks, rather than item use.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CompiledItemAttackCooldown {
    pub category: Box<str>,
    pub duration_seconds: ItemDisplayScalar,
}

/// Authored kinetic-weapon windows, already measured in simulation ticks.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CompiledKineticWeaponTiming {
    pub delay_ticks: u32,
    pub dismount_ticks: u32,
    pub knockback_ticks: u32,
    pub damage_ticks: u32,
}

/// Validates ordered optional facts without converting their authored units.
pub(super) fn validate(entries: &[CompiledItemAttackTiming]) -> Result<(), AssetError> {
    if entries.len() > super::MAX_ITEM_TIMINGS {
        return Err(super::invalid("item attack timing count exceeds bound"));
    }
    let valid_seconds = |value: ItemDisplayScalar| {
        value.get() >= 0.0 && ItemDisplayScalar::new(value.get()) == Some(value)
    };
    let mut previous: Option<&str> = None;
    for entry in entries {
        super::validate_identifier(&entry.identifier)?;
        if let Some(cooldown) = &entry.attack_cooldown {
            super::validate_identifier(&cooldown.category)?;
        }
        if previous.is_some_and(|previous| previous >= entry.identifier.as_ref())
            || entry
                .swing_duration_seconds
                .is_some_and(|value| !valid_seconds(value))
            || entry
                .attack_cooldown
                .as_ref()
                .is_some_and(|value| !valid_seconds(value.duration_seconds))
        {
            return Err(super::invalid("invalid or unordered item attack timing"));
        }
        previous = Some(&entry.identifier);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        RuntimeEquipmentCatalog, encode_equipment_catalog_full,
        encode_equipment_catalog_with_attack_timings,
    };

    #[test]
    fn optional_attack_facts_round_trip_without_changing_old_carriers() {
        let old = encode_equipment_catalog_full([1; 32], [2; 32], &[], &[], &[]).unwrap();
        let old_catalog = RuntimeEquipmentCatalog::decode(&old).unwrap();
        assert!(old_catalog.item_attack_timings().is_empty());
        let payload_end = old.len() - super::super::HASH_BYTES - std::mem::size_of::<u32>();
        assert!(
            !std::str::from_utf8(&old[super::super::HEADER_BYTES..payload_end])
                .unwrap()
                .contains("item_attack")
        );
        let attack = CompiledItemAttackTiming {
            identifier: "fixture:spear".into(),
            swing_duration_seconds: ItemDisplayScalar::new(0.75),
            attack_cooldown: Some(CompiledItemAttackCooldown {
                category: "fixture:jab".into(),
                duration_seconds: ItemDisplayScalar::new(0.5).unwrap(),
            }),
            piercing_weapon: true,
            is_spear: true,
            kinetic_weapon: Some(CompiledKineticWeaponTiming {
                delay_ticks: 4,
                dismount_ticks: 30,
                knockback_ticks: 60,
                damage_ticks: 90,
            }),
        };
        let bytes = encode_equipment_catalog_with_attack_timings(
            [1; 32],
            [2; 32],
            &[],
            &[],
            &[],
            std::slice::from_ref(&attack),
        )
        .unwrap();
        assert_eq!(
            RuntimeEquipmentCatalog::decode(&bytes)
                .unwrap()
                .item_attack_timings(),
            std::slice::from_ref(&attack)
        );
        let mut invalid = attack.clone();
        invalid.swing_duration_seconds = ItemDisplayScalar::new(-0.5);
        assert!(
            encode_equipment_catalog_with_attack_timings(
                [1; 32],
                [2; 32],
                &[],
                &[],
                &[],
                &[invalid]
            )
            .is_err()
        );
        assert!(
            encode_equipment_catalog_with_attack_timings(
                [1; 32],
                [2; 32],
                &[],
                &[],
                &[],
                &[attack.clone(), attack]
            )
            .is_err()
        );
    }
}
