use bevy::prelude::Vec3;
use client_world::DragonDeathView;
use render::{BlockEntityKind, BlockEntityLight, BlockEntitySubmission, DragonDeathModel};

pub(super) fn update_without_atlas(
    stream: Option<&chunk_pipeline::WorldStream>,
    partial_tick: f32,
    camera: Option<Vec3>,
    scene: &mut render::BlockEntityScene,
    frame: &mut render::BlockEntityFrame,
) {
    let mut submissions = Vec::new();
    if let Some(stream) = stream {
        submit(
            &mut submissions,
            stream.authority().dragon_death_rays(partial_tick),
            camera,
            |runtime, position| {
                let actor = stream.authority().actor(runtime)?;
                stream.solved_light_at(actor.brightness_sample_position(position))
            },
        );
    }
    *frame = scene
        .update(render::SceneClock::default(), &[], &submissions)
        .clone();
}

/// Admission follows the interpolated body before consuming the bounded scene capacity.
pub(super) fn submit(
    submissions: &mut Vec<BlockEntitySubmission>,
    rays: impl IntoIterator<Item = DragonDeathView>,
    camera: Option<Vec3>,
    mut light_at: impl FnMut(u64, [f32; 3]) -> Option<(u8, u8)>,
) {
    for ray in rays {
        if camera.is_some_and(|camera| {
            !client_presentation::presentation::actors::within_actor_candidate_cube(
                ray.owner_position,
                camera.to_array(),
            )
        }) {
            continue;
        }
        if submissions.len() >= super::MAX_SUBMISSIONS {
            break;
        }
        submissions.push(BlockEntitySubmission {
            block: ray.center.map(|value| value.floor() as i32),
            light: light_at(ray.runtime_id, ray.owner_position).map_or_else(
                || 1.0.into(),
                |(block, sky)| BlockEntityLight::Actor { block, sky },
            ),
            kind: BlockEntityKind::DragonDeath(DragonDeathModel {
                center: ray.center,
                death_ticks: u32::from(ray.death_ticks),
                partial_tick: ray.partial_tick,
                seed: ray.seed,
                duration_ticks: ray.duration_ticks,
            }),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ray(position: [f32; 3]) -> DragonDeathView {
        DragonDeathView {
            runtime_id: 7,
            owner_position: position,
            center: position,
            death_ticks: 60,
            partial_tick: 0.5,
            seed: 17,
            duration_ticks: 120.0,
        }
    }

    #[test]
    fn only_candidate_bodies_consume_ray_submission_and_light_capacity() {
        let radius = render::ACTOR_CANDIDATE_RADIUS_BLOCKS;
        let mut submissions = Vec::new();
        let mut samples = Vec::new();
        submit(
            &mut submissions,
            [ray([radius + 1.0; 3]), ray([radius; 3])],
            Some(Vec3::ZERO),
            |id, position| {
                samples.push((id, position));
                Some((4, 11))
            },
        );
        assert_eq!(submissions.len(), 1);
        assert_eq!(samples, [(7, [radius; 3])]);
        assert_eq!(
            submissions[0].light,
            BlockEntityLight::Actor { block: 4, sky: 11 }
        );
        submissions.resize(super::super::MAX_SUBMISSIONS, submissions[0].clone());
        submit(&mut submissions, [ray([0.0; 3])], None, |_, _| {
            panic!("full capacity cannot sample light")
        });
        assert_eq!(submissions.len(), super::super::MAX_SUBMISSIONS);
    }
}
