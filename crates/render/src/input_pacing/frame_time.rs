//! Supplies simulation time from the current update rather than the previous presentation.

use bevy::time::{TimeReceiver, TimeSender, TimeSystems, create_time_channels};

use super::*;

#[derive(Resource)]
struct FrameTime {
    rendered: TimeReceiver,
    admitted: TimeSender,
}

/// Replaces the renderer's time source while keeping its bounded channel serviced.
pub(super) fn install(app: &mut App) {
    let Some(rendered) = app.world_mut().remove_resource::<TimeReceiver>() else {
        return;
    };
    let (admitted, receiver) = create_time_channels();
    app.insert_resource(FrameTime { rendered, admitted })
        .insert_resource(receiver)
        .add_systems(First, sample_frame_time.before(TimeSystems));
}

/// Publishes current monotonic time; Bevy still owns virtual time and manual recording clocks.
fn sample_frame_time(pacer: Res<InputPacer>, clock: Res<FrameTime>, receiver: Res<TimeReceiver>) {
    while clock.rendered.0.try_recv().is_ok() {}
    while receiver.0.try_recv().is_ok() {}
    clock
        .admitted
        .0
        .try_send(pacer.clock.now())
        .expect("the frame time channel was drained");
}
