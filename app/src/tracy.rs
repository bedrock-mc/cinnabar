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
