use super::*;

/// Compiles original shared pixels and records the destinations from real material descriptors.
fn compile_keys(
    pack: &PackSources,
    descriptors: BTreeMap<Descriptor, Box<str>>,
) -> (MaterialKeys, BTreeMap<Descriptor, u32>) {
    use crate::animation::{AnimationLimits, DecodedImage, compile_animation_plan};
    let paths = descriptors
        .keys()
        .map(|descriptor| descriptor.path.clone())
        .collect::<BTreeSet<_>>();
    let images = paths
        .into_iter()
        .map(|source_path| DecodedImage {
            source_path,
            width: assets::TILE_SIZE,
            height: assets::TILE_SIZE,
            rgba8: [31, 47, 59, 255]
                .repeat((assets::TILE_SIZE * assets::TILE_SIZE) as usize)
                .into_boxed_slice(),
        })
        .collect::<Vec<_>>();
    let plan = compile_animation_plan(
        pack,
        &images,
        AnimationLimits {
            max_layers_per_page: 32,
            max_pages: 1,
        },
    )
    .unwrap();
    let (materials, ids) =
        super::super::compile_materials(&descriptors, &plan, &BTreeSet::new()).unwrap();
    let keys = build(pack, &ids, &materials, &[]);
    assert_eq!(
        MaterialKeys::from_json(&keys.to_json(materials.len() as u32), materials.len()),
        Some(keys.clone())
    );
    (keys, ids)
}

#[test]
fn honey_fronts_and_cake_inner_faces_keep_their_compiled_atlas_indices() {
    use serde_json::json;
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("textures")).unwrap();
    let blocks = json!({
        "bee_nest":{"textures":{"west":"bee_nest_side","east":"bee_nest_side","down":"bee_nest_bottom","up":"bee_nest_top","north":"bee_nest_side","south":"bee_nest_front"}},
        "beehive":{"textures":{"west":"beehive_side","east":"beehive_side","down":"beehive_top","up":"beehive_top","north":"beehive_side","south":"beehive_front"}},
        "cake":{"textures":{"west":"cake_west","east":"cake_side","down":"cake_bottom","up":"cake_top","north":"cake_side","south":"cake_side"}}
    });
    let mut data = serde_json::Map::new();
    for key in [
        "bee_nest_bottom",
        "bee_nest_side",
        "bee_nest_top",
        "beehive_side",
        "beehive_top",
    ] {
        data.insert(
            key.into(),
            json!({"textures":[format!("textures/blocks/{key}")]}),
        );
    }
    for key in ["bee_nest_front", "beehive_front"] {
        data.insert(key.into(), json!({"textures":[format!("textures/blocks/{key}"),format!("textures/blocks/{key}_honey")]}));
    }
    for key in ["cake_bottom", "cake_side", "cake_top"] {
        let path = format!("textures/blocks/{key}");
        data.insert(key.into(), json!({"textures":[path,path]}));
    }
    data.insert(
        "cake_west".into(),
        json!({"textures":["textures/blocks/cake_side","textures/blocks/cake_inner"]}),
    );
    std::fs::write(root.path().join("blocks.json"), blocks.to_string()).unwrap();
    std::fs::write(
        root.path().join("textures/terrain_texture.json"),
        json!({"texture_data":data}).to_string(),
    )
    .unwrap();
    std::fs::write(root.path().join("textures/flipbook_textures.json"), "[]").unwrap();
    let pack = pack_compile_input::read_pack(root.path()).unwrap();
    let descriptors = super::super::visuals::bee_housing::bee_housing_material_descriptors(&pack)
        .unwrap()
        .into_iter()
        .chain(super::super::visuals::cake::cake_material_descriptors(&pack).unwrap())
        .collect();
    let (keys, ids) = compile_keys(&pack, descriptors);
    for key in ["bee_nest_front", "beehive_front"] {
        let pair = pack.terrain.get_exact_pair_plain(key).unwrap();
        let destinations = keys.materials(key);
        assert_eq!(destinations.len(), pair.len());
        for (index, path) in pair.into_iter().enumerate() {
            let (descriptor, &id) = ids
                .iter()
                .find(|(descriptor, _)| {
                    descriptor.texture_key.as_ref() == key && descriptor.path.as_ref() == path
                })
                .unwrap();
            assert_eq!(descriptor.state_variant, index as u32);
            assert_eq!(keys.flipbook_selector(id), (index as u32, 0));
            assert!(destinations.contains(&id));
            assert_eq!(
                destinations
                    .iter()
                    .filter(|&&candidate| keys.flipbook_selector(candidate) == (index as u32, 0))
                    .count(),
                1
            );
        }
    }
    let cake = keys.materials("cake_west");
    assert_eq!(cake.len(), 1);
    assert_eq!(keys.flipbook_selector(cake[0]), (1, 0));
}

#[test]
fn snowy_grass_keeps_the_source_catalog_key_and_clamped_index() {
    use serde_json::json;
    for native in [true, false] {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("textures")).unwrap();
        let ordinary = json!({"path":"textures/grass","overlay_color":"#ffffff"});
        let snow = assets::SNOWED_GRASS_SIDE_TEXTURE;
        let grass = if native {
            json!([ordinary, snow])
        } else {
            ordinary
        };
        let terrain = json!({"texture_data":{
            "grass_side":{"textures":grass},
            "mycelium_side":{"textures":["textures/soil",snow]}
        }});
        std::fs::write(root.path().join("blocks.json"), "{}").unwrap();
        std::fs::write(
            root.path().join("textures/terrain_texture.json"),
            terrain.to_string(),
        )
        .unwrap();
        std::fs::write(root.path().join("textures/flipbook_textures.json"), "[]").unwrap();
        let pack = pack_compile_input::read_pack(root.path()).unwrap();
        let (descriptor, key) =
            super::super::visuals::snowy_grass::material_descriptor(&pack).unwrap();
        let expected_key = if native {
            "grass_side"
        } else {
            "mycelium_side"
        };
        assert_eq!(key.as_ref(), expected_key);
        let (keys, ids) = compile_keys(&pack, BTreeMap::from([(descriptor.clone(), key)]));
        let id = ids[&descriptor];
        assert_eq!(keys.materials(expected_key), [id]);
        assert_eq!(keys.flipbook_selector(id), (1, 0));
    }
}

/// Reads a small synthetic source pack without compiling any image pixels.
fn pack() -> PackSources {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("textures")).unwrap();
    for (path, bytes) in [
        ("blocks.json", "{}"),
        (
            "textures/terrain_texture.json",
            r#"{"texture_data":{"water":{"textures":"textures/water"}}}"#,
        ),
        (
            "textures/flipbook_textures.json",
            r#"[{"atlas_tile":"water","flipbook_texture":"textures/water","atlas_index":1,"atlas_tile_variant":2,"replicate":2}]"#,
        ),
    ] {
        std::fs::write(root.path().join(path), bytes).unwrap();
    }
    pack_compile_input::read_pack(root.path()).unwrap()
}

#[test]
fn compiled_animation_destinations_survive_generated_material_copies() {
    let pack = pack();
    let descriptors = BTreeMap::from([(
        Descriptor {
            path: "textures/water".into(),
            texture_key: "water".into(),
            flags: 0,
            state_variant: 0,
            flipbook_selector: None,
        },
        1,
    )]);
    let materials = [Material {
        texture: TextureRef::DIAGNOSTIC,
        flags: 0,
        animation: 0,
        ..Material::unvaried()
    }; 2];
    let animations = [Animation {
        frame_start: 0,
        frame_count: 2,
        ticks_per_frame: 1,
        atlas_index: 1,
        atlas_tile_variant: 2,
        replicate: 2,
        flags: 0,
    }];
    let keys = build(&pack, &descriptors, &materials, &animations);
    assert_eq!(keys.flipbook_selector(1), (1, 2));
    let keys = extend(keys, &[], Vec::<(Box<str>, [u8; 3])>::new(), &[(2, 1)]);
    assert_eq!(keys.materials("water"), [1, 2]);
    assert_eq!(keys.flipbook_selector(2), (1, 2));
    assert_eq!(keys.flipbooks(), pack.flipbooks.as_ref());
}

#[test]
fn unanimated_state_destinations_remain_available_to_new_pack_descriptions() {
    let pack = pack();
    let descriptors = BTreeMap::from([(
        Descriptor {
            path: "textures/water".into(),
            texture_key: "water".into(),
            flags: 0,
            state_variant: 2,
            flipbook_selector: None,
        },
        1,
    )]);
    let materials = [Material {
        texture: TextureRef::DIAGNOSTIC,
        flags: 0,
        animation: NO_ANIMATION,
        ..Material::unvaried()
    }; 2];
    let keys = build(&pack, &descriptors, &materials, &[]);
    assert_eq!(keys.flipbook_selector(1), (2, 0));
}

#[test]
fn weighted_uv_leaves_and_the_representative_selector_are_recorded() {
    use crate::animation::{AnimationLimits, DecodedImage, compile_animation_plan};
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("textures")).unwrap();
    for (path, bytes) in [
        ("blocks.json", "{}"),
        (
            "textures/terrain_texture.json",
            r#"{"texture_data":{"tile":{"textures":["textures/shared",{"variations":[
            {"path":"textures/shared","weight":1},{"path":"textures/shared","weight":3}
        ]}]}}}"#,
        ),
        ("textures/flipbook_textures.json", "[]"),
    ] {
        std::fs::write(root.path().join(path), bytes).unwrap();
    }
    let pack = pack_compile_input::read_pack(root.path()).unwrap();
    let descriptor = Descriptor {
        path: "textures/shared".into(),
        texture_key: "tile".into(),
        flags: 0,
        state_variant: 1,
        flipbook_selector: None,
    };
    let mut descriptors = BTreeMap::from([(descriptor.clone(), "tile".into())]);
    let groups = super::super::variations::expand(&pack, &mut descriptors);
    let image = DecodedImage {
        source_path: "textures/shared".into(),
        width: assets::TILE_SIZE,
        height: assets::TILE_SIZE,
        rgba8: [31, 47, 59, 255]
            .into_iter()
            .cycle()
            .take((assets::TILE_SIZE * assets::TILE_SIZE * 4) as usize)
            .collect(),
    };
    let plan = compile_animation_plan(
        &pack,
        &[image],
        AnimationLimits {
            max_layers_per_page: 4,
            max_pages: 1,
        },
    )
    .unwrap();
    let (materials, ids) =
        super::super::compile_materials(&descriptors, &plan, &BTreeSet::new()).unwrap();
    let (materials, ids) = super::super::variations::install(materials, ids, groups).unwrap();
    let selector_id = ids[&descriptor];
    let selector = materials[selector_id as usize];
    assert_eq!(selector.variation_count, 2);
    let first = selector.variation_start;
    let second = first + 1;
    let keys = build(&pack, &ids, &materials, &[]);
    assert_eq!(keys.materials("tile"), [first, second, selector_id]);
    assert_eq!(keys.flipbook_selector(selector_id), (1, 0));
    assert_eq!(keys.flipbook_selector(first), (1, 0));
    assert_eq!(keys.flipbook_selector(second), (1, 1));
    assert_eq!(
        materials[first as usize].texture,
        materials[second as usize].texture
    );
    assert_eq!(
        f32::from_bits(materials[first as usize].variation_weight),
        0.25
    );
    assert_eq!(
        f32::from_bits(materials[second as usize].variation_weight),
        0.75
    );
    assert_eq!(materials[selector_id as usize], selector);
    assert_eq!(
        MaterialKeys::from_json(&keys.to_json(materials.len() as u32), materials.len()),
        Some(keys)
    );
}

#[test]
fn matching_pixels_share_storage_without_collapsing_material_destinations() {
    use crate::animation::{AnimationLimits, DecodedImage, compile_animation_plan};
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("textures")).unwrap();
    for (path, bytes) in [
        ("blocks.json", "{}"),
        (
            "textures/terrain_texture.json",
            r#"{"texture_data":{
            "water":{"textures":["textures/shared", "textures/shared"]},
            "alias":{"textures":"textures/shared"}
        }}"#,
        ),
        ("textures/flipbook_textures.json", "[]"),
    ] {
        std::fs::write(root.path().join(path), bytes).unwrap();
    }
    let pack = pack_compile_input::read_pack(root.path()).unwrap();
    let image = DecodedImage {
        source_path: "textures/shared".into(),
        width: assets::TILE_SIZE,
        height: assets::TILE_SIZE,
        rgba8: [31, 47, 59, 255]
            .into_iter()
            .cycle()
            .take((assets::TILE_SIZE * assets::TILE_SIZE * 4) as usize)
            .collect(),
    };
    let plan = compile_animation_plan(
        &pack,
        &[image],
        AnimationLimits {
            max_layers_per_page: 4,
            max_pages: 1,
        },
    )
    .unwrap();
    let first = Descriptor {
        path: "textures/shared".into(),
        texture_key: "water".into(),
        flags: 0,
        state_variant: 0,
        flipbook_selector: None,
    };
    let second = Descriptor {
        state_variant: 1,
        ..first.clone()
    };
    let same_destination = Descriptor {
        flipbook_selector: Some((0, 0)),
        ..first.clone()
    };
    let alias = Descriptor {
        texture_key: "alias".into(),
        ..first.clone()
    };
    let descriptors = BTreeMap::from([
        (first.clone(), "water".into()),
        (second.clone(), "water".into()),
        (same_destination.clone(), "water".into()),
        (alias.clone(), "alias".into()),
    ]);
    let (materials, ids) =
        super::super::compile_materials(&descriptors, &plan, &BTreeSet::new()).unwrap();
    let (first_id, second_id, alias_id) = (ids[&first], ids[&second], ids[&alias]);
    assert_ne!(first_id, second_id);
    assert_ne!(first_id, alias_id);
    assert_eq!(first_id, ids[&same_destination]);
    assert_eq!(
        materials[first_id as usize].texture,
        materials[second_id as usize].texture
    );
    assert_eq!(
        materials[first_id as usize].texture,
        materials[alias_id as usize].texture
    );
    let keys = build(&pack, &ids, &materials, &[]);
    assert_eq!(keys.materials("water").len(), 2);
    assert_eq!(keys.flipbook_selector(first_id), (0, 0));
    assert_eq!(keys.flipbook_selector(second_id), (1, 0));
    assert_eq!(keys.materials("alias"), [alias_id]);
}
