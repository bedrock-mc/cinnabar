//! Distant terrain level of detail: an opt-in Cinnabar extension, not vanilla behaviour.
//!
//! Terrain the player has seen is kept as 32³-voxel sections at levels `L0..=L6`, where a
//! level-`L` voxel spans `2^L` blocks. This crate owns the data model and the pure
//! transforms on it; it reads no clock, files or runtime assets. Callers supply block
//! classes ([`classify::BlockClassifier`], [`classify::MaterialClasses`]) resolved from
//! their own assets. Each item has one path, under its owning module.
//!
//! - [`geometry`]: section positions, persistent keys, octants and voxel boxes.
//! - [`bit_field`], [`voxel`]: named word fields and the 32-bit voxel word layout, the
//!   single source for shader constants.
//! - [`storage`], [`biome_grid`], [`section`]: packed and dense sections, biome grids and
//!   tri-state child existence.
//! - [`sampler`]: level-0 sampling of a chunk column.
//! - [`classify`]: block classes and the caller-supplied classifier traits.
//! - [`mip_policy`], [`mip`]: the 2×2×2 policy, the local L0..L4 chain and full re-mips.
//! - [`exposure`]: neighbour face planes the mip reads to judge exposure.
//! - [`ingest`]: merging sampled columns into the stored sections they share.
//! - [`registry`]: the persistent block-state key → material id table.
//! - [`codec`]: checksummed, compressed section records.
//! - [`epoch`]: surface signatures and voting that tell worlds behind one server apart.
//! - [`mesh`]: section meshes of greedy quads in 64-quad clusters, with crack border lists,
//!   and the bounded cache of meshes a renderer wants.
//! - [`colour`]: flat face colours per material and biome tints per tint class.
//! - [`select`]: which sections to draw for a view, and the swap from one selection to
//!   the next without holes.
//! - [`draw`]: what a renderer uploads: cluster and section records, packed biome grids,
//!   crack-list choices and the translucent section order.

pub mod biome_grid;
pub mod bit_field;
pub mod classify;
pub mod codec;
pub mod colour;
pub mod draw;
pub mod epoch;
pub mod exposure;
#[cfg(any(test, feature = "test-support"))]
pub mod fixtures;
pub mod geometry;
pub mod ingest;
pub mod mesh;
pub mod mip;
pub mod mip_policy;
pub mod registry;
pub mod sampler;
pub mod section;
pub mod select;
pub mod storage;
pub mod voxel;
