//! App adapters for the optional acceptance plugin.
#[cfg(not(feature = "acceptance"))]
mod disabled;
#[cfg(not(feature = "acceptance"))]
pub(crate) use disabled::AcceptanceRun;
#[cfg(feature = "acceptance")]
pub(crate) mod model_witness;
#[cfg(feature = "acceptance")]
pub(crate) mod world_ready;
