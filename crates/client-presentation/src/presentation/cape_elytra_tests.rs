//! Cape-backed wings keep their draw and glint while replacing only the texture source.

use super::apply_capes;
use view_presentation::cape::{ACTOR_LAYER_CAPE, CapeRig};
use crate::presentation::{actors::ActorPresentationBatch, equipment::ELYTRA_LAYER};
use assets::EntityRenderMaterial;
use client_world::PlayerProfile;
use protocol::{CapeImage, PlayerSkin, StandardSkin};
use render::{
    ACTOR_LAYER_BODY, ActorArtworkPages, ActorGlint, ActorMaterial, ActorRenderIdentity,
    ActorRigRenderInput, ActorRigRoute, ActorRigSubmission, };
use render_model::equipment::EquipmentRaster;
use render_model::{EntityRigId, RenderBoneTransform, STANDARD_SKIN_BYTES, STANDARD_SKIN_SIDE};
use std::{collections::HashMap, sync::Arc};

/// Worn wings bypass the separate cape geometry and pose.
fn fixture_cape() -> CapeRig {
    let geometry = render_model::diagnostic_geometry();
    let bones = geometry
        .bone_pivots
        .iter()
        .enumerate()
        .map(|(index, _)| {
            serde_json::from_value(serde_json::json!({
                "name": format!("bone{index}"), "cubes": []
            }))
            .unwrap()
        })
        .collect::<Vec<assets::EntityGeometryBone>>();
    CapeRig::from_geometry(geometry, &bones).unwrap()
}

/// Supplies a body and one enchanted wing draw with an ordinary equipment artwork route.
fn batch() -> ActorPresentationBatch {
    let pose = Arc::from([RenderBoneTransform {
        rotation: [0.0, 0.0, 0.0, 1.0],
        translation_scale: [0.0, 0.0, 0.0, 1.0],
        axis_scale: render_model::UNIT_AXIS_SCALE,
    }]);
    let body = ActorRigSubmission {
        material: Default::default(),
        culling_bounds: Default::default(),
        input: ActorRigRenderInput {
            identity: ActorRenderIdentity {
                session_id: 1,
                dimension: 0,
                runtime_id: 2,
                spawn_revision: 1,
                ingress_sequence: 1,
                source_tick: None,
                movement_revision: 0,
                pose_generation: 1,
                layer: ACTOR_LAYER_BODY,
            },
            rig: EntityRigId(1),
            previous_bones: Arc::clone(&pose),
            current_bones: pose,
            completed_tick: 4,
            reset_generation: 0,
        },
        world_from_actor: [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ],
        texture_layer: 0,
        route: ActorRigRoute::Compiled,
        tint: 0,
        uv_anim: render::IDENTITY_UV_ANIM,
        light: 0,
        overlay_rgba8: 0,
    };
    let (_, locations) = ActorArtworkPages::default().with_equipment_rasters(&[EquipmentRaster {
        width: 64,
        height: 32,
        rgba8: vec![200; 64 * 32 * 4].into(),
    }]);
    let location = locations[0].unwrap();
    let mut wing = body.clone();
    wing.input.identity.layer = ELYTRA_LAYER;
    wing.input.rig = EntityRigId(7);
    wing.texture_layer = location.layer();
    wing.material = ActorMaterial {
        kind: EntityRenderMaterial::Glint,
        state: Some(assets::EntityRenderMaterialState {
            alpha_test: true,
            cull: false,
            ..Default::default()
        }),
        glint: ActorGlint {
            time_seconds: 4.0,
            ..Default::default()
        },
        ..Default::default()
    };
    ActorPresentationBatch {
        artwork: HashMap::from([(wing.input.identity, location)]),
        submissions: vec![body, wing],
        skin_layers: vec![vec![0; STANDARD_SKIN_BYTES].into()],
    }
}

/// A synthetic player skin with an optional solid cape raster.
fn profile(has_cape: bool) -> PlayerProfile {
    PlayerProfile {
        unique_id: 2,
        username: "cape-test".into(),
        verified: true,
        skin: PlayerSkin::Standard(StandardSkin {
            width: STANDARD_SKIN_SIDE as u32,
            height: STANDARD_SKIN_SIDE as u32,
            rgba8: vec![0; STANDARD_SKIN_BYTES].into(),
            geometry: None,
            cape: has_cape.then(|| CapeImage {
                width: 64,
                height: 32,
                rgba8: [9, 8, 7, 255].repeat(64 * 32).into(),
            }),
        }),
    }
}

#[test]
fn player_cape_replaces_the_wings_texture_and_suppresses_a_separate_cape_draw() {
    let mut batch = batch();
    let profile = profile(true);
    let cape = fixture_cape();
    let before = batch.submissions[1].clone();
    apply_capes(
        &mut batch,
        &cape,
        |_| None,
        |_| Some(&profile),
        |_| None,
        |_| true,
    );
    assert_eq!(batch.submissions.len(), 2);
    assert!(
        !batch
            .submissions
            .iter()
            .any(|draw| draw.input.identity.layer == ACTOR_LAYER_CAPE)
    );
    let wing = &batch.submissions[1];
    assert_eq!(wing.input, before.input);
    assert_eq!(wing.world_from_actor, before.world_from_actor);
    assert_eq!(
        wing.material, before.material,
        "cape replacement retains enchant glint"
    );
    assert_eq!(wing.texture_layer, 1);
    assert!(!batch.artwork.contains_key(&wing.input.identity));
    assert_eq!(batch.skin_layers.len(), 2);
    assert_eq!(&batch.skin_layers[1][..4], &[9, 8, 7, 255]);
}

#[test]
fn capeless_player_keeps_the_pack_wings_texture_without_adding_a_skin_layer() {
    let mut batch = batch();
    let profile = profile(false);
    let cape = fixture_cape();
    let before = batch.submissions[1].clone();
    let artwork = batch.artwork.clone();
    apply_capes(
        &mut batch,
        &cape,
        |_| None,
        |_| Some(&profile),
        |_| None,
        |_| true,
    );
    assert_eq!(batch.submissions.len(), 2);
    assert_eq!(batch.submissions[1], before);
    assert_eq!(batch.artwork, artwork);
    assert_eq!(batch.skin_layers.len(), 1);
}

#[test]
fn java_cape_motion_cannot_replace_worn_wing_pose_or_glint() {
    let mut batch = batch();
    let mut extra = batch.submissions[1].clone();
    let location = batch.artwork[&extra.input.identity];
    extra.input.identity.layer = (0..=u8::MAX)
        .find(|&layer| {
            layer != ELYTRA_LAYER && crate::presentation::equipment::is_elytra_layer(layer)
        })
        .unwrap();
    batch.artwork.insert(extra.input.identity, location);
    batch.submissions.push(extra);
    let profile = profile(true);
    let cape = fixture_cape();
    let before = batch.submissions[1..].to_vec();
    let java_calls = std::cell::Cell::new(0);
    apply_capes(
        &mut batch,
        &cape,
        |_| None,
        |_| Some(&profile),
        |_| {
            java_calls.set(java_calls.get() + 1);
            Some(render_model::java_animation::JavaCapeInput {
                chase: bevy::math::Vec3::new(0.4, -0.8, 0.7),
                bob: 0.1,
                walked: 12.5,
                sneaking: true,
                ..Default::default()
            })
        },
        |_| true,
    );
    assert_eq!(
        java_calls.get(),
        0,
        "worn wings suppress Java cape evaluation"
    );
    assert_eq!(batch.submissions.len(), 3);
    for (wing, before) in batch.submissions[1..].iter().zip(before) {
        assert_eq!(wing.input, before.input);
        assert_eq!(wing.world_from_actor, before.world_from_actor);
        assert_eq!(wing.material, before.material);
        assert_eq!(
            &batch.skin_layers[wing.texture_layer as usize][..4],
            &[9, 8, 7, 255]
        );
        assert!(!batch.artwork.contains_key(&wing.input.identity));
    }
}
