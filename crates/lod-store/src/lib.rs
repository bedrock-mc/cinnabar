//! On-disk cache of distant terrain: an opt-in Cinnabar extension, not vanilla behaviour.
//!
//! Sections from [`lod`] are kept in region files ([`region_file`]), one per level and
//! [`region_id::REGION_SIDE_SECTIONS`]² group of section columns, with each level-0 file
//! also holding the content hash of its chunks. A [`store::Store`] is one destination
//! world's directory under a [`root::StoreRoot`]: a lock, the material registry, world
//! epochs with their surface signatures, an access table, and per epoch a
//! [`region_set::RegionSet`], with open files bounded across the store. A
//! [`actor::StoreActor`] thread is a store's only writer. The store is a cache that can
//! be rebuilt from chunks: corrupt data is reported as typed errors and events, never
//! panics, and commits are not fsynced (see [`region_file`] and [`store`] for the
//! durability contract). This crate reads no clock; callers supply every time value as a
//! [`minute::Minute`].
//!
//! - [`identity`]: store keys, their directory names and dimension directories.
//! - [`root`]: the versioned root of all stores, quotas and clearing the cache.
//! - [`store`]: one store, its sessions, epoch votes and far-region eviction.
//! - [`actor`]: the write-back thread with bounded, coalescing queues.
//! - [`lock`]: the cross-process writer, reader and removal locks.
//! - [`epochs`], [`access`], [`signatures`]: the store's catalog, access table and
//!   surface signature files.
//! - [`region_set`], [`region_file`], [`region_index`], [`frame`], [`region_id`]: region
//!   files, their index, frames and identity.
//! - [`table`]: the checked envelope and atomic replacement of small table files.
//! - [`error`]: region and store errors and recovery reports.

pub mod access;
pub mod actor;
pub mod epochs;
pub mod error;
pub mod frame;
pub mod identity;
pub mod lock;
pub mod minute;
mod positional;
pub mod region_file;
pub mod region_id;
pub mod region_index;
pub mod region_set;
pub mod root;
pub mod signatures;
pub mod store;
pub mod table;
#[cfg(test)]
mod test_support;
