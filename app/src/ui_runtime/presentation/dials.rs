//! Advances the local player's compass and clock needles once per tick and publishes the
//! strip frames every dial icon shows.

use bevy::{
    ecs::system::SystemParam,
    math::EulerRot,
    prelude::{Local, Res, ResMut},
    time::{Real, Time},
};
use client_shell::ui_resources::UiPresentationResource;
use inventory::{ClockReading, CompassReading, DialFrames, DialNeedle};

use crate::runtime::world::ClientWorld;

/// The local player's dial needles, the tick they last advanced on, and the random state
/// their spin draws from.
#[derive(Default)]
pub(crate) struct DialNeedles {
    compass: DialNeedle,
    recovery_compass: DialNeedle,
    clock: DialNeedle,
    advanced_tick: Option<u64>,
    random: u64,
}

impl DialNeedles {
    /// Takes one reading for an observed completed tick, regardless of its numeric distance
    /// from the previous observation. Server reanchors do not represent simulated ticks.
    fn advance_tick(
        &mut self,
        tick: u64,
        update: impl FnOnce(&mut Self) -> DialFrames,
    ) -> Option<DialFrames> {
        if self.advanced_tick == Some(tick) {
            return None;
        }
        self.advanced_tick = Some(tick);
        Some(update(self))
    }

    /// A uniform draw in `0..1` from a xorshift sequence.
    fn next_random(&mut self) -> f32 {
        let mut state = self.random.max(1);
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        self.random = state;
        (state >> 40) as f32 / (1u64 << 24) as f32
    }

    /// Advances every needle one tick for a holder at `position` facing `yaw` degrees in
    /// `dimension`, under the world spawn, the holder's last death (block and dimension) and
    /// the sky's `celestial_angle`.
    fn advance(
        &mut self,
        dimension: i32,
        world_spawn: [i32; 3],
        last_death: Option<([i32; 3], i32)>,
        position: [f32; 3],
        yaw: f32,
        celestial_angle: f32,
    ) -> DialFrames {
        let natural = inventory::natural_dimension(dimension);
        let toward = |target: [i32; 3]| CompassReading::Target {
            target: [target[0], target[2]],
            position: [position[0], position[2]],
            yaw,
        };
        let compass = if natural {
            toward(world_spawn)
        } else {
            CompassReading::Spin {
                random: self.next_random(),
            }
        };
        let recovery_compass = match last_death {
            Some((block, death_dimension)) if death_dimension == dimension => toward(block),
            _ => CompassReading::Spin {
                random: self.next_random(),
            },
        };
        let clock = if natural {
            ClockReading::Sky { celestial_angle }
        } else {
            ClockReading::Spin {
                random: self.next_random(),
            }
        };
        DialFrames {
            compass: Some(self.compass.update_compass(compass, false)),
            recovery_compass: Some(
                self.recovery_compass
                    .update_compass(recovery_compass, false),
            ),
            clock: Some(self.clock.update_clock(clock, false)),
        }
    }
}

/// Borrows the current local holder and the UI and equipment destinations for its readings.
#[derive(SystemParam)]
pub(crate) struct DialResources<'w> {
    client_world: Res<'w, ClientWorld>,
    physics: Res<'w, client_interaction::movement::LocalPhysicsController>,
    view: Res<'w, client_presentation::local_player::LocalViewPose>,
    clock: Res<'w, client_presentation::environment::WorldClock>,
    time: Res<'w, Time<Real>>,
    presentation: ResMut<'w, UiPresentationResource>,
    equipment: Option<ResMut<'w, client_presentation::presentation::equipment::EquipmentRuntime>>,
}

/// Advances needles for completed physics observations before hands are prepared.
/// Without a local player, dials keep their static icons.
pub(crate) fn advance_dials(
    resources: DialResources,
    mut needles: Local<DialNeedles>,
    mut frames: Local<DialFrames>,
) {
    let DialResources {
        client_world,
        physics,
        view,
        clock,
        time,
        mut presentation,
        equipment,
    } = resources;
    let local = client_world.stream.as_ref().and_then(|stream| {
        let authority = stream.authority();
        let actor = authority.actor(stream.local_player_runtime_id())?;
        let last_death = actor
            .last_death()
            .map(|(position, dimension)| (position.map(|axis| axis.floor() as i32), dimension));
        Some((
            authority.current_dimension(),
            authority.world_spawn(),
            last_death,
        ))
    });
    let Some((dimension, world_spawn, last_death)) = local else {
        *needles = DialNeedles::default();
        *frames = DialFrames::default();
        publish(&mut presentation, equipment, *frames);
        return;
    };
    if physics
        .state()
        .zip(needles.advanced_tick)
        .is_some_and(|(state, after)| state.tick < after)
    {
        needles.advanced_tick = None;
    }
    let celestial_angle = render_model::celestial_angle(
        client_presentation::environment::visual_world_time(*clock, time.elapsed_secs_f64()),
    );
    let (yaw, _, _) = view.camera_rotation().to_euler(EulerRot::YXZ);
    let yaw = (180.0 - yaw.to_degrees()).rem_euclid(360.0);
    let after = needles.advanced_tick;
    physics.visit_completed_ticks(after, &mut |sample, _, _| {
        if let Some(next) = needles.advance_tick(sample.tick, |needles| {
            needles.advance(
                dimension,
                world_spawn,
                last_death,
                sample.position,
                yaw,
                celestial_angle,
            )
        }) {
            *frames = next;
        }
    });
    publish(&mut presentation, equipment, *frames);
}

/// Hands `frames` to the UI icons and held items.
fn publish(
    presentation: &mut UiPresentationResource,
    equipment: Option<ResMut<client_presentation::presentation::equipment::EquipmentRuntime>>,
    frames: DialFrames,
) {
    presentation.set_dial_frames(frames);
    if let Some(mut equipment) = equipment {
        equipment.set_dial_frames(frames);
    }
}

#[cfg(test)]
mod tests;
