//! The render world's single non-blocking device poll, after every submission of the frame.
//!
//! A poll takes the device's fence and lifetime locks that submits and buffer writes also need,
//! so owners never poll themselves: completion and map callbacks fire here, and readers consume
//! what arrived by the previous frame's poll.

use bevy::{
    prelude::*,
    render::{
        Render, RenderSystems,
        render_resource::PollType,
        renderer::{RenderDevice, RenderQueue, render_system},
    },
};

/// Owners whose systems register completion callbacks or request readbacks after the graph.
#[derive(SystemSet, Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct FrameSubmissions;

/// Polls issued so far; tests read it to prove there is exactly one per frame.
#[derive(Resource, Default, Debug)]
pub(crate) struct DevicePolls(pub(crate) u64);

/// Idempotent, so every owner that relies on callbacks installs it.
pub(crate) fn install(render_app: &mut SubApp) {
    if render_app.world().contains_resource::<DevicePolls>() {
        return;
    }
    render_app
        .init_resource::<DevicePolls>()
        .configure_sets(
            Render,
            FrameSubmissions
                .in_set(RenderSystems::Render)
                .after(render_system),
        )
        .add_systems(
            Render,
            poll_device
                .in_set(RenderSystems::Render)
                .after(FrameSubmissions),
        );
}

/// Runs callback from a device poll after all submissions so far, including this frame, complete.
/// Call from a FrameSubmissions system; registering it requires no extra submit.
pub(crate) fn on_frame_complete(queue: &RenderQueue, callback: impl FnOnce() + Send + 'static) {
    queue.on_submitted_work_done(callback);
}

/// Reads a reserved queue index with an empty submit; callback registration may reserve one too.
#[cfg(test)]
pub(crate) fn submission_marker(queue: &RenderQueue) -> u64 {
    let index = format!("{:?}", queue.submit([]));
    index
        .chars()
        .filter(char::is_ascii_digit)
        .collect::<String>()
        .parse()
        .expect("submission index")
}

fn poll_device(device: Res<RenderDevice>, mut polls: ResMut<DevicePolls>) {
    #[cfg(feature = "tracy")]
    let _span = bevy::log::info_span!("render.device_poll").entered();
    if let Err(error) = device.poll(PollType::Poll) {
        warn!(?error, "could not nonblockingly poll the render device");
    }
    polls.0 += 1;
}
