use super::*;
use crate::presentation::actors::{PoseConversions, actor_rig_presentation_cached};

#[test]
fn java_local_shadow_pose_matches_third_person_during_walk_and_swing() {
    let mut first = tests::head_stream();
    let mut third = tests::head_stream();
    first.set_actor_world_body_enabled(true);
    let equipment = ActorEquipmentInput::default();
    let mut first_poses = PoseConversions::default();
    let mut shadow_poses = PoseConversions::default();
    let mut third_poses = PoseConversions::default();
    let mut shadow_java = PoseScratch::default();
    let mut third_java = PoseScratch::default();
    for tick in 0..6 {
        let mut feed = tests::uploaded_skin_feed(true);
        feed.position[0] += tick as f32 * 0.2;
        feed.velocity = [0.2, 0.0, 0.0];
        feed.sneaking = tick >= 3;
        for (stream, first_person) in [(&mut first, true), (&mut third, false)] {
            feed.first_person = first_person;
            stream.sync_local_player_pose(&feed);
            stream.prepare_actor_appearance_fixture();
            stream.advance_actor_interpolation_frame(1);
            stream.sync_local_swing(actor_animation::LocalSwingProgress {
                bedrock: [0.1, 0.3],
                java: [0.2, 0.4],
                frame_alpha: Some(0.5),
            });
        }
        for alpha in [0.0, 0.5, 1.0] {
            let rig = third.authority().actor_rig(1).unwrap();
            let actor = third.authority().actor(1).unwrap();
            let selected = third_person(
                &third,
                &rig,
                actor,
                Some(&equipment),
                alpha,
                &mut third_java,
            )
            .unwrap();
            let mut expected = actor_rig_presentation_cached(
                &selected.rig,
                actor,
                third.authority().actor_player_profile(1),
                alpha,
                &mut third_poses,
            )
            .unwrap();
            apply_pose(&mut expected, &selected.bones, true, actor, alpha);
            lift(
                &mut expected.submission.world_from_actor,
                actor.is_sneaking(),
                true,
            );

            let rig = first.authority().actor_rig(1).unwrap();
            let actor = first.authority().actor(1).unwrap();
            let local = actor_rig_presentation_cached(
                &rig,
                actor,
                first.authority().actor_player_profile(1),
                alpha,
                &mut first_poses,
            )
            .unwrap();
            let mut posed = Vec::new();
            let shadow = super::super::culling::with_local_shadow_body(
                Some(local),
                true,
                Some(&first),
                (&mut shadow_poses, &mut shadow_java, &mut posed),
                (true, &equipment),
                alpha,
            )
            .unwrap();
            assert_eq!(
                shadow.submission.route,
                render_model::ActorRigRoute::ShadowOnly
            );
            assert_eq!(
                shadow.submission.input.previous_bones,
                expected.submission.input.previous_bones
            );
            assert_eq!(
                shadow.submission.input.current_bones,
                expected.submission.input.current_bones
            );
            assert!((shadow.world_yaw_degrees - expected.world_yaw_degrees).abs() < 1e-5);
            for (actual, expected) in shadow
                .submission
                .world_from_actor
                .iter()
                .zip(expected.submission.world_from_actor)
            {
                for axis in 0..3 {
                    assert!((actual[axis] - expected[axis]).abs() < 1e-5);
                }
            }
            assert_eq!(posed.len(), 1);
            assert_eq!(posed[0].skin_layers.len(), selected.posed.skin_layers.len());
            for (actual, expected) in posed[0].skin_layers.iter().zip(&selected.posed.skin_layers) {
                assert_eq!(actual.current, expected.current);
                assert_eq!(actual.previous, expected.previous);
            }
            assert_eq!(posed[0].cape, selected.posed.cape);
        }
    }
}
