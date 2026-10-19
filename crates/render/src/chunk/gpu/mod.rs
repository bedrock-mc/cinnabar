#[cfg(test)]
mod alpha_edge_tests;
pub(in crate::chunk) mod arena;
mod asset_tables;
#[cfg(feature = "enhanced")]
mod authored_upload;
pub(in crate::chunk) mod bind_groups;
mod capacity;
pub(in crate::chunk) mod graphics_metadata;
pub(in crate::chunk) mod layout;
pub(in crate::chunk) mod resource_geometry;
mod resource_sorts;
pub(in crate::chunk) mod telemetry;
pub(in crate::chunk) mod texture_upload;
pub(in crate::chunk) mod types;
pub(in crate::chunk) mod upload;
