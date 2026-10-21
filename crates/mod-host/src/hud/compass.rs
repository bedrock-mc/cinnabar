//! Current-session compass poses are native observations, never guest-authored geometry.

use crate::{MotionObservation, WorldObservation};
use ui::mod_hud::{CompassFrame, Hud};

const _: () = assert!(ui::mod_hud::MAX_COMPASS_COORDINATE == mod_api::MAX_PRIMITIVE_COORDINATE);

/// Refreshes only poses whose authored stamp matches both live native observations.
pub(super) fn hydrate(
    hud: &mut Hud,
    world: Option<&WorldObservation>,
    motion: Option<&MotionObservation>,
) {
    for card in &mut hud.cards {
        let Some(compass) = &mut card.compass else {
            continue;
        };
        compass.frame = None;
        if card.sample {
            compass.frame = Some(CompassFrame {
                heading: 45.,
                position: [0., 64., 0.],
            });
            continue;
        }
        let (Some(world), Some(motion)) = (world, motion) else {
            continue;
        };
        if compass.session != world.session
            || compass.dimension != world.dimension
            || world.session != motion.session
            || world.dimension != motion.dimension
        {
            continue;
        }
        let heading = mod_api::bedrock_view_angles(motion.yaw, motion.pitch)[0];
        let position = [world.position.0, world.position.1, world.position.2];
        if heading.is_finite()
            && position
                .iter()
                .all(|p| p.is_finite() && p.abs() <= mod_api::MAX_PRIMITIVE_COORDINATE)
        {
            compass.frame = Some(CompassFrame { heading, position });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::{State, set_content, sources};
    use super::*;
    use crate::ModGrants;

    /// Supplies a real session stamp without a server, clock or component callback.
    fn observations() -> (WorldObservation, MotionObservation) {
        (
            WorldObservation {
                session: 1,
                world: "fixture".into(),
                dimension: 0,
                position: (1., 64., 2.),
                alive: true,
            },
            MotionObservation {
                session: 1,
                dimension: 0,
                yaw: 180.,
                pitch: 0.,
                velocity: (0., 0., 0.),
                on_ground: true,
            },
        )
    }

    /// Native refresh follows turns without another guest callback and rejects missing grants.
    #[test]
    fn native_compass_tracks_rotation_and_clears_stale_session_poses() {
        let mut state = State::new(
            ModGrants {
                hud: true,
                ..Default::default()
            },
            String::new(),
            Default::default(),
        );
        let json = r#"{"cards":[{"id":"compass","rows":[],"width":280,"compass":{"session":1,"dimension":0}}]}"#;
        assert!(set_content(&mut state, json.into()).unwrap().is_err());
        state.grants.player_state = true;
        set_content(&mut state, json.into()).unwrap().unwrap();
        state.hud.commit();
        let (world, motion) = observations();
        state.player_state.world = Some(world.clone());
        state.player_state.observations.motion = Some(motion);
        sources::refresh(&mut state);
        let frame = state.hud.resolved.as_ref().unwrap().cards[0]
            .compass
            .as_ref()
            .unwrap()
            .frame
            .as_ref()
            .unwrap();
        assert_eq!(frame.heading, 0.);
        assert_eq!(frame.position, [1., 64., 2.]);
        state.player_state.observations.motion.as_mut().unwrap().yaw = 90.;
        sources::refresh(&mut state);
        assert_eq!(
            state.hud.resolved.as_ref().unwrap().cards[0]
                .compass
                .as_ref()
                .unwrap()
                .frame
                .as_ref()
                .unwrap()
                .heading,
            90.
        );
        assert!(
            state.hud.content.as_ref().unwrap().cards[0]
                .compass
                .as_ref()
                .unwrap()
                .frame
                .is_none()
        );
        for (session, dimension) in [(2, 0), (1, 1)] {
            state.player_state.world = Some(WorldObservation {
                session,
                dimension,
                ..world.clone()
            });
            sources::refresh(&mut state);
            assert!(
                state.hud.resolved.as_ref().unwrap().cards[0]
                    .compass
                    .as_ref()
                    .unwrap()
                    .frame
                    .is_none()
            );
        }
        state.player_state.world = Some(world);
        state.player_state.observations.motion = None;
        sources::refresh(&mut state);
        assert!(
            state.hud.resolved.as_ref().unwrap().cards[0]
                .compass
                .as_ref()
                .unwrap()
                .frame
                .is_none()
        );
    }
}
