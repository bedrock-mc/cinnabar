//! App adapters for the optional acceptance plugin.
#[cfg(not(feature = "acceptance"))]
mod disabled;
#[cfg(all(test, feature = "acceptance"))]
pub(crate) use ::acceptance::{
    AcceptanceExitDecision, Phase3TerminalDrainDecision, TRANSPARENT_PRESENTATION_EXIT_GRACE,
    proofs, remesh, teleport,
};
#[cfg(feature = "acceptance")]
pub(crate) use ::acceptance::{AcceptanceRun, mutation, transparent_witness};
pub(crate) use diagnostics::markers;
#[cfg(not(feature = "acceptance"))]
pub(crate) use disabled::AcceptanceRun;
#[cfg(feature = "acceptance")]
pub(crate) mod model_witness;
#[cfg(feature = "acceptance")]
pub(crate) mod world_ready;
#[cfg(not(feature = "acceptance"))]
pub(crate) mod mutation {
    pub(crate) use diagnostics::write_stdout_marker;
}
