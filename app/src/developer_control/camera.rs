//! The scripted cinematic camera: overrides the rendered camera after every player and
//! server camera writer, leaving the player's own view and physics untouched.

use bevy::prelude::*;
use developer_control::camera::CameraPath;
use serde_json::{Value, json};

use crate::{
    app::ClientFrameSet,
    camera::{FlyCamera, projection_fov_radians},
    runtime::telemetry::bedrock_camera_rotation,
};

#[derive(Resource)]
pub(super) struct ScriptedCamera {
    path: CameraPath,
    /// Game time at the first sampled frame.
    started: Option<f32>,
    elapsed: f32,
    /// The hand is hidden by an in-memory override, lifted on release.
    hid_hand: bool,
}

/// The vanilla video option the cinematic camera borrows to hide the hand.
const HIDE_HAND: &str = "hide_hand";

impl ScriptedCamera {
    pub(super) fn finished(&self) -> bool {
        self.path.finished(self.elapsed)
    }

    pub(super) fn summary(&self) -> Value {
        json!({
            "elapsed": self.elapsed,
            "duration": self.path.duration(),
            "looping": self.path.looping,
            "finished": self.finished(),
        })
    }
}

pub(super) fn configure(app: &mut App) {
    app.add_systems(
        Update,
        drive_camera
            .after(ClientFrameSet::Camera)
            .before(ClientFrameSet::Interaction),
    );
}

pub(super) fn start(world: &mut World, path: CameraPath) -> Result<Value, String> {
    path.validate()?;
    let duration = path.duration();
    let previously_hid = world
        .remove_resource::<ScriptedCamera>()
        .is_some_and(|previous| previous.hid_hand);
    let hid_hand = path.hide_hand;
    if previously_hid != hid_hand {
        set_hand_hidden(world, hid_hand);
    }
    world.insert_resource(ScriptedCamera {
        path,
        started: None,
        elapsed: 0.0,
        hid_hand,
    });
    Ok(json!({ "duration": duration }))
}

pub(super) fn release(world: &mut World) -> Result<Value, String> {
    let Some(scripted) = world.remove_resource::<ScriptedCamera>() else {
        return Ok(json!({ "released": false }));
    };
    if scripted.hid_hand {
        set_hand_hidden(world, false);
    }
    Ok(json!({ "released": true }))
}

/// Never saved: the override lives only in this process's settings snapshot.
fn set_hand_hidden(world: &mut World, hidden: bool) {
    if let Some(mut menu) = world.get_resource_mut::<crate::menu::MenuRuntime>() {
        menu.set_session_option(HIDE_HAND, hidden.then_some(1));
    }
}

/// Game time drives sampling, so a fixed-clock recording plays the path frame-exactly.
fn drive_camera(
    time: Res<Time>,
    scripted: Option<ResMut<ScriptedCamera>>,
    mut cameras: Query<(&mut Transform, &mut Projection), With<FlyCamera>>,
) {
    let Some(mut scripted) = scripted else {
        return;
    };
    let now = time.elapsed_secs();
    let started = *scripted.started.get_or_insert(now);
    scripted.elapsed = now - started;
    let Some(sample) = scripted.path.sample(scripted.elapsed) else {
        return;
    };
    for (mut transform, mut projection) in &mut cameras {
        *transform = Transform::from_translation(Vec3::from_array(sample.position))
            .with_rotation(bedrock_camera_rotation(sample.yaw, sample.pitch));
        if let (Some(fov), Projection::Perspective(perspective)) = (sample.fov, &mut *projection) {
            perspective.fov = projection_fov_radians(fov);
        }
    }
}
