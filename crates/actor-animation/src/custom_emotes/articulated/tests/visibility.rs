use super::*;
use crate::custom_emotes::{sample_custom_emote, tests::Fixture};
use crate::{RenderTextureLayer, SkinRenderLayer};

/// Creates a body draw with an independent render-controller visibility mask.
fn body_layer(hidden_bones: Arc<[u32]>) -> RenderTextureLayer {
    RenderTextureLayer {
        material: assets::EntityRenderMaterial::Default,
        material_state: None,
        source: 0,
        texture_slot: 0,
        multitexture: None,
        color: [1.0; 4],
        overlay: [0.0; 4],
        hidden_bones,
        uv_anim: [0.0, 0.0, 1.0, 1.0],
        geometry: None,
        previous_pose: Arc::from([]),
        pose: Arc::from([]),
        ignore_lighting: false,
        light_color_multiplier: 1.0,
        sampled_scale: None,
    }
}

/// Checks each original or split piece against the visibility of its original cuboid.
fn assert_visibility(
    source: &SkinGeometry,
    sampled: &SkinGeometry,
    original_hidden: &[u32],
    sampled_hidden: &[u32],
) {
    for (index, bone) in sampled.bones.iter().enumerate() {
        let name = bone.name.split(".cinnabar_").next().unwrap();
        let original = source
            .bones
            .iter()
            .position(|bone| bone.name.eq_ignore_ascii_case(name))
            .unwrap();
        assert_eq!(
            sampled_hidden.contains(&(index as u32)),
            original_hidden.contains(&(original as u32)),
            "{} must keep {}'s visibility",
            bone.name,
            source.bones[original].name,
        );
    }
}

#[test]
fn generated_body_pieces_keep_each_render_layers_source_visibility() {
    let source = full_fixture();
    let fixture = Fixture::from_skin((*source).clone());
    for emote in [crate::custom_emotes::tests::catalog_emote()] {
        for hidden in std::iter::once(None).chain((0..source.bones.len()).map(Some)) {
            let hidden: Arc<[u32]> = hidden.map(|index| index as u32).into_iter().collect();
            let render = [body_layer(Arc::clone(&hidden)), body_layer(Arc::from([]))];
            let original = crate::ActorRigSnapshot {
                render: &render,
                ..fixture.rig()
            };
            let pose = sample_custom_emote(&original, emote, 0.1, 1.0).unwrap();
            let sampled = pose.snapshot(original);
            let geometry = sampled.skin_geometry.unwrap();
            assert!(geometry.bones.len() > source.bones.len());
            assert_visibility(&source, geometry, &hidden, &sampled.render[0].hidden_bones);
            assert_visibility(&source, geometry, &[], &sampled.render[1].hidden_bones);
            assert_eq!(original.render[0].hidden_bones, hidden);
            assert_eq!(original.bone_names.len(), source.bones.len());
        }
    }
}

#[test]
fn generated_skin_layer_pieces_keep_their_own_source_visibility() {
    let source = full_fixture();
    let fixture = Fixture::from_skin((*source).clone());
    let mut layer_source = (*source).clone();
    layer_source.bones.reverse();
    layer_source.digest = Sha256::digest(b"reordered visibility layer").into();
    let layer_fixture = Fixture::from_skin(layer_source.clone());
    let layer_source = Arc::new(layer_source);
    for emote in [crate::custom_emotes::tests::catalog_emote()] {
        for hidden in std::iter::once(None).chain((0..layer_source.bones.len()).map(Some)) {
            let hidden: Arc<[u32]> = hidden.map(|index| index as u32).into_iter().collect();
            let layer = SkinRenderLayer {
                image: render_api::SkinAnimation {
                    kind: render_api::SkinAnimationKind::Face,
                    width: 64,
                    height: 64,
                    rgba8: Arc::from([]),
                    frames: 1,
                    blinking: false,
                },
                geometry: Arc::clone(&layer_source),
                mesh: None,
                previous: layer_fixture.rest.clone().into(),
                current: layer_fixture.rest.clone().into(),
                rest: layer_fixture.rest.clone().into(),
                hidden_bones: Arc::clone(&hidden),
                uv_anim: [0.0, 0.0, 1.0, 1.0],
            };
            let layers = [layer];
            let render = [body_layer(Arc::from([]))];
            let original = crate::ActorRigSnapshot {
                render: &render,
                skin_layers: &layers,
                ..fixture.rig()
            };
            let pose = sample_custom_emote(&original, emote, 0.1, 1.0).unwrap();
            let sampled = pose.snapshot(original);
            let layer = &sampled.skin_layers[0];
            assert!(layer.geometry.bones.len() > layer_source.bones.len());
            assert_visibility(&layer_source, &layer.geometry, &hidden, &layer.hidden_bones);
            assert!(sampled.render[0].hidden_bones.is_empty());
            assert_eq!(original.skin_layers[0].hidden_bones, hidden);
            assert!(Arc::ptr_eq(
                &original.skin_layers[0].geometry,
                &layer_source
            ));
        }
    }
}

#[test]
fn hidden_body_pieces_do_not_raise_visible_floor_contacts() {
    let source = full_fixture();
    let fixture = Fixture::from_skin((*source).clone());
    let hidden = source
        .bones
        .iter()
        .enumerate()
        .filter_map(|(index, bone)| {
            (!bone.name.eq_ignore_ascii_case("head")).then_some(index as u32)
        })
        .collect();
    let render = [body_layer(hidden)];
    let original = crate::ActorRigSnapshot {
        render: &render,
        ..fixture.rig()
    };
    let pose = sample_custom_emote(
        &original,
        crate::custom_emotes::tests::catalog_emote(),
        0.0,
        0.0,
    )
    .unwrap();
    let sampled = pose.snapshot(original);
    let head = sampled
        .bone_names
        .iter()
        .position(|name| name.as_ref() == "head")
        .unwrap();
    let minimum = posed_vertices(sampled.skin_mesh.unwrap(), &pose.current, head)
        .iter()
        .map(|point| point[1])
        .fold(f32::INFINITY, f32::min);
    assert!(
        minimum.abs() < 1e-4,
        "only the visible head should contact the floor: {minimum}"
    );
}

#[test]
fn contacts_include_a_piece_visible_in_any_body_draw_and_allow_no_visible_bounds() {
    let source = full_fixture();
    let fixture = Fixture::from_skin((*source).clone());
    let hidden: Arc<[u32]> = (0..source.bones.len() as u32).collect();
    let model = model(&source).unwrap();
    let (_, names) = crate::geometry::skeleton(&model.geometry.bones).unwrap();
    let render = [body_layer(Arc::clone(&hidden)), body_layer(Arc::from([]))];
    let original = crate::ActorRigSnapshot {
        render: &render,
        ..fixture.rig()
    };
    let contacts = crate::custom_emotes::floor::contacts(&original, Some(&model), &names, 12.0);
    assert!(contacts.iter().any(Option::is_some));
    let render = [body_layer(hidden)];
    let original = crate::ActorRigSnapshot {
        render: &render,
        ..fixture.rig()
    };
    let contacts = crate::custom_emotes::floor::contacts(&original, Some(&model), &names, 12.0);
    assert!(contacts.iter().all(Option::is_none));
    let mut pose = fixture.rest.clone();
    assert_eq!(
        crate::custom_emotes::floor::ground_pose(&mut pose, &contacts, 0.0),
        0.0
    );
    assert_eq!(pose, fixture.rest);
}
