//! Fixture builders and semantic observations for other crates' tests. Wire layouts stay
//! private to this crate; callers build and observe packets through owned values only.

pub mod items;
pub mod packets;
pub mod recipes;
pub mod remote_equipment;
pub mod startup;
pub mod transactions;
