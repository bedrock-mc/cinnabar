//! Bevy ownership of the session queues and their end-of-frame flush.

use bevy::app::{App, MainScheduleOrder};
use bevy::diagnostic::FrameCount;
use bevy::ecs::schedule::ScheduleLabel;
use bevy::prelude::{
    Deref, DerefMut, IntoScheduleConfigs, Query, Res, ResMut, Resource, Time, Update,
};
use bevy::render::view::ExtractedWindow;
use bevy::render::{
    Extract, ExtractSchedule, Render, RenderApp, RenderSystems,
    renderer::{RenderQueue, render_system},
};
use std::{collections::VecDeque, path::PathBuf};

/// A Bevy resource retaining the domain's exact command and event queues.
#[derive(Resource, Deref, DerefMut)]
pub struct NetworkHandle<P: Send + Sync + 'static> {
    #[deref]
    inner: client_session::NetworkHandle<P>,
    pending_world_ingress: VecDeque<client_session::WorldIngress>,
    core_socket_dir: Option<PathBuf>, // Set only while a joined session is live.
    connected: bool,
    join_profile: bridge::join_profile::JoinProfile,
}

impl<P: Send + Sync + 'static> From<client_session::NetworkHandle<P>> for NetworkHandle<P> {
    fn from(inner: client_session::NetworkHandle<P>) -> Self {
        Self {
            inner,
            pending_world_ingress: VecDeque::new(),
            core_socket_dir: None,
            connected: false,
            join_profile: bridge::join_profile::JoinProfile::current(),
        }
    }
}

impl<P: Send + Sync + 'static> NetworkHandle<P> {
    /// Borrows the session FIFO and the bounded payloads retained by ingress lookahead.
    pub fn world_ingress_mut(
        &mut self,
    ) -> (
        &mut tokio::sync::mpsc::Receiver<client_session::WorldIngress>,
        &mut VecDeque<client_session::WorldIngress>,
    ) {
        (
            self.inner.world_events_mut(),
            &mut self.pending_world_ingress,
        )
    }

    /// Counts every unconsumed event, including payloads retained outside the channel.
    pub fn pending_event_count(&self) -> usize {
        self.inner
            .pending_event_count()
            .saturating_add(self.pending_world_ingress.len())
    }

    /// Adopts the domain queues and the endpoint used by this live session.
    pub fn connected(inner: client_session::NetworkHandle<P>, socket_dir: PathBuf) -> Self {
        let mut handle = Self::from(inner);
        handle.core_socket_dir = Some(socket_dir);
        handle.connected = true;
        handle
    }

    /// Binds event processing to this session's captured diagnostic attempt.
    pub fn enter_join_profile(&self) -> bridge::join_profile::Binding {
        self.join_profile.enter()
    }

    /// Returns the control endpoint only while this handle belongs to a live session.
    pub fn core_socket_dir(&self) -> Option<&std::path::Path> {
        self.core_socket_dir.as_deref()
    }

    /// Adopts a native session with no Go control endpoint.
    pub fn native(inner: client_session::NetworkHandle<P>) -> Self {
        Self {
            connected: true,
            ..Self::from(inner)
        }
    }

    /// Retires the control endpoint and stops the owned network worker.
    pub fn shutdown(&mut self) {
        self.core_socket_dir = None;
        self.connected = false;
        self.inner.shutdown();
    }

    /// Supplies an idle session until the launcher starts a join.
    pub fn disconnected() -> Self {
        client_session::NetworkHandle::disconnected().into()
    }

    /// Provides isolated queues for app adapter tests.
    #[cfg(any(test, feature = "test-support"))]
    pub fn stub() -> (
        Self,
        tokio::sync::watch::Receiver<simulation_clock::identity::PositionAuthorityGeneration>,
    ) {
        let (handle, epoch) = client_session::NetworkHandle::stub();
        (handle.into(), epoch)
    }

    /// Keeps a bounded test queue open until the caller drops its guard.
    #[cfg(any(test, feature = "test-support"))]
    pub fn with_command_capacity(capacity: usize) -> (Self, Box<dyn std::any::Any>) {
        let (handle, guard) = client_session::NetworkHandle::with_command_capacity(capacity);
        (handle.into(), guard)
    }

    /// Lets an app adapter test publish terminal or bootstrap controls.
    #[cfg(any(test, feature = "test-support"))]
    pub fn stub_with_control_sender() -> (
        Self,
        tokio::sync::mpsc::Sender<client_session::NetworkControlEvent<P>>,
    ) {
        let (handle, sender) = client_session::NetworkHandle::stub_with_control_sender();
        (handle.into(), sender)
    }

    /// Captures outbound packets through the domain's production FIFO.
    #[cfg(any(test, feature = "test-support"))]
    pub fn stub_capturing_packets() -> (Self, client_session::CapturedPackets) {
        let (handle, packets) = client_session::NetworkHandle::stub_capturing_packets();
        (handle.into(), packets)
    }
    /// Captures a bounded production FIFO for admission backpressure tests.
    #[cfg(any(test, feature = "test-support"))]
    pub fn stub_capturing_packets_with_capacity(
        capacity: usize,
    ) -> (Self, client_session::CapturedPackets) {
        let (handle, packets) =
            client_session::NetworkHandle::stub_capturing_packets_with_capacity(capacity);
        (handle.into(), packets)
    }
}

/// Submits the current frame's packet batch before waiting for the next frame.
#[derive(ScheduleLabel, Debug, Clone, PartialEq, Eq, Hash)]
pub struct NetworkFrameFlush;

/// Registers the session flush after ordinary frame work and before the pacing wait.
pub fn configure_network_frame_flush<P: Send + Sync + 'static>(app: &mut App) {
    render::FrameEnd::install(app);
    app.init_schedule(NetworkFrameFlush);
    app.world_mut()
        .resource_mut::<MainScheduleOrder>()
        .insert_before(render::FrameEnd, NetworkFrameFlush);
    app.add_systems(NetworkFrameFlush, flush_network_frame::<P>);
    app.add_systems(
        Update,
        advance_network_simulation::<P>
            .after(client_state::ClientFrameSet::Physics)
            .before(client_state::ClientFrameSet::Camera),
    );
}

/// Advances the joined session after physics, including idle and terrain-blocked frames.
fn advance_network_simulation<P: Send + Sync + 'static>(
    time: Option<Res<Time<bevy::time::Real>>>,
    network: Option<Res<NetworkHandle<P>>>,
) {
    if let (Some(time), Some(network)) = (time, network)
        && network.connected
    {
        network.advance_simulation(time.delta());
    }
}

/// Records each extracted frame, and each that reached a window's swap chain, in the latency trace
/// while tracing; needs the render sub-app.
pub(crate) fn trace_presented_frames(app: &mut App) {
    if bridge::latency_trace::active().is_none() && !bridge::join_profile::ENABLED {
        return;
    }
    if let Some(render_app) = app.get_sub_app_mut(RenderApp) {
        render_app
            .init_resource::<HeldSwapChains>()
            .init_resource::<PresentedJoin>()
            .add_systems(ExtractSchedule, trace_frame_extracted)
            .add_systems(
                Render,
                (
                    count_held_swap_chains
                        .in_set(RenderSystems::Render)
                        .before(render_system),
                    trace_frame_presented.in_set(render::FrameSubmissions),
                ),
            );
    }
}

/// Windows holding an acquired swap-chain texture before rendering.
#[derive(Resource, Default)]
struct HeldSwapChains(usize);

/// The attempt whose loading-free UI was extracted for this exact render frame.
#[derive(Resource, Default)]
struct PresentedJoin {
    profile: Option<bridge::join_profile::JoinProfile>,
    hidden: bool,
}

/// Stamps the main frame as the render world extracts it, the count pairing it with its present.
fn trace_frame_extracted(
    frames: Extract<Option<Res<FrameCount>>>,
    mut pending: ResMut<PresentedJoin>,
    windows: Extract<
        Query<&bevy::window::Window, bevy::prelude::With<bevy::window::PrimaryWindow>>,
    >,
) {
    pending.hidden = windows.single().is_ok_and(|window| !window.visible);
    let profile = bridge::join_profile::JoinProfile::current();
    pending.profile = profile.presentation_pending().then_some(profile);
    if let (Some(trace), Some(frames)) = (bridge::latency_trace::active(), frames.as_ref()) {
        trace.record(
            bridge::latency_trace::kind::EXTRACTED,
            &[i64::from(frames.0), trace.now()],
        );
    }
}

fn count_held_swap_chains(windows: Query<&ExtractedWindow>, mut held: ResMut<HeldSwapChains>) {
    held.0 = held_swap_chains(&windows);
}

/// Records the frame only when rendering handed a held swap-chain texture to the window, which
/// presenting takes; a frame without a surface or a skipped present is not shown.
fn trace_frame_presented(
    frames: Option<Res<FrameCount>>,
    windows: Query<&ExtractedWindow>,
    held: Res<HeldSwapChains>,
    queue: Res<RenderQueue>,
    mut pending: ResMut<PresentedJoin>,
) {
    if presented(held.0, held_swap_chains(&windows)) {
        trace_frame(bridge::latency_trace::kind::PRESENTED, frames);
    }
    let was_presented = presented(held.0, held_swap_chains(&windows));
    if (was_presented || pending.hidden)
        && let Some(profile) = pending.profile.take()
    {
        let stage = if pending.hidden {
            "first_world_frame_completed"
        } else {
            "first_world_presented"
        };
        queue.on_submitted_work_done(move || profile.finish(stage));
    }
}

fn held_swap_chains(windows: &Query<&ExtractedWindow>) -> usize {
    windows
        .iter()
        .filter(|window| window.swap_chain_texture.is_some())
        .count()
}

/// Whether presenting released any of the swap-chain textures held before rendering.
fn presented(held_before: usize, held_after: usize) -> bool {
    held_after < held_before
}

fn trace_frame(kind: u8, frames: Option<Res<FrameCount>>) {
    if let (Some(trace), Some(frames)) = (bridge::latency_trace::active(), frames) {
        trace.record(kind, &[i64::from(frames.0), trace.now()]);
    }
}

/// Flushes packets queued during the frame when a network handle is installed.
fn flush_network_frame<P: Send + Sync + 'static>(network: Option<Res<NetworkHandle<P>>>) {
    if let Some(network) = network {
        network.flush_frame();
    }
}

#[cfg(test)]
#[path = "network/tests.rs"]
mod tests;
