use super::*;
use crate::facts::{
    UiItemIcon, dimension_fallback_biome, dimension_id, presentation_generated::ambient_block,
};

#[test]
fn poplar_falling_leaf_effects_follow_species_and_only_admitted_leaf_states() {
    use crate::facts::{BlockFacts, ParticleEffect};
    let records = crate::read_registry_for_protocol(
        crate::pinned_block_registry_bytes(),
        crate::active_content_registry_protocol(),
    )
    .unwrap();
    let facts = BlockFacts::new(&records).unwrap();
    for (name, expected) in [
        (
            "minecraft:red_poplar_leaves",
            ParticleEffect::RedPoplarLeavesParticle,
        ),
        (
            "minecraft:orange_poplar_leaves",
            ParticleEffect::OrangePoplarLeavesParticle,
        ),
        (
            "minecraft:yellow_poplar_leaves",
            ParticleEffect::YellowPoplarLeavesParticle,
        ),
    ] {
        let states = records
            .iter()
            .filter(|record| record.name.as_ref() == name)
            .collect::<Vec<_>>();
        assert!(!states.is_empty(), "missing {name}");
        for record in states {
            assert_eq!(
                facts.get(record.sequential_id).leaf_particle,
                Some(expected)
            );
        }
    }
    assert_eq!(leaf_particle("minecraft:oak_leaves"), None);
    assert_eq!(leaf_particle("minecraft:red_shrub"), None);
    assert_eq!(leaf_particle("fixture:red_poplar_leaves"), None);
    assert_eq!(facts.get(u32::MAX).leaf_particle, None);
}

/// Retains the previous sign texture grammar as a migration oracle.
fn previous_sign_texture(name: &str) -> (String, bool) {
    let name = name.strip_prefix("minecraft:").unwrap_or(name);
    let hanging = name.ends_with("hanging_sign");
    let wood = name
        .trim_end_matches("_hanging_sign")
        .trim_end_matches("hanging_sign")
        .trim_end_matches("_standing_sign")
        .trim_end_matches("standing_sign")
        .trim_end_matches("_wall_sign")
        .trim_end_matches("wall_sign")
        .trim_end_matches("_sign");
    let wood = match wood {
        "" | "oak" | "sign" => "",
        "dark_oak" | "darkoak" => "darkoak",
        other => other,
    };
    let texture = match (hanging, wood) {
        (true, "") => "hanging_sign".to_owned(),
        (true, wood) => format!("hanging_sign_{wood}"),
        (false, "") => "sign".to_owned(),
        (false, wood @ ("mangrove" | "bamboo" | "cherry" | "pale_oak" | "poplar")) => {
            format!("{wood}_sign")
        }
        (false, wood) => format!("sign_{wood}"),
    };
    (format!("textures/ui/{texture}"), hanging)
}

#[test]
fn facts_migration_compiled_sign_rules_preserve_the_previous_grammar_over_the_pinned_registry() {
    let records = crate::read_registry_for_protocol(
        crate::pinned_block_registry_bytes(),
        crate::active_content_registry_protocol(),
    )
    .unwrap();
    for record in records {
        assert_eq!(
            sign_texture(Some(&record.name)),
            previous_sign_texture(&record.name),
            "{}",
            record.name
        );
    }
    for name in [
        "",
        "standing_sign",
        "oak_hanging_sign",
        "fixture:hanging_sign",
        "fixture:unregistered",
    ] {
        assert_eq!(sign_texture(Some(name)), previous_sign_texture(name));
    }
    assert_eq!(sign_texture(None), previous_sign_texture(""));
}

#[test]
fn facts_migration_compiled_ambient_facts_preserve_exact_membership_in_numeric_block_facts() {
    let records = crate::read_registry_for_protocol(
        crate::pinned_block_registry_bytes(),
        crate::active_content_registry_protocol(),
    )
    .unwrap();
    let facts = crate::facts::BlockFacts::new(&records).unwrap();
    for record in records {
        let expected = match record.name.as_ref() {
            "minecraft:fire" => AmbientBlock::Fire,
            "minecraft:portal" => AmbientBlock::Portal,
            _ => AmbientBlock::Other,
        };
        assert_eq!(
            facts
                .get(record.sequential_id)
                .traits
                .contains(crate::registry::BlockTraits::CAMPFIRE),
            matches!(
                record.name.as_ref(),
                "minecraft:campfire" | "minecraft:soul_campfire"
            ),
            "{}",
            record.name,
        );
        assert_eq!(
            facts.get(record.sequential_id).ambient,
            expected,
            "{}",
            record.name
        );
    }
    assert_eq!(ambient_block("fixture:fire"), AmbientBlock::Other);
}

#[test]
fn facts_migration_compiled_icons_and_dimension_routes_preserve_the_existing_requests() {
    let expected = [
        (UiItemIcon::Featured, "minecraft:compass"),
        (UiItemIcon::Realm, "minecraft:ender_pearl"),
        (UiItemIcon::Friend, "minecraft:heart_of_the_sea"),
        (UiItemIcon::Book, "minecraft:book"),
        (UiItemIcon::Construction, "minecraft:brick"),
        (UiItemIcon::Nature, "minecraft:oak_sapling"),
        (UiItemIcon::Equipment, "minecraft:iron_sword"),
        (UiItemIcon::Items, "minecraft:stick"),
        (UiItemIcon::Search, "minecraft:compass"),
        (UiItemIcon::Overworld, "minecraft:grass_block"),
        (UiItemIcon::Nether, "minecraft:netherrack"),
        (UiItemIcon::End, "minecraft:end_stone"),
    ];
    assert_eq!(expected.len(), UiItemIcon::ALL.len());
    for (icon, name) in expected {
        assert_eq!(icon.identifier(), name);
        assert!(crate::item::retail::identifiers().contains(name));
    }
    for (id, dimension, biome) in [
        (0, "minecraft:overworld", "minecraft:plains"),
        (1, "minecraft:nether", "minecraft:hell"),
        (2, "minecraft:the_end", "minecraft:the_end"),
    ] {
        assert_eq!(dimension_id(dimension), Some(id));
        assert_eq!(dimension_fallback_biome(id), Some(biome));
    }
    assert_eq!(dimension_id("fixture:dimension"), None);
    assert_eq!(dimension_fallback_biome(1000), None);
}

#[test]
fn facts_migration_pinned_air_trait_preserves_named_empty_block_admission() {
    let records = crate::read_registry_for_protocol(
        crate::pinned_block_registry_bytes(),
        crate::active_content_registry_protocol(),
    )
    .unwrap();
    for record in records {
        assert_eq!(
            record.traits.contains(crate::registry::BlockTraits::AIR),
            record.name.as_ref() == crate::facts::block::AIR_IDENTIFIER
        );
    }
}

#[test]
fn leaf_effect_classification_matches_the_native_species_vocabulary() {
    for &effect in crate::facts::ParticleEffect::ALL {
        let leaf = effect
            .identifier()
            .strip_suffix("_particle")
            .and_then(leaf_particle);
        assert_eq!(is_leaf_particle(effect), leaf == Some(effect), "{effect:?}");
    }
}
