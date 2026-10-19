use super::*;
use assets::EquipmentCategory;

fn payloads(entries: &[(&str, &str)]) -> SourcePayloads {
    entries
        .iter()
        .map(|(path, bytes)| ((*path).into(), bytes.as_bytes().to_vec().into_boxed_slice()))
        .collect()
}

fn symbols(entries: &[(EntityAssetKind, &str, u32)]) -> Vec<EntityAssetSymbol> {
    entries
        .iter()
        .map(|(kind, identifier, source_index)| EntityAssetSymbol {
            mount_equipment: assets::entity::mount::MountEquipment::Unresolved,
            kind: *kind,
            identifier: (*identifier).into(),
            source_index: *source_index,
            dependencies: Box::new([]),
        })
        .collect()
}

fn sources(paths: &[&str]) -> Vec<EntityAssetSource> {
    paths
        .iter()
        .map(|path| EntityAssetSource {
            path: (*path).into(),
            source_bytes: 1,
            source_sha256: [0; 32],
        })
        .collect()
}

const TRIDENT: &str = r#"{"format_version":"1.10","minecraft:attachable":{"description":{
    "identifier":"minecraft:trident",
    "materials":{"default":"entity_alphatest"},
    "textures":{"default":"textures/entity/trident"},
    "geometry":{"default":"geometry.trident"},
    "animations":{"wield_first_person":"animation.trident.wield_first_person","wield_third_person":"animation.trident.wield_third_person"},
    "render_controllers":["controller.render.item_default"]}}}"#;

const TRIDENT_ANIM: &str = r#"{"format_version":"1.10.0","animations":{
    "animation.trident.wield_first_person":{"loop":true,"bones":{"pole":{"position":[-7.0,-3.0,-2.0],"rotation":[152.0,-9.0,25.0]}}},
    "animation.trident.wield_third_person":{"loop":true,"bones":{"pole":{"position":[1.5,-2.5,-10.5],"rotation":[97.0,-1.5,-49.0]}}}}}"#;

const HELMET_MOB: &str = r#"{"format_version":"1.8.0","minecraft:attachable":{"description":{
    "identifier":"minecraft:diamond_helmet",
    "materials":{"default":"armor"},
    "textures":{"default":"textures/models/armor/diamond_1"},
    "geometry":{"default":"geometry.humanoid.armor.helmet"},
    "render_controllers":["controller.render.armor"]}}}"#;

const HELMET_PLAYER: &str = r#"{"format_version":"1.10.0","minecraft:attachable":{"description":{
    "identifier":"minecraft:diamond_helmet.player",
    "item":{"minecraft:diamond_helmet":"query.owner_identifier == 'minecraft:player'"},
    "materials":{"default":"armor"},
    "textures":{"default":"textures/models/armor/diamond_1"},
    "geometry":{"default":"geometry.player.armor.helmet"},
    "animations":{"offset":"animation.armor.helmet.offset"},
    "render_controllers":["controller.render.armor"]}}}"#;

#[test]
fn trident_carries_literal_first_and_third_person_transforms() {
    let payloads = payloads(&[
        ("attachables/trident.entity.json", TRIDENT),
        ("animations/trident.animation.json", TRIDENT_ANIM),
    ]);
    let symbols = symbols(&[
        (EntityAssetKind::Geometry, "geometry.trident", 1),
        (EntityAssetKind::Texture, "textures/entity/trident", 2),
        (
            EntityAssetKind::Animation,
            "animation.trident.wield_first_person",
            0,
        ),
        (
            EntityAssetKind::Animation,
            "animation.trident.wield_third_person",
            0,
        ),
    ]);
    let sources = sources(&["animations/trident.animation.json"]);
    let bindings =
        compile_bindings(&payloads, &symbols, &sources, &BTreeMap::new(), false).unwrap();
    assert_eq!(bindings.len(), 1);
    let trident = &bindings[0];
    assert_eq!(trident.category, EquipmentCategory::Held);
    assert_eq!(
        trident.geometry.resolution,
        EntityDependencyResolution::Catalog
    );
    assert_eq!(
        trident.texture.resolution,
        EntityDependencyResolution::Catalog
    );
    let first = trident.first_person.literal().unwrap();
    assert_eq!(first.translation[0].get(), -7.0);
    assert_eq!(first.rotation[2].get(), 25.0);
    assert_eq!(first.scale[1].get(), 1.0);
    let third = trident.third_person.literal().unwrap();
    assert_eq!(third.translation[2].get(), -10.5);
    assert!(trident.dropped.literal().is_none());
}

#[test]
fn owner_armor_variants_are_retained_with_the_player_geometry_in_catalog() {
    let payloads = payloads(&[
        ("attachables/diamond_helmet.json", HELMET_MOB),
        ("attachables/diamond_helmet.player.json", HELMET_PLAYER),
    ]);
    // Only the player geometry is collected; the mob geometry is external.
    let symbols = symbols(&[(EntityAssetKind::Geometry, "geometry.player.armor.helmet", 0)]);
    let bindings = compile_bindings(&payloads, &symbols, &[], &BTreeMap::new(), false).unwrap();
    assert_eq!(bindings.len(), 2);
    let helmet = &bindings[1];
    assert_eq!(helmet.identifier.as_ref(), "minecraft:diamond_helmet");
    assert_eq!(
        helmet.geometry.identifier.as_ref(),
        "geometry.player.armor.helmet"
    );
    assert_eq!(
        helmet.geometry.resolution,
        EntityDependencyResolution::Catalog
    );
    assert_eq!(
        helmet.category,
        EquipmentCategory::Armor {
            slot: ArmorSlot::Helmet
        }
    );
    // Armor offsets are Molang locator queries, never literal.
    assert!(helmet.first_person.literal().is_none());
    assert!(helmet.third_person.literal().is_none());
}

#[test]
fn molang_bone_keyframes_stay_needs_measurement() {
    const SHIELD: &str = r#"{"format_version":"1.10.0","minecraft:attachable":{"description":{
        "identifier":"minecraft:shield",
        "materials":{"default":"entity_alphatest"},
        "textures":{"default":"textures/entity/shield"},
        "geometry":{"default":"geometry.shield"},
        "animations":{"wield_third_person":"animation.shield.wield_third_person"},
        "render_controllers":["controller.render.item_default"]}}}"#;
    const SHIELD_ANIM: &str = r#"{"format_version":"1.10.0","animations":{
        "animation.shield.wield_third_person":{"bones":{"shield":{"position":["c.item_slot == 'main_hand' ? -0.4 : -1.6",9.0,9.3],"rotation":[-90.0,0.0,90.0]}}}}}"#;
    let payloads = payloads(&[
        ("attachables/shield.entity.json", SHIELD),
        ("animations/shield.animation.json", SHIELD_ANIM),
    ]);
    let symbols = symbols(&[
        (EntityAssetKind::Geometry, "geometry.shield", 0),
        (
            EntityAssetKind::Animation,
            "animation.shield.wield_third_person",
            0,
        ),
    ]);
    let sources = sources(&["animations/shield.animation.json"]);
    let bindings =
        compile_bindings(&payloads, &symbols, &sources, &BTreeMap::new(), false).unwrap();
    assert_eq!(bindings[0].category, EquipmentCategory::Shield);
    assert!(bindings[0].third_person.literal().is_none());
}

#[test]
fn elytra_keeps_only_literal_poses_sorted_by_key() {
    const ELYTRA: &str = r#"{"format_version":"1.10.0","minecraft:attachable":{"description":{
        "identifier":"minecraft:elytra",
        "materials":{"default":"elytra"},
        "textures":{"default":"textures/models/armor/elytra"},
        "geometry":{"default":"geometry.elytra"},
        "animations":{"sneaking":"animation.elytra.sneaking","default":"animation.elytra.default","gliding":"animation.elytra.gliding","default_controller":"controller.animation.elytra.default"},
        "render_controllers":["controller.render.armor"]}}}"#;
    const ELYTRA_ANIM: &str = r#"{"format_version":"1.8.0","animations":{
        "animation.elytra.default":{"bones":{"body":{"scale":1.067},"left_wing":{"position":[4.5,4,-2],"rotation":[15,0,-13],"scale":[1,1,2]}}},
        "animation.elytra.sneaking":{"bones":{"left_wing":{"position":[2,3,-2],"rotation":[20,-15,-45]}}},
        "animation.elytra.gliding":{"bones":{"left_wing":{"rotation":["math.lerp(15, 20, 1)",0,0]}}}}}"#;
    let payloads = payloads(&[
        ("attachables/elytra.json", ELYTRA),
        ("animations/elytra.animation.json", ELYTRA_ANIM),
    ]);
    let symbols = symbols(&[
        (EntityAssetKind::Geometry, "geometry.elytra", 1),
        (EntityAssetKind::Texture, "textures/models/armor/elytra", 2),
        (EntityAssetKind::Animation, "animation.elytra.default", 0),
        (EntityAssetKind::Animation, "animation.elytra.sneaking", 0),
        (EntityAssetKind::Animation, "animation.elytra.gliding", 0),
    ]);
    let sources = sources(&["animations/elytra.animation.json"]);
    let bindings =
        compile_bindings(&payloads, &symbols, &sources, &BTreeMap::new(), false).unwrap();
    assert_eq!(bindings[0].category, EquipmentCategory::Elytra);
    let keys = bindings[0]
        .poses
        .iter()
        .map(|pose| pose.key.as_ref())
        .collect::<Vec<_>>();
    assert_eq!(keys, ["default", "sneaking"]);
    let default = bindings[0].pose("default").unwrap();
    let body = default
        .bones
        .iter()
        .find(|bone| bone.bone.as_ref() == "body")
        .unwrap();
    assert_eq!(body.scale.unwrap().map(|value| value.get()), [1.067; 3]);
    assert!(body.translation.is_none());
}

#[test]
fn shield_third_person_resolves_the_hand_conditional_into_one_pose_per_hand() {
    const SHIELD: &str = r#"{"format_version":"1.10.0","minecraft:attachable":{"description":{
        "identifier":"minecraft:shield",
        "materials":{"default":"entity_alphatest"},
        "textures":{"default":"textures/entity/shield"},
        "geometry":{"default":"geometry.shield"},
        "animations":{"wield_third_person":"animation.shield.wield_third_person","wield_main_hand_first_person":"animation.shield.wield_main_hand_first_person"},
        "render_controllers":["controller.render.item_default"]}}}"#;
    const SHIELD_ANIM: &str = r#"{"format_version":"1.10.0","animations":{
        "animation.shield.wield_third_person":{"bones":{"shield":{"position":["c.item_slot == 'main_hand' ? -0.4 : -1.6",9.0,9.3],"rotation":[-90.0,0.0,90.0],"scale":["c.item_slot == 'main_hand' ? 1.0 : -1.0",-1.0,"c.item_slot == 'main_hand' ? -1.0 : 1.0"]}}},
        "animation.shield.wield_main_hand_first_person":{"bones":{"shield":{"position":["variable.x",0,0]}}}}}"#;
    let payloads = payloads(&[
        ("attachables/shield.entity.json", SHIELD),
        ("animations/shield.animation.json", SHIELD_ANIM),
    ]);
    let symbols = symbols(&[
        (EntityAssetKind::Geometry, "geometry.shield", 1),
        (EntityAssetKind::Texture, "textures/entity/shield", 2),
        (
            EntityAssetKind::Animation,
            "animation.shield.wield_third_person",
            0,
        ),
        (
            EntityAssetKind::Animation,
            "animation.shield.wield_main_hand_first_person",
            0,
        ),
    ]);
    let sources = sources(&["animations/shield.animation.json"]);
    let bindings =
        compile_bindings(&payloads, &symbols, &sources, &BTreeMap::new(), false).unwrap();
    let shield = &bindings[0];
    assert_eq!(shield.category, EquipmentCategory::Shield);
    assert_eq!(shield.poses.len(), 2);
    let main = shield.pose("wield_third_person@main_hand").unwrap();
    let off = shield.pose("wield_third_person@off_hand").unwrap();
    let translation = |pose: &AttachablePose| pose.bones[0].translation.unwrap().map(|v| v.get());
    assert_eq!(translation(main), [-0.4, 9.0, 9.3]);
    assert_eq!(translation(off), [-1.6, 9.0, 9.3]);
    assert_eq!(
        off.bones[0].scale.unwrap().map(|v| v.get()),
        [-1.0, -1.0, 1.0]
    );
}

#[test]
fn item_use_durations_read_seconds_and_ticks_and_skip_the_rest() {
    let root = tempfile::tempdir().unwrap();
    let items = root.path().join("items");
    std::fs::create_dir(&items).unwrap();
    let write = |name: &str, body: &str| std::fs::write(items.join(name), body).unwrap();
    write(
        "honey.json",
        r#"{"minecraft:item":{"description":{"identifier":"minecraft:honey_bottle"},
            "components":{"minecraft:use_modifiers":{"use_duration":1.6}}}}"#,
    );
    write(
        "spear.json",
        r#"{"minecraft:item":{"description":{"identifier":"minecraft:iron_spear"},
            "components":{"minecraft:use_duration":72000}}}"#,
    );
    write(
        "seed.json",
        r#"{"minecraft:item":{"description":{"identifier":"minecraft:seeds"},"components":{}}}"#,
    );
    write(
        "fractional.json",
        r#"{"minecraft:item":{"description":{"identifier":"test:fractional"},
            "components":{"minecraft:use_modifiers":{"use_duration":1.049}}}}"#,
    );
    write(
        "precision.json",
        r#"{"minecraft:item":{"description":{"identifier":"test:precision"},
            "components":{"minecraft:use_modifiers":{"use_duration":1.04999998}}}}"#,
    );
    for (name, duration) in [("sub_tick", 0.049), ("negative", -1.0), ("overflow", 1e40)] {
        write(
            &format!("{name}.json"),
            &serde_json::json!({"minecraft:item": {
                "description": {"identifier": format!("test:{name}")},
                "components": {"minecraft:use_modifiers": {"use_duration": duration}}
            }})
            .to_string(),
        );
    }
    write(
        "unsupported_modern.json",
        r#"{"minecraft:item":{"description":{"identifier":"test:unsupported_modern"},
            "components":{"minecraft:use_modifiers":{"use_duration":-1},
                "minecraft:use_duration":32}}}"#,
    );
    write("broken.json", "{");
    let durations = compile_item_use(root.path()).unwrap();
    let pairs = durations
        .iter()
        .map(|entry| (entry.identifier.as_ref(), entry.ticks))
        .collect::<Vec<_>>();
    assert_eq!(
        pairs,
        [
            ("minecraft:honey_bottle", 32),
            ("minecraft:iron_spear", 72000),
            ("test:fractional", 20),
            ("test:precision", 21)
        ]
    );
}

/// Comments in a food's unrelated effects must not hide its use duration.
#[test]
fn item_use_durations_accept_commented_food_definitions() {
    let root = tempfile::tempdir().unwrap();
    let items = root.path().join("items");
    std::fs::create_dir(&items).unwrap();
    std::fs::write(
        items.join("golden_apple.json"),
        r#"{"minecraft:item":{
            "description":{"identifier":"minecraft:golden_apple"},
            "components":{
                "minecraft:use_duration":32,
                "minecraft:food":{
                    "can_always_eat":true, /* Usable at full hunger. */
                    "effects":[{"duration":120 // Seconds.
                    }]
                }
            }
        }}"#,
    )
    .unwrap();
    let durations = compile_item_use(root.path()).unwrap();
    assert_eq!(durations.len(), 1);
    assert_eq!(durations[0].identifier.as_ref(), "minecraft:golden_apple");
    assert_eq!(durations[0].ticks, 32);
}

#[test]
fn equipment_dye_follows_authored_material_defines_instead_of_its_name() {
    let document = serde_json::json!({"materials": {
        "cloth_tint:entity": {"+defines": ["USE_COLOR_MASK"]},
        "renamed:cloth_tint": {},
        "leather_decoy:entity": {},
        "removed:cloth_tint": {"-defines": ["USE_COLOR_MASK"]},
        "replaced:cloth_tint": {"defines": []},
        "unknown:absent": {},
        "cycle_a:cycle_b": {},
        "cycle_b:cycle_a": {},
        "malformed:entity": {"+defines": "USE_COLOR_MASK"}
    }});
    let material_text = document.to_string();
    let mut payloads = payloads(&[("materials/test.material", &material_text)]);
    for declaration in document["materials"].as_object().unwrap().keys() {
        let material = declaration.split(':').next().unwrap();
        let attachable = serde_json::json!({"format_version": "1.10.0", "minecraft:attachable": {"description": {
            "identifier": format!("custom:{material}"),
            "materials": {"default": material},
            "textures": {"default": "textures/test"},
            "geometry": {"default": "geometry.test"},
            "render_controllers": ["controller.render.test"]
        }}});
        payloads.insert(
            format!("attachables/{material}.json").into(),
            attachable.to_string().into_bytes().into_boxed_slice(),
        );
    }
    let bindings = compile_bindings(
        &payloads,
        &[],
        &sources(&["materials/test.material"]),
        &BTreeMap::new(),
        false,
    )
    .unwrap();
    let dye = Some(0x00b0_2e26);
    let inventory = payloads
        .iter()
        .filter(|(path, _)| path.starts_with("attachables/"))
        .map(|(_, bytes)| {
            let source: Value = serde_json::from_slice(bytes).unwrap();
            source["minecraft:attachable"]["description"]["identifier"]
                .as_str()
                .unwrap()
                .to_owned()
        })
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        bindings
            .iter()
            .map(|binding| binding.identifier.to_string())
            .collect::<std::collections::BTreeSet<_>>(),
        inventory,
    );
    for binding in &bindings {
        let expected = if matches!(
            binding.identifier.as_ref(),
            "custom:renamed" | "custom:cloth_tint"
        ) {
            dye
        } else {
            None
        };
        assert_eq!(
            binding.color_mask_rgb(dye),
            expected,
            "{}",
            binding.identifier
        );
        if matches!(
            binding.identifier.as_ref(),
            "custom:unknown" | "custom:cycle_a" | "custom:cycle_b" | "custom:malformed"
        ) {
            assert_eq!(binding.color_mask, assets::EquipmentColorMask::Unresolved);
        }
    }
}

#[test]
fn equipment_category_uses_the_admitted_slot_with_neutral_geometry() {
    let facts = assets::facts::ItemFacts::builtin().unwrap();
    for record in &facts.payload().records {
        let Some(slot) = record.fact.armour_slot else {
            continue;
        };
        if record
            .fact
            .traits
            .contains(assets::facts::ItemTraits::GLIDER)
        {
            continue;
        }
        use assets::item::armour::ItemArmourSlot as S;
        let expected = match slot {
            S::Head => ArmorSlot::Helmet,
            S::Chest => ArmorSlot::Chestplate,
            S::Legs => ArmorSlot::Leggings,
            S::Feet => ArmorSlot::Boots,
        };
        assert_eq!(
            category(&record.identifier, "geometry.trident"),
            EquipmentCategory::Armor { slot: expected }
        );
    }
}

#[test]
fn merged_animation_document_is_parsed_once_for_many_attachables() {
    let mut payloads = payloads(&[("animations/_pack.json", TRIDENT_ANIM)]);
    for index in 0..65 {
        payloads.insert(
            format!("attachables/item_{index}.json").into(),
            TRIDENT
                .replace("minecraft:trident", &format!("fixture:item_{index}"))
                .into_bytes()
                .into(),
        );
    }
    let symbols = symbols(&[
        (
            EntityAssetKind::Animation,
            "animation.trident.wield_first_person",
            0,
        ),
        (
            EntityAssetKind::Animation,
            "animation.trident.wield_third_person",
            0,
        ),
    ]);
    let sources = sources(&["animations/_pack.json"]);
    ANIMATION_PARSES.set(0);
    let bindings =
        compile_bindings(&payloads, &symbols, &sources, &BTreeMap::new(), false).unwrap();
    assert_eq!(bindings.len(), 65);
    assert!(
        bindings
            .iter()
            .all(|binding| binding.first_person.literal().is_some()
                && binding.third_person.literal().is_some())
    );
    assert_eq!(ANIMATION_PARSES.get(), 1);
}

#[test]
fn unreferenced_animation_documents_are_not_parsed_for_bindings() {
    let payloads = payloads(&[
        ("attachables/helmet.json", HELMET_MOB),
        ("animations/unused.json", TRIDENT_ANIM),
    ]);
    let symbols = symbols(&[(
        EntityAssetKind::Animation,
        "animation.trident.wield_first_person",
        0,
    )]);
    let sources = sources(&["animations/unused.json"]);
    ANIMATION_PARSES.set(0);
    let bindings =
        compile_bindings(&payloads, &symbols, &sources, &BTreeMap::new(), false).unwrap();
    assert_eq!(bindings.len(), 1);
    assert_eq!(ANIMATION_PARSES.get(), 0);
}
