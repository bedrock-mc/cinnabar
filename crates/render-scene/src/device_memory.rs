//! Graphics memory of the adapter the renderer selected.

use bevy::{prelude::Resource, render::extract_resource::ExtractResource};

/// Dedicated memory of the selected adapter in bytes, 0 when the backend does not report it
/// (integrated and unified-memory devices). Published by the shell once the renderer has
/// chosen its adapter; renderers size their device-memory budgets from it.
#[derive(Resource, ExtractResource, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[extract_app(bevy::render::RenderApp)]
pub struct DedicatedGraphicsMemory(pub u64);
