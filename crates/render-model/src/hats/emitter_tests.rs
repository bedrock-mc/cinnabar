use super::*;

/// An attachment bone with a rotated child whose cube corner sits at `tip + offset`.
fn geometry(offset: [f32; 3]) -> String {
    let tip = [2.0, 28.0, 1.0];
    let corner: Vec<f32> = (0..3).map(|axis| tip[axis] + offset[axis]).collect();
    serde_json::json!({
        "format_version": "1.12.0",
        "minecraft:geometry": [{
            "description": {"identifier": "geometry.cinnabar.crown", "texture_width": 64, "texture_height": 32},
            "bones": [
                {"name": "head", "pivot": [0, 24, 0], "cubes": [{"origin": [-1, 24, -1], "size": [2, 2, 2], "uv": [0, 0]}]},
                {"name": "tip", "parent": "head", "pivot": tip, "rotation": [10, 20, 30],
                 "cubes": [{"origin": corner, "size": [1, 1, 1], "uv": [0, 0]}]}
            ]
        }]
    })
    .to_string()
}

/// An animation file declaring the given particle entries.
fn animation(particles: serde_json::Value) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({ "particles": particles })).unwrap()
}

/// A particle entry using the fixture effect.
fn entry(bone: &str, offset: [f32; 3]) -> serde_json::Value {
    serde_json::json!({
        "bone": bone,
        "offset": offset,
        "effect": serde_json::from_str::<serde_json::Value>(fixture::EMITTER_EFFECT).unwrap(),
    })
}

/// Builds the test rig with its animation asset digest, without installing it.
fn build(geometry: &str, animation: &[u8]) -> Option<HatRig> {
    let mut descriptor = fixture::crown().descriptor().clone();
    descriptor.assets.animation = Some(fixture::animation_asset(animation));
    model::build(
        &descriptor,
        geometry,
        include_bytes!("crown.png"),
        animation,
    )
}

/// Maps a rig-frame vertex through its bone exactly as the actor vertex path does.
fn rendered(
    rig: &HatRig,
    poses: &[RenderBoneTransform],
    vertex: &crate::ActorRigVertex,
    world: [[f32; 4]; 3],
) -> Vec3 {
    let bone = vertex.bone_index as usize;
    let pose = poses[bone];
    let model = Vec3::from_slice(&pose.translation_scale[..3])
        + Quat::from_array(pose.rotation)
            * ((Vec3::from_array(vertex.position)
                - Vec3::from_array(rig.geometry.bone_pivots[bone]))
                * pose.translation_scale[3]);
    Vec3::from_array(world.map(|row| Vec3::from_slice(&row[..3]).dot(model) + row[3]))
}

/// The authored corner `tip + offset` in the rig frame, where authored X is mirrored.
fn corner(rig: &HatRig, offset: [f32; 3]) -> &crate::ActorRigVertex {
    let target = Vec3::new(-(2.0 + offset[0]), 28.0 + offset[1], 1.0 + offset[2]) / 16.0;
    rig.geometry
        .vertices
        .iter()
        .find(|vertex| {
            vertex.bone_index == 1 && Vec3::from_array(vertex.position).distance(target) < 1e-6
        })
        .expect("authored corner vertex")
}

#[test]
fn an_emitter_lands_on_the_rendered_point_of_its_authored_bone_offset() {
    let preview = [
        [-1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, -1.0, 0.0],
    ];
    for offset in [[0.0; 3], [3.0, -2.5, 1.5]] {
        let rig = build(
            &geometry(offset),
            &animation(serde_json::json!([entry("tip", offset)])),
        )
        .unwrap();
        let root = rig.geometry.bone_pivots[0];
        let rest = rig.pose(
            RenderBoneTransform {
                rotation: Quat::IDENTITY.to_array(),
                translation_scale: [root[0], root[1], root[2], 1.0],
                axis_scale: crate::UNIT_AXIS_SCALE,
            },
            Motion::default(),
        );
        let vertex = corner(&rig, offset);
        let index = rig
            .geometry
            .vertices
            .iter()
            .position(|candidate| std::ptr::eq(candidate, vertex))
            .unwrap();
        let placed = rig
            .emitter_transform(0, &rest, &rest, preview, 0.0)
            .unwrap();
        let expected = Vec3::from_array(rig.rest_vertices[index].position);
        let preview = rig.preview_emitter_transform(0, Motion::default()).unwrap();
        assert!(Vec3::from_array(preview.position).distance(expected) < 1e-5);
        assert!(
            Vec3::from_array(placed.position).distance(expected) < 1e-5,
            "{offset:?}: {placed:?} != {expected}"
        );
    }
}

#[test]
fn an_offset_emitter_follows_the_posed_bone_rotation_scale_and_placement() {
    let offset = [3.0, -2.5, 1.5];
    let rig = build(
        &geometry(offset),
        &animation(serde_json::json!([entry("tip", offset)])),
    )
    .unwrap();
    let world = [
        [0.0, 0.0, 1.0, 10.0],
        [0.0, 1.0, 0.0, 64.0],
        [-1.0, 0.0, 0.0, -3.0],
    ];
    let attachment = |yaw: f32, scale: f32| RenderBoneTransform {
        rotation: Quat::from_rotation_y(yaw).to_array(),
        translation_scale: [0.0, 1.5, 0.0, scale],
        axis_scale: crate::UNIT_AXIS_SCALE,
    };
    let previous = rig.pose(attachment(0.0, 1.0), Motion::default());
    let current = rig.pose(attachment(1.1, 0.5), Motion::default());
    let vertex = corner(&rig, offset);
    for alpha in [0.0, 0.25, 1.0] {
        let placed = rig
            .emitter_transform(0, &previous, &current, world, alpha)
            .unwrap();
        let expected = rendered(&rig, &previous, vertex, world)
            .lerp(rendered(&rig, &current, vertex, world), alpha);
        assert!(Vec3::from_array(placed.position).distance(expected) < 1e-5);
    }
    let still = rig
        .emitter_transform(0, &previous, &previous, world, 0.0)
        .unwrap();
    let turned = rig
        .emitter_transform(0, &current, &current, world, 0.0)
        .unwrap();
    assert!(Vec3::from_array(still.position).distance(Vec3::from_array(turned.position)) > 0.05);
    let axis = Vec3::from_array(turned.basis[1]);
    assert!(
        (axis.length() - 0.5).abs() < 1e-5,
        "the basis keeps inherited scale"
    );
}

#[test]
fn emitter_effects_get_unique_client_owned_identifiers() {
    let bytes = animation(serde_json::json!([
        entry("tip", [0.0; 3]),
        entry("head", [1.0, 2.0, 3.0])
    ]));
    let rig = build(&geometry([0.0; 3]), &bytes).unwrap();
    let digest = fixture::animation_asset(&bytes).sha256;
    assert_eq!(rig.emitters().len(), 2);
    for (index, emitter) in rig.emitters().iter().enumerate() {
        let expected = format!("{COSMETIC_EFFECT_NAMESPACE}:{}_{index}", &digest[..16]);
        assert_eq!(&*emitter.identifier, expected);
        let effect: serde_json::Value = serde_json::from_slice(&emitter.effect).unwrap();
        assert_eq!(
            effect["particle_effect"]["description"]["identifier"],
            expected
        );
        assert_eq!(
            effect["particle_effect"]["components"],
            serde_json::from_str::<serde_json::Value>(fixture::EMITTER_EFFECT).unwrap()["particle_effect"]
                ["components"]
        );
    }
    assert!(super::rig(fixture::crown()).emitters().is_empty());
}

#[test]
fn malformed_emitters_reject_the_whole_item() {
    let geometry = geometry([0.0; 3]);
    assert!(
        build(
            &geometry,
            &animation(serde_json::json!([entry("tip", [64.0, -64.0, 0.0])]))
        )
        .is_some()
    );
    let mut missing_identifier = entry("tip", [0.0; 3]);
    missing_identifier["effect"]["particle_effect"]["description"]
        .as_object_mut()
        .unwrap()
        .remove("identifier");
    let mut numeric_identifier = entry("tip", [0.0; 3]);
    numeric_identifier["effect"]["particle_effect"]["description"]["identifier"] = 7.into();
    let mut not_object = entry("tip", [0.0; 3]);
    not_object["effect"] = "minecraft:basic_flame_particle".into();
    let mut unknown_field = entry("tip", [0.0; 3]);
    unknown_field["locator"] = "tip".into();
    for particles in [
        serde_json::json!([entry("missing", [0.0; 3])]),
        serde_json::json!([entry("tip", [64.5, 0.0, 0.0])]),
        serde_json::json!([missing_identifier]),
        serde_json::json!([numeric_identifier]),
        serde_json::json!([not_object]),
        serde_json::json!([unknown_field]),
        serde_json::json!([{"bone": "tip", "offset": [0, 0], "effect": {}}]),
        serde_json::Value::Array(vec![entry("tip", [0.0; 3]); MAX_COSMETIC_EMITTERS + 1]),
    ] {
        assert!(
            build(&geometry, &animation(particles.clone())).is_none(),
            "{particles}"
        );
    }
    let bytes = animation(serde_json::json!([entry("tip", [0.0; 3])]));
    let mut descriptor = fixture::crown().descriptor().clone();
    descriptor.assets.animation = None;
    assert!(
        model::build(&descriptor, &geometry, include_bytes!("crown.png"), &bytes).is_none(),
        "emitter identifiers require a verified animation digest"
    );
}

#[test]
fn emissive_items_keep_preview_art_and_add_a_fullbright_world_raster() {
    let plain = rig(fixture::crown());
    let glowing = rig(fixture::glowing_crown());
    assert!(!plain.emissive());
    assert!(glowing.emissive());
    assert_eq!(plain.world_pixels().pixels(), plain.pixels.pixels());
    assert_eq!(glowing.pixels.pixels(), plain.pixels.pixels());
    assert_eq!(glowing.texture, plain.texture);
    let world = glowing.world_pixels();
    assert_eq!(world.len(), glowing.pixels.len());
    let mut opaque = 0;
    for (source, converted) in glowing
        .pixels
        .as_chunks::<4>()
        .0
        .iter()
        .zip(world.as_chunks::<4>().0)
    {
        if source[3] >= 128 {
            opaque += 1;
            assert_eq!(converted[3], 0);
            assert_ne!(converted[..3], [0; 3]);
        } else {
            assert_eq!(*converted, [0; 4]);
        }
    }
    assert!(opaque > 0);
    assert_eq!(glowing.emitters().len(), 1);
}

#[test]
fn emissive_raster_lifts_opaque_black_and_clears_translucent_texels() {
    assert_eq!(
        &*model::emissive_raster(&[
            10, 20, 30, 255, 0, 0, 0, 200, 0, 0, 0, 128, 9, 9, 9, 127, 4, 5, 6, 0
        ]),
        &[
            10, 20, 30, 0, 1, 1, 1, 0, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0
        ]
    );
}

/// A particle entry whose effect samples the cosmetic sprite at `sprites`.
fn sprite_entry(sprites: Option<serde_json::Value>, texture: &str) -> serde_json::Value {
    let mut entry = entry("tip", [0.0; 3]);
    entry["effect"]["particle_effect"]["description"]["basic_render_parameters"]["texture"] =
        texture.into();
    if let Some(sprites) = sprites {
        entry["sprites"] = sprites;
    }
    entry
}

#[test]
fn sprite_emitters_cut_their_region_from_the_original_texture() {
    let geometry = geometry([0.0; 3]);
    let bytes = animation(serde_json::json!([
        sprite_entry(Some(serde_json::json!([16, 8])), COSMETIC_PARTICLE_TEXTURE),
        sprite_entry(Some(serde_json::json!([48, 16])), COSMETIC_PARTICLE_TEXTURE),
        entry("tip", [0.0; 3]),
    ]));
    let model = build(&geometry, &bytes).unwrap();
    let side = COSMETIC_SPRITE_SIDE as usize;
    let width = model.texture.width as usize;
    for (emitter, [u, v]) in model.emitters()[..2].iter().zip([[16, 8], [48, 16]]) {
        let sprite = emitter.sprite.as_ref().unwrap();
        let expected: Vec<u8> = (v..v + side)
            .flat_map(|y| {
                model.texture.rgba8[(y * width + u) * 4..(y * width + u + side) * 4].to_vec()
            })
            .collect();
        assert_eq!(&*sprite.pixels, expected.as_slice());
    }
    let keys: Vec<_> = model.emitters()[..2]
        .iter()
        .map(|emitter| emitter.sprite.as_ref().unwrap().key)
        .collect();
    assert_ne!(keys[0], keys[1], "each region has its own atlas tile");
    let again = build(&geometry, &bytes).unwrap();
    assert_eq!(again.emitters()[0].sprite.as_ref().unwrap().key, keys[0]);
    assert!(model.emitters()[2].sprite.is_none());
    assert!(
        build(
            &geometry,
            &animation(serde_json::Value::Array(vec![
                entry("tip", [0.0; 3]);
                MAX_COSMETIC_EMITTERS
            ]))
        )
        .is_some()
    );
}

#[test]
fn mismatched_or_out_of_bounds_sprites_reject_the_item() {
    let geometry = geometry([0.0; 3]);
    for particle in [
        sprite_entry(Some(serde_json::json!([49, 0])), COSMETIC_PARTICLE_TEXTURE),
        sprite_entry(Some(serde_json::json!([0, 17])), COSMETIC_PARTICLE_TEXTURE),
        sprite_entry(Some(serde_json::json!([-1, 0])), COSMETIC_PARTICLE_TEXTURE),
        sprite_entry(
            Some(serde_json::json!([u32::MAX, 0])),
            COSMETIC_PARTICLE_TEXTURE,
        ),
        sprite_entry(None, COSMETIC_PARTICLE_TEXTURE),
        sprite_entry(
            Some(serde_json::json!([0, 0])),
            "textures/particle/particles",
        ),
    ] {
        assert!(
            build(&geometry, &animation(serde_json::json!([particle.clone()]))).is_none(),
            "{particle}"
        );
    }
    assert!(
        build(
            &geometry,
            &animation(serde_json::json!([sprite_entry(
                Some(serde_json::json!([48, 16])),
                COSMETIC_PARTICLE_TEXTURE
            )]))
        )
        .is_some(),
        "the bottom-right region fits exactly"
    );
}
