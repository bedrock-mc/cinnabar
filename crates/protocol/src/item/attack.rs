//! Normalized component facts for item-directed attacks and kinetic presentation.

use std::sync::Arc;

use crate::nbt_tree::Nbt;

/// Authored kinetic phases, measured in simulation ticks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct KineticWeaponTiming {
    pub delay_ticks: u32,
    pub dismount_ticks: u32,
    pub knockback_ticks: u32,
    pub damage_ticks: u32,
}

/// An attack category's timer; the server continues to own damage and movement effects.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ItemAttackCooldown {
    pub category: Arc<str>,
    pub ticks: u32,
}

/// Item-specific attack facts shared by admission and animation.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ItemAttackTiming {
    pub is_spear: bool,
    pub swing_duration_ticks: Option<u32>,
    pub attack_cooldown: Option<ItemAttackCooldown>,
    pub piercing_weapon: bool,
    pub kinetic_weapon: Option<KineticWeaponTiming>,
}

/// Reads finite durations and complete kinetic phases, skipping malformed optional facts.
pub(super) fn parse_attack(components: &Nbt) -> Option<ItemAttackTiming> {
    let ticks = |nbt: Option<&Nbt>| {
        nbt.and_then(Nbt::number)
            .filter(|value| value.is_finite() && *value >= 0.0 && *value <= f64::from(u32::MAX))
            .map(|value| value as u32)
    };
    let seconds = |nbt: Option<&Nbt>| {
        nbt.and_then(Nbt::number)
            .filter(|value| value.is_finite() && *value > 0.0)
            .and_then(super::components::duration_ticks)
            .filter(|value| *value > 0)
    };
    let swing_duration_ticks = seconds(
        components
            .field("minecraft:swing_duration")
            .and_then(|swing| swing.field("value")),
    );
    let attack_cooldown = components.field("minecraft:cooldown").and_then(|cooldown| {
        (cooldown.field("type")?.as_str()? == "attack").then_some(())?;
        let category = cooldown.field("category")?.as_str()?;
        if category.is_empty() || category.len() > super::components::MAX_TEXT_BYTES {
            return None;
        }
        Some(ItemAttackCooldown {
            category: category.into(),
            ticks: seconds(cooldown.field("duration"))?,
        })
    });
    let kinetic_name = "minecraft:kinetic_weapon";
    let kinetic_weapon = components.field(kinetic_name).and_then(|weapon| {
        // Registry entries may wrap the authored fields in the component name again.
        let weapon = weapon.field(kinetic_name).unwrap_or(weapon);
        let phase = |name| ticks(weapon.field(name)?.field("max_duration"));
        Some(KineticWeaponTiming {
            delay_ticks: ticks(weapon.field("delay"))?,
            dismount_ticks: phase("dismount_conditions")?,
            knockback_ticks: phase("knockback_conditions")?,
            damage_ticks: phase("damage_conditions")?,
        })
    });
    let timing = ItemAttackTiming {
        is_spear: components
            .list("item_tags")
            .iter()
            .chain(
                components
                    .field("minecraft:tags")
                    .into_iter()
                    .flat_map(|tags| tags.list("tags")),
            )
            .any(|tag| tag.as_str() == Some("minecraft:is_spear")),
        swing_duration_ticks,
        attack_cooldown,
        piercing_weapon: matches!(
            components.field("minecraft:piercing_weapon"),
            Some(Nbt::Compound(_))
        ),
        kinetic_weapon,
    };
    (timing != ItemAttackTiming::default()).then_some(timing)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds original network-component trees without redistributing a game definition.
    fn component(name: &str, fields: Vec<(&str, Nbt)>) -> (String, Nbt) {
        (
            name.into(),
            Nbt::Compound(
                fields
                    .into_iter()
                    .map(|(key, value)| (key.into(), value))
                    .collect(),
            ),
        )
    }

    #[test]
    fn attack_facts_keep_authored_durations_and_require_the_explicit_spear_tag() {
        let duration = |ticks| Nbt::Compound(vec![("max_duration".into(), Nbt::Int(ticks))]);
        let components = vec![
            component("minecraft:swing_duration", vec![("value", Nbt::Float(0.8))]),
            component(
                "minecraft:cooldown",
                vec![
                    ("category", Nbt::String("test:attack".into())),
                    ("duration", Nbt::Float(0.8)),
                    ("type", Nbt::String("attack".into())),
                ],
            ),
            component("minecraft:piercing_weapon", vec![]),
            component(
                "minecraft:kinetic_weapon",
                vec![
                    ("delay", Nbt::Int(4)),
                    ("dismount_conditions", duration(10)),
                    ("knockback_conditions", duration(30)),
                    ("damage_conditions", duration(50)),
                ],
            ),
        ];
        let mut root = Nbt::Compound(components);
        let facts = parse_attack(&root).unwrap();
        assert_eq!(facts.swing_duration_ticks, Some(16));
        assert_eq!(facts.attack_cooldown.unwrap().ticks, 16);
        assert!(facts.piercing_weapon);
        assert!(!facts.is_spear);
        assert_eq!(
            facts.kinetic_weapon,
            Some(KineticWeaponTiming {
                delay_ticks: 4,
                dismount_ticks: 10,
                knockback_ticks: 30,
                damage_ticks: 50
            })
        );
        let Nbt::Compound(components) = &mut root else {
            unreachable!()
        };
        components.push(component(
            "minecraft:tags",
            vec![(
                "tags",
                Nbt::List(vec![Nbt::String("minecraft:is_spear".into())]),
            )],
        ));
        assert!(parse_attack(&root).unwrap().is_spear);
    }

    #[test]
    fn malformed_optional_facts_do_not_disable_valid_attack_routing() {
        let facts = parse_attack(&Nbt::Compound(vec![
            component(
                "minecraft:swing_duration",
                vec![("value", Nbt::Float(f64::NAN))],
            ),
            component(
                "minecraft:cooldown",
                vec![
                    ("category", Nbt::String("test:use".into())),
                    ("duration", Nbt::Float(1.0)),
                    ("type", Nbt::String("use".into())),
                ],
            ),
            component("minecraft:piercing_weapon", vec![]),
            component("minecraft:kinetic_weapon", vec![("delay", Nbt::Int(-1))]),
        ]))
        .unwrap();
        assert!(facts.piercing_weapon);
        assert_eq!(facts.swing_duration_ticks, None);
        assert_eq!(facts.attack_cooldown, None);
        assert_eq!(facts.kinetic_weapon, None);
    }

    #[test]
    fn registry_item_tags_keep_the_explicit_spear_pose_gate() {
        let mut components = Nbt::Compound(vec![
            component(
                "minecraft:swing_duration",
                vec![("value", Nbt::Float(0.95))],
            ),
            (
                "item_tags".into(),
                Nbt::List(vec![Nbt::String("minecraft:is_spear".into())]),
            ),
        ]);
        let facts = parse_attack(&components).unwrap();
        assert!(
            facts.is_spear,
            "registry tags must reach the native pose gate"
        );
        assert_eq!(facts.swing_duration_ticks, Some(19));
        let Nbt::Compound(fields) = &mut components else {
            unreachable!()
        };
        fields[1].1 = Nbt::List(vec![Nbt::String("test:kinetic_weapon".into())]);
        assert!(!parse_attack(&components).unwrap().is_spear);
    }

    #[test]
    fn registry_kinetic_component_keeps_its_nested_phase_lengths() {
        let duration = |ticks| Nbt::Compound(vec![("max_duration".into(), Nbt::Int(ticks))]);
        let weapon = Nbt::Compound(vec![
            ("delay".into(), Nbt::Int(4)),
            ("dismount_conditions".into(), duration(10)),
            ("knockback_conditions".into(), duration(30)),
            ("damage_conditions".into(), duration(50)),
        ]);
        let components = Nbt::Compound(vec![
            component("minecraft:piercing_weapon", vec![]),
            component(
                "minecraft:kinetic_weapon",
                vec![("minecraft:kinetic_weapon", weapon)],
            ),
        ]);
        let facts = parse_attack(&components).unwrap();
        assert!(facts.piercing_weapon);
        assert_eq!(
            facts.kinetic_weapon,
            Some(KineticWeaponTiming {
                delay_ticks: 4,
                dismount_ticks: 10,
                knockback_ticks: 30,
                damage_ticks: 50,
            }),
            "registry wrappers must not erase the authored kinetic phases"
        );
    }
}
