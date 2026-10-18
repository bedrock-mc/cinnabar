//! Legacy empty-arm and offhand pose isolation.
use super::*;

#[test]
fn legacy_empty_main_arm_and_native_offhand_keep_separate_pose_stacks() {
    for identifier in ["minecraft:shield", "minecraft:totem_of_undying"] {
        let (mut stream, mut equipment, artwork, mut input) = fixture(None);
        let mut feed = player_feed();
        feed.first_person = true;
        feed.off_hand = Some(identifier.into());
        stream.sync_local_player_pose(&feed);
        stream.advance_actor_interpolation_frame(0);
        input.off = Some(crate::presentation::equipment::WornItem {
            variant: Default::default(),
            charged_projectile: None,
            use_facts: client_world::EffectiveItemUse::default(),
            grip: assets::item::grip::builtin_item_grip(identifier),
            identifier: identifier.into(),
            metadata: 0,
            damage: None,
            dye_rgb: None,
            enchanted: false,
            glint: Default::default(),
            kind: HeldKind::Sprite,
        });
        let mut cache = java::HandCache::default();
        let mut masked_pose: Option<[Arc<[render_model::RenderBoneTransform]>; 2]> = None;
        for (physics_alpha, actor_alpha) in [(0.75, 0.1), (0.25, 0.9)] {
            stream.sync_local_swing(actor_animation::LocalSwingProgress {
                bedrock: [0.25, 0.5],
                java: [0.25, 0.5],
                frame_alpha: Some(physics_alpha),
            });
            stream.advance_actor_interpolation_frame(0);
            let rig = stream.authority().actor_rig(1).unwrap();
            let hand = hand_progress(rig.hand, None, actor_alpha);
            let source = hand_source_for_mode(
                &stream,
                &mut equipment,
                &artwork,
                &input,
                &mut cache,
                actor_alpha,
                false,
            );
            let swing = 0.25 + 0.25 * physics_alpha;
            let camera = render_model::legacy_combat::legacy_first_person_arm(swing, hand.equip);
            assert!(source.body_camera.unwrap().abs_diff_eq(camera, 1e-5));
            let rest = cache.native_pose.rest_pose(&rig).unwrap();
            let body = source.body.as_ref().expect("empty right arm");
            let arm = rig
                .bone_names
                .iter()
                .position(|name| name.as_ref() == "rightarm")
                .unwrap();
            assert_eq!(body.input.previous_bones[arm], rest[arm]);
            assert_eq!(body.input.current_bones[arm], rest[arm]);
            if let Some([previous, current]) = masked_pose.as_ref() {
                assert!(Arc::ptr_eq(previous, &body.input.previous_bones));
                assert!(Arc::ptr_eq(current, &body.input.current_bones));
            }
            masked_pose = Some([
                Arc::clone(&body.input.previous_bones),
                Arc::clone(&body.input.current_bones),
            ]);
            let left = rig
                .bone_names
                .iter()
                .position(|name| name.as_ref() == "leftarm")
                .unwrap();
            let expected = Quat::from_rotation_x(-swing * 60.0_f32.to_radians());
            assert!(
                Quat::from_array(source.presentation.submission.input.current_bones[left].rotation)
                    .abs_diff_eq(expected, 1e-5)
            );
            let off = &source.items[1].as_ref().expect("drawable offhand").0;
            if identifier == "minecraft:shield" {
                assert!(!off.camera_space);
                assert!(
                    Quat::from_array(off.presentation.submission.input.current_bones[0].rotation)
                        .abs_diff_eq(expected, 1e-5)
                );
            } else {
                assert!(off.camera_space);
            }
            let off_camera = if off.camera_space {
                camera_space_rows(Mat4::IDENTITY)
            } else {
                hand_camera_from_rig(source.presentation.authored_scale, Mat4::IDENTITY)
            };
            let [body, _, off] = source.submissions();
            let off = off.unwrap();
            assert_eq!(off.world_from_actor, off_camera);
            assert_ne!(body.unwrap().world_from_actor, off.world_from_actor);
            assert!(cache.native_pose.sample_work.0 > 0);
        }
    }
}
