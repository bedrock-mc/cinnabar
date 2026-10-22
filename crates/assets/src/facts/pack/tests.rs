use super::*;

/// Loads both pinned palettes to exercise current and retained legacy texture routes.
fn block_names() -> BTreeSet<String> {
    [
        crate::pinned_block_registry_bytes(),
        include_bytes!("../../../data/block-registry-v1001.bin").as_slice(),
    ]
    .into_iter()
    .flat_map(|bytes| {
        let protocol = crate::registry_header_protocol(bytes).unwrap();
        crate::read_registry_for_protocol(bytes, protocol)
            .unwrap()
            .iter()
            .map(|record| record.name.to_string())
            .collect::<Vec<_>>()
    })
    .collect()
}

#[test]
fn pack_rules_validate_every_pinned_contract() {
    rules().validate().unwrap();
}

#[test]
fn pack_rules_reject_stale_keys_and_incomplete_products() {
    let mut document: serde_json::Value = serde_json::from_slice(BLOCK_POLICY_BYTES).unwrap();
    document["pack_rules"]["textures"]["CakeSide"] = "removed_texture".into();
    let broken: PackRules = serde_json::from_value(document["pack_rules"].clone()).unwrap();
    assert!(broken.validate().unwrap_err().contains("removed_texture"));
    document["pack_rules"]["textures"]["CakeSide"] = "cake_side".into();
    document["pack_rules"]["variants"]["Door"]
        .as_array_mut()
        .unwrap()
        .pop();
    let broken: PackRules = serde_json::from_value(document["pack_rules"].clone()).unwrap();
    assert!(broken.validate().unwrap_err().contains("product"));
}

#[test]
fn pack_rules_preserve_legacy_selector_membership_and_indices() {
    let old = [
        (
            TextureSelector::Door,
            vec![
                "wooden_door",
                "spruce_door",
                "birch_door",
                "jungle_door",
                "acacia_door",
                "dark_oak_door",
                "iron_door",
            ],
        ),
        (
            TextureSelector::Flower,
            vec![
                "poppy",
                "blue_orchid",
                "allium",
                "azure_bluet",
                "red_tulip",
                "orange_tulip",
                "white_tulip",
                "pink_tulip",
                "oxeye_daisy",
                "cornflower",
                "lily_of_the_valley",
            ],
        ),
        (
            TextureSelector::Sapling,
            vec![
                "oak_sapling",
                "spruce_sapling",
                "birch_sapling",
                "jungle_sapling",
                "acacia_sapling",
                "dark_oak_sapling",
            ],
        ),
    ];
    for name in block_names() {
        let bare = name.strip_prefix("minecraft:").unwrap_or(&name);
        for (selector, members) in &old {
            let expected = members.iter().position(|member| *member == bare);
            assert_eq!(rules().variant(*selector, &name), expected, "{name}");
            assert_eq!(rules().variant(*selector, bare), expected, "{bare}");
        }
        assert_eq!(
            rules().is_powered(&name),
            bare.starts_with("powered"),
            "{name}"
        );
    }
    for row in include_str!("../../../data/vanilla-pack-vocabulary-1.26.50.tsv").lines() {
        let (domain, key) = row.split_once('\t').unwrap();
        if domain != "terrain_texture" {
            continue;
        }
        let expected = match key {
            "door_lower" | "door_upper" => Some(TextureSelector::Door),
            "red_flower" => Some(TextureSelector::Flower),
            "sapling" => Some(TextureSelector::Sapling),
            "repeater_up" | "comparator_up" => Some(TextureSelector::Powered),
            "frosted_ice" => Some(TextureSelector::FrostedIce),
            "wheat" => Some(TextureSelector::Wheat),
            "melon_stem" | "pumpkin_stem" => Some(TextureSelector::Stem),
            "carrots" | "potatoes" | "beetroot" => Some(TextureSelector::CropStages),
            "torchflower_crop" => Some(TextureSelector::Torchflower),
            _ => None,
        };
        assert_eq!(rules().selector(key), expected, "{key}");
        let mushroom = [
            ("mushroom_brown_", "minecraft:brown_mushroom_block"),
            ("mushroom_red_", "minecraft:red_mushroom_block"),
            ("mushroom_stem_", "minecraft:mushroom_stem"),
        ]
        .into_iter()
        .find_map(|(prefix, block)| {
            key.strip_prefix(prefix)
                .filter(|face| {
                    matches!(
                        *face,
                        "west" | "east" | "bottom" | "top" | "north" | "south"
                    )
                })
                .map(|_| block)
        });
        assert_eq!(rules().mushroom_block(key), mushroom, "{key}");
    }
    assert_eq!(
        rules().variant(TextureSelector::Door, "custom:wooden_door"),
        None
    );
    assert_eq!(rules().mushroom_block("mushroom_red_unknown"), None);
}

#[test]
fn pack_rules_preserve_alpha_and_leather_sprite_membership() {
    for row in include_str!("../../../data/vanilla-pack-vocabulary-1.26.50.tsv").lines() {
        let (domain, key) = row.split_once('\t').unwrap();
        if domain == "terrain_texture" {
            assert_eq!(
                rules().supports_alpha(key),
                matches!(key, "bamboo_fence_gate" | "beacon_core" | "beacon_shell"),
                "{key}"
            );
        }
    }
    for record in &super::super::ItemFacts::builtin()
        .unwrap()
        .payload()
        .records
    {
        let stem = record.identifier.strip_prefix("minecraft:").unwrap();
        let expected = matches!(
            stem,
            "leather_helmet"
                | "leather_chestplate"
                | "leather_leggings"
                | "leather_boots"
                | "leather_horse_armor"
        );
        for extension in ["png", "tga"] {
            assert_eq!(
                leather_sprite(&format!("textures/items/{stem}.{extension}")),
                expected,
                "{stem}"
            );
        }
    }
    for path in [
        "textures/items/leather_helmet_overlay.png",
        "textures/entity/leather_helmet.png",
        "textures/items/leather_helmet.jpg",
    ] {
        assert!(!leather_sprite(path));
    }
}

#[test]
fn every_vanilla_pack_material_has_a_kind() {
    for name in &vocabulary()["material"] {
        assert!(rules().material(name).is_some(), "{name}");
    }
    assert_eq!(rules().material("spider"), Some(MaterialKind::Provisional));
    assert_eq!(
        rules().material("spider_invisible"),
        Some(MaterialKind::Provisional)
    );
    assert_eq!(rules().material_state("spider"), None);
    assert_eq!(rules().material("custom"), None);
}

#[test]
fn pack_rules_reject_a_listed_provisional_material() {
    let mut document: serde_json::Value = serde_json::from_slice(BLOCK_POLICY_BYTES).unwrap();
    document["pack_rules"]["materials"]["spider"] = "Provisional".into();
    let broken: PackRules = serde_json::from_value(document["pack_rules"].clone()).unwrap();
    assert!(broken.validate().unwrap_err().contains("provisional"));
}

#[test]
fn pack_rules_preserve_material_alias_states_and_rasters() {
    let mut names = BTreeSet::new();
    for row in include_str!("../../../data/vanilla-pack-vocabulary-1.26.50.tsv").lines() {
        let (domain, name) = row.split_once('\t').unwrap();
        if domain == "material" {
            names.insert(name);
        }
    }
    names.extend([
        "entity_dissolve_layer0",
        "entity_dissolve_layer1",
        "ender_dragon.skinning",
        "custom",
        "elytra",
        "harness",
    ]);
    for name in names {
        let expected = match name {
            "ender_dragon" => EntityRenderMaterial::Dragon,
            "entity_dissolve_layer0" | "entity_dissolve_layer0.skinning" => {
                EntityRenderMaterial::DissolveDepth
            }
            "entity_dissolve_layer1" | "entity_dissolve_layer1.skinning" => {
                EntityRenderMaterial::DissolveColor
            }
            _ => EntityRenderMaterial::Default,
        };
        assert_eq!(rules().raster(name), expected, "{name}");
        let mut state = EntityRenderMaterialState::default();
        let expected = match name {
            "skeleton" | "entity_alphatest" | "sulfur_cube" | "harness" => {
                state.alpha_test = true;
                state.cull = false;
                Some(state)
            }
            "experience_orb" => {
                state.alpha_test = true;
                Some(state)
            }
            "player_spectator" | "slime_outer" | "sulfur_cube_outer" => {
                state.blend = true;
                Some(state)
            }
            "breeze_wind" => {
                state.alpha_test = true;
                state.blend = true;
                state.cull = false;
                state.depth_write = false;
                state.disable_overlay = true;
                Some(state)
            }
            "charged_creeper" => {
                state.alpha_test = true;
                state.blend = true;
                state.cull = false;
                state.additive = true;
                state.disable_overlay = true;
                Some(state)
            }
            _ => None,
        };
        assert_eq!(rules().material_state(name), expected, "{name}");
    }
}

#[test]
fn pack_rules_preserve_actor_egg_renames_and_item_precedence() {
    for actor in include_str!("../../../../protocol/data/retail_actors_1_26_50.txt").lines() {
        let old = match actor {
            "minecraft:evocation_illager" => "minecraft:evoker",
            "minecraft:tropicalfish" => "minecraft:tropical_fish",
            "minecraft:villager_v2" => "minecraft:villager",
            "minecraft:zombie_villager_v2" => "minecraft:zombie_villager",
            other => other,
        };
        assert_eq!(rules().egg_identifier(actor), format!("{old}_spawn_egg"));
        assert_eq!(
            rules().superseding_egg(actor),
            match actor {
                "minecraft:villager" => Some("minecraft:villager_v2"),
                "minecraft:zombie_villager" => Some("minecraft:zombie_villager_v2"),
                _ => None,
            }
        );
    }
    for row in std::str::from_utf8(crate::item::retail::SOURCE_BYTES)
        .unwrap()
        .lines()
    {
        let (_, identifier) = row.split_once('\t').unwrap();
        let expected = match identifier {
            "minecraft:air" => Some(ItemVisualKind::EmptyHand),
            "minecraft:banner" => Some(ItemVisualKind::BlockModel),
            "minecraft:bed" | "minecraft:nether_wart" | "minecraft:straw_bed" => {
                Some(ItemVisualKind::Sprite)
            }
            _ => None,
        };
        assert_eq!(rules().item_visual(identifier), expected, "{identifier}");
    }
    assert_eq!(rules().empty_hand(), "minecraft:air");
}

#[test]
fn pack_rules_preserve_mineral_wire_and_texture_contracts() {
    let records = crate::read_registry_for_protocol(
        crate::pinned_block_registry_bytes(),
        crate::active_content_registry_protocol(),
    )
    .unwrap();
    let expected = [
        (
            "minecraft:cinnabar",
            0xbda0_2665,
            "cinnabar",
            "textures/blocks/cinnabar",
        ),
        (
            "minecraft:sulfur",
            0x2d65_8dd8,
            "sulfur",
            "textures/blocks/sulfur",
        ),
    ];
    assert_eq!(rules().minerals.len(), expected.len());
    for record in &records {
        let old = expected
            .iter()
            .find(|(name, _, _, _)| *name == record.name.as_ref());
        let rule = rules().mineral(&record.name);
        assert_eq!(rule.is_some(), old.is_some(), "{}", record.name);
        if let (Some(rule), Some((_, hash, key, path))) = (rule, old) {
            assert_eq!(rule.network_hash, *hash);
            assert_eq!(record.network_hash, *hash);
            assert_eq!(&rule.key, key);
            assert_eq!(&rule.path, path);
        }
    }
}

#[test]
fn pack_rules_preserve_typed_texture_and_block_requests() {
    for (slot, old) in [
        (PackTexture::PaleMossCarpetSide, "pale_moss_carpet_side"),
        (PackTexture::BambooFenceGate, "bamboo_fence_gate"),
        (PackTexture::BeaconCore, "beacon_core"),
        (PackTexture::BeaconShell, "beacon_shell"),
        (PackTexture::BeeNestSide, "bee_nest_side"),
        (PackTexture::BeeNestBottom, "bee_nest_bottom"),
        (PackTexture::BeeNestTop, "bee_nest_top"),
        (PackTexture::BeeNestFront, "bee_nest_front"),
        (PackTexture::BeehiveSide, "beehive_side"),
        (PackTexture::BeehiveTop, "beehive_top"),
        (PackTexture::BeehiveFront, "beehive_front"),
        (
            PackTexture::ChiseledBookshelfSide,
            "chiseled_bookshelf_side",
        ),
        (PackTexture::ChiseledBookshelfTop, "chiseled_bookshelf_top"),
        (
            PackTexture::ChiseledBookshelfFront,
            "chiseled_bookshelf_front",
        ),
        (PackTexture::CactusSide, "cactus_side"),
        (PackTexture::CactusBottom, "cactus_bottom"),
        (PackTexture::CactusTop, "cactus_top"),
        (PackTexture::CakeSide, "cake_side"),
        (PackTexture::CakeBottom, "cake_bottom"),
        (PackTexture::CakeTop, "cake_top"),
        (PackTexture::CakeWest, "cake_west"),
        (PackTexture::FarmlandSide, "farmland_side"),
        (PackTexture::Farmland, "farmland"),
        (PackTexture::GrassSide, "grass_side"),
        (PackTexture::MyceliumSide, "mycelium_side"),
        (PackTexture::ResinClump, "resin_clump"),
        (PackTexture::PowderSnow, "powder_snow"),
    ] {
        assert_eq!(slot.key(), old);
    }
    for (slot, old) in [
        (PackBlock::BeeNest, "minecraft:bee_nest"),
        (PackBlock::Beehive, "minecraft:beehive"),
        (PackBlock::ChiseledBookshelf, "minecraft:chiseled_bookshelf"),
        (PackBlock::Cactus, "minecraft:cactus"),
        (PackBlock::Cake, "minecraft:cake"),
        (PackBlock::Farmland, "minecraft:farmland"),
        (PackBlock::ResinClump, "minecraft:resin_clump"),
        (PackBlock::Dirt, "minecraft:dirt"),
        (PackBlock::CobblestoneWall, "minecraft:cobblestone_wall"),
        (PackBlock::Air, "minecraft:air"),
        (PackBlock::Stone, "minecraft:stone"),
        (PackBlock::HeavyCore, "minecraft:heavy_core"),
    ] {
        assert_eq!(slot.identifier(), old);
    }
}

#[test]
fn pack_rules_preserve_selector_alias_cube_sources() {
    let old = [
        (
            "minecraft:bone_block",
            AliasCubeSource::Pillar(
                ["bone_block_top", "bone_block_top", "bone_block_side"].map(String::from),
            ),
        ),
        (
            "minecraft:chiseled_quartz_block",
            AliasCubeSource::Pillar(
                [
                    "chiseled_quartz_block_top",
                    "chiseled_quartz_block_top",
                    "chiseled_quartz_block_side",
                ]
                .map(String::from),
            ),
        ),
        (
            "minecraft:hay_block",
            AliasCubeSource::Pillar(
                ["hayblock_top", "hayblock_top", "hayblock_side"].map(String::from),
            ),
        ),
        (
            "minecraft:quartz_block",
            AliasCubeSource::Pillar(
                [
                    "flattened_quartz_block_top",
                    "flattened_quartz_block_top",
                    "flattened_quartz_block_side",
                ]
                .map(String::from),
            ),
        ),
        (
            "minecraft:tnt",
            AliasCubeSource::Pillar(
                [
                    "flattened_tnt_bottom",
                    "flattened_tnt_top",
                    "flattened_tnt_side",
                ]
                .map(String::from),
            ),
        ),
        (
            "minecraft:purpur_block",
            AliasCubeSource::Scalar("flattened_purpur_block".into()),
        ),
        (
            "minecraft:smooth_quartz",
            AliasCubeSource::Scalar("smooth_quartz".into()),
        ),
    ];
    let names = block_names();
    for name in &names {
        assert_eq!(
            rules().alias_cube(name),
            old.iter()
                .find(|(old, _)| *old == name.as_str())
                .map(|(_, source)| source),
            "{name}"
        );
        let companion = (super::super::block_render_fact(name).kind
            == super::super::BlockRenderKind::CandleCake)
            .then(|| name.strip_suffix("_cake").unwrap());
        assert_eq!(candle_companion(name), companion);
        if let Some(companion) = companion {
            assert!(names.contains(companion));
        }
    }
    for (key, path) in [
        ("bone_block_top", "textures/blocks/bone_block_top"),
        ("bone_block_side", "textures/blocks/bone_block_side"),
        (
            "chiseled_quartz_block_top",
            "textures/blocks/quartz_block_chiseled_top",
        ),
        (
            "chiseled_quartz_block_side",
            "textures/blocks/quartz_block_chiseled",
        ),
        ("hayblock_top", "textures/blocks/hay_block_top"),
        ("hayblock_side", "textures/blocks/hay_block_side"),
        ("flattened_purpur_block", "textures/blocks/purpur_block"),
        (
            "flattened_quartz_block_top",
            "textures/blocks/quartz_block_top",
        ),
        (
            "flattened_quartz_block_side",
            "textures/blocks/quartz_block_side",
        ),
        ("smooth_quartz", "textures/blocks/quartz_block_bottom"),
        ("flattened_tnt_bottom", "textures/blocks/tnt_bottom"),
        ("flattened_tnt_side", "textures/blocks/tnt_side"),
        ("flattened_tnt_top", "textures/blocks/tnt_top"),
    ] {
        assert_eq!(rules().alias_path(key), Some(path));
    }
}

#[test]
fn existing_item_and_entity_facts_preserve_pack_special_cases() {
    let items = super::super::ItemFacts::builtin().unwrap();
    for tag in &items.payload().tags {
        let mut fact = super::super::ItemFact::default();
        items.apply_tags(&mut fact, [tag.name.as_ref()]);
        assert_eq!(
            fact.traits.contains(super::super::ItemTraits::SPEAR),
            tag.name.as_ref() == "minecraft:is_spear",
            "{}",
            tag.name
        );
    }
    for record in &super::super::entity::EntityFacts::pinned().records {
        assert_eq!(
            record.fact.native_class == super::super::entity::NativeEntityClass::EnderCrystal,
            record.identifier.as_ref() == "minecraft:ender_crystal"
        );
    }
}
