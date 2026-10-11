use bevy::prelude::Resource;

/// Bevy adapter for the generation-bound admitted server stack.
#[derive(Debug, Default, Resource, bevy::prelude::Deref, bevy::prelude::DerefMut)]
pub struct ResourcePackAdmissionState(pack_runtime::ResourcePackAdmissionState);
