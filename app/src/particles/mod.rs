//! App wiring for the particle engine: optional carrier loading, protocol trigger routing, and
//! the per-frame drive that ticks and draws the simulation against the live world.

mod actors;
mod ambient;
mod carrier;
mod drive;
mod tiles;
mod world_adapter;

pub(crate) use carrier::load_optional_carrier;
pub(crate) use drive::{
    ParticleIcons, ParticleInbox, configure_particles, drain_committed_particles,
};
