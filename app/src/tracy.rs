use bevy::{log::BoxedLayer, prelude::*};
use tracing_tracy::{Config, DefaultConfig, TracyLayer};

#[derive(Default)]
struct ZoneConfig(DefaultConfig);

impl Config for ZoneConfig {
    type Formatter = <DefaultConfig as Config>::Formatter;

    /// Retains per-call fields as inspectable zone text.
    fn formatter(&self) -> &Self::Formatter {
        self.0.formatter()
    }

    /// Changing counters must not exhaust Tracy's finite source-location table.
    fn format_fields_in_zone_name(&self) -> bool {
        false
    }
}

/// Uses Bevy's CPU spans without its unsupported Metal GPU calibration path.
pub(crate) fn layer(_app: &mut App) -> Option<BoxedLayer> {
    Some(Box::new(TracyLayer::new(ZoneConfig::default())))
}

/// Appended to the log filter: keeps wgpu's encoding and submission scopes, which
/// `DEFAULT_FILTER`'s `wgpu=error` drops; wgpu's per-call API logging stays at trace.
pub(crate) const WGPU_SCOPE_FILTER: &str = ",wgpu_core=info,wgpu_hal=info";

/// When this frame's local physics finished.
#[derive(Resource, Default)]
pub(crate) struct PhysicsEnd(Option<std::time::Instant>);

pub(crate) fn mark_physics_end(mut end: ResMut<PhysicsEnd>) {
    end.0 = Some(std::time::Instant::now());
}

/// Plots the wait from physics to the auth-input send; a Tracy zone cannot span two systems.
pub(crate) fn plot_physics_to_send(mut end: ResMut<PhysicsEnd>) {
    let (Some(at), Some(client)) = (end.0.take(), tracing_tracy::client::Client::running()) else {
        return;
    };
    client.plot(
        tracing_tracy::client::plot_name!("physics to auth-input send ms"),
        at.elapsed().as_secs_f64() * 1_000.0,
    );
}
