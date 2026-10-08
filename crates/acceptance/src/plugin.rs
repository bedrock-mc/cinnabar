//! Optional acceptance resources installed by app composition.
use crate::{
    AcceptanceRun, audio_wire::WireEvidence, model_witness::ModelWitnessFileSource,
    transparent_witness::TransparentWitnessFileSource,
};
use bevy::prelude::{App, Plugin};
use std::path::PathBuf;

/// Configures the evidence adapters. Runtime composition supplies their explicit ordering.
pub struct AcceptancePlugin {
    pub seconds: Option<u64>,
    pub metrics_out: Option<PathBuf>,
    pub full_view_teleport_gate: bool,
    pub require_transparent_presentation: bool,
    pub transparent_witness_request: Option<PathBuf>,
    pub model_witness_request: Option<PathBuf>,
}

impl Plugin for AcceptancePlugin {
    /// Installs acceptance state without acquiring network or gameplay ownership.
    fn build(&self, app: &mut App) {
        app.insert_resource(AcceptanceRun::new(
            self.seconds,
            self.metrics_out.clone(),
            self.full_view_teleport_gate,
            self.require_transparent_presentation,
        ))
        .insert_resource(TransparentWitnessFileSource::new(
            self.transparent_witness_request.clone(),
        ))
        .insert_resource(ModelWitnessFileSource::new(
            self.model_witness_request.clone(),
        ))
        .init_resource::<WireEvidence>();
    }
}
