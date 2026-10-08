use bevy::prelude::Vec3;
use client_world::CrystalBeamView;
use render::{BlockEntityKind, BlockEntityLight, BlockEntitySubmission, CrystalBeamModel};

/// Admits effects by their owning body's interpolated position before consuming scene capacity.
pub(super) fn submit(
    submissions: &mut Vec<BlockEntitySubmission>,
    beams: impl IntoIterator<Item = CrystalBeamView>,
    camera: Option<Vec3>,
    mut light_at: impl FnMut(u64, [f32; 3]) -> Option<(u8, u8)>,
) {
    for beam in beams {
        if camera.is_some_and(|camera| {
            !client_presentation::presentation::actors::within_actor_candidate_cube(
                beam.owner_position,
                camera.to_array(),
            )
        }) {
            continue;
        }
        if submissions.len() >= super::MAX_SUBMISSIONS {
            break;
        }
        submissions.push(BlockEntitySubmission {
            block: beam.target.map(|value| value as i32),
            light: light_at(beam.runtime_id, beam.owner_position).map_or_else(
                || 1.0.into(),
                |(block, sky)| BlockEntityLight::Actor { block, sky },
            ),
            kind: BlockEntityKind::CrystalBeam(CrystalBeamModel {
                target: beam.target,
                crystal: beam.crystal,
                age_ticks: beam.age_ticks,
            }),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use render::ACTOR_CANDIDATE_RADIUS_BLOCKS;

    /// A beam reaching the camera from a crystal at the supplied interpolated position.
    fn beam(camera: Vec3, offset: Vec3) -> CrystalBeamView {
        CrystalBeamView {
            runtime_id: 1,
            owner_position: (camera + offset).to_array(),
            target: camera.to_array(),
            crystal: (camera + offset).to_array(),
            age_ticks: 1.0,
        }
    }

    #[test]
    fn crystal_beams_outside_actor_candidate_cube_never_use_the_submission_budget() {
        let camera = Vec3::new(100.0, 50.0, -100.0);
        let radius = ACTOR_CANDIDATE_RADIUS_BLOCKS;
        for axis in [Vec3::X, Vec3::Y, Vec3::Z] {
            for sign in [-1.0, 1.0] {
                let rejected = beam(camera, axis * sign * (radius + 1.0));
                let boundary = beam(camera, axis * sign * radius);
                let mut submissions = vec![
                    BlockEntitySubmission {
                        block: [0; 3],
                        light: 1.0.into(),
                        kind: BlockEntityKind::EndPortal,
                    };
                    super::super::MAX_SUBMISSIONS - 1
                ];
                submit(
                    &mut submissions,
                    [rejected, boundary],
                    Some(camera),
                    |_, _| None,
                );
                assert_eq!(submissions.len(), super::super::MAX_SUBMISSIONS);
                let BlockEntityKind::CrystalBeam(model) = &submissions.last().unwrap().kind else {
                    panic!("the in-range crystal must keep the remaining submission slot");
                };
                assert_eq!(
                    model.crystal, boundary.crystal,
                    "an out-of-range crystal cannot draw a beam even with a nearby target"
                );
            }
        }
    }

    #[test]
    fn culled_crystal_beams_stay_absent_as_their_animation_age_advances() {
        let camera = Vec3::ZERO;
        let mut far = beam(camera, Vec3::X * (ACTOR_CANDIDATE_RADIUS_BLOCKS + 1.0));
        for age in [1.0, 2.0] {
            far.age_ticks = age;
            let mut submissions = Vec::new();
            submit(&mut submissions, [far], Some(camera), |_, _| None);
            assert!(
                submissions.is_empty(),
                "culled beams must produce no per-frame mesh work"
            );
        }
    }

    #[test]
    fn crystal_beam_admission_uses_a_cube_and_matches_the_body_without_a_camera() {
        let camera = Vec3::new(100.0, 50.0, -100.0);
        let corner = beam(camera, Vec3::splat(ACTOR_CANDIDATE_RADIUS_BLOCKS));
        let mut submissions = Vec::new();
        submit(&mut submissions, [corner], Some(camera), |_, _| None);
        assert_eq!(
            submissions.len(),
            1,
            "cube corners are inside, regardless of spherical distance"
        );
        submissions.clear();
        let far = beam(camera, Vec3::splat(ACTOR_CANDIDATE_RADIUS_BLOCKS + 1.0));
        submit(&mut submissions, [far], None, |_, _| None);
        assert_eq!(
            submissions.len(),
            1,
            "body admission also stays open without a camera"
        );
    }

    #[test]
    fn healing_beam_admission_follows_the_dragon_instead_of_the_crystal_endpoint() {
        let mut healing = beam(Vec3::ZERO, Vec3::X * (ACTOR_CANDIDATE_RADIUS_BLOCKS + 1.0));
        healing.owner_position = Vec3::ZERO.to_array();
        let mut submissions = Vec::new();
        submit(&mut submissions, [healing], Some(Vec3::ZERO), |_, _| None);
        assert_eq!(submissions.len(), 1);
        submissions.clear();
        healing.owner_position = healing.crystal;
        healing.crystal = Vec3::ZERO.to_array();
        submit(&mut submissions, [healing], Some(Vec3::ZERO), |_, _| None);
        assert!(submissions.is_empty());
    }

    #[test]
    fn admitted_beam_samples_owner_light_after_culling_and_retains_both_levels() {
        let culled = beam(Vec3::ZERO, Vec3::X * (ACTOR_CANDIDATE_RADIUS_BLOCKS + 1.0));
        let mut admitted = beam(Vec3::ZERO, Vec3::new(2.0, 3.0, 4.0));
        admitted.runtime_id = 37;
        admitted.target = [100.0; 3];
        admitted.crystal = [200.0; 3];
        let mut submissions = Vec::new();
        let mut samples = Vec::new();
        submit(
            &mut submissions,
            [culled, admitted],
            Some(Vec3::ZERO),
            |runtime, position| {
                samples.push((runtime, position));
                Some((4, 11))
            },
        );
        assert_eq!(submissions.len(), 1);
        assert_eq!(
            submissions[0].light,
            render::BlockEntityLight::Actor { block: 4, sky: 11 }
        );
        assert_eq!(samples, [(admitted.runtime_id, admitted.owner_position)]);
    }

    #[test]
    fn unavailable_owner_light_keeps_the_existing_scalar_fallback() {
        let admitted = beam(Vec3::ZERO, Vec3::ZERO);
        let mut submissions = Vec::new();
        submit(&mut submissions, [admitted], None, |_, _| None);
        assert_eq!(submissions[0].light, render::BlockEntityLight::Scalar(1.0));
    }

    #[test]
    fn full_submission_budget_does_not_sample_beam_light() {
        let mut submissions = vec![
            BlockEntitySubmission {
                block: [0; 3],
                light: 1.0.into(),
                kind: BlockEntityKind::EndPortal,
            };
            super::super::MAX_SUBMISSIONS
        ];
        submit(
            &mut submissions,
            [beam(Vec3::ZERO, Vec3::ZERO)],
            None,
            |_, _| panic!("a full submission budget cannot consume owner light samples"),
        );
        assert_eq!(submissions.len(), super::super::MAX_SUBMISSIONS);
    }
}
