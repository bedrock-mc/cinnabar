mod cache;
mod output;
mod queue;
mod scratch;
mod solve;
mod types;

pub use output::LightSolveOutput;
pub use scratch::LightSolverScratch;
pub use solve::{solve_light, solve_light_with_scratch};
pub use types::{
    BlockPos, BoundaryLightSample, DimensionLightProfile, EmptyLight, LightBlockAccess,
    LightBlockSample, LightBounds, LightProperties, LightReadAccess, LightSolveError,
    LightSolveStats, SolverLimits,
};

#[cfg(test)]
use queue::IncreaseQueue;
#[cfg(test)]
use {
    crate::LightChannel,
    cache::DensePositionSet,
    output::{MutableOutput, MutableOutputScratch},
    solve::{IncreaseEntry, NEIGHBOURS, seed_boundary_from_halo},
    types::light_channel_index,
};

#[cfg(test)]
#[path = "light_solver/boundary_scan_tests.rs"]
mod boundary_scan_tests;

#[cfg(test)]
mod pending_tests;

#[cfg(test)]
mod oracle_tests;
