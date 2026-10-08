use bevy::{ecs::schedule::Schedule, prelude::*, render::RenderSystems};

/// The queues capture allocation identities and water-ref ranges. Publish all
/// geometry and sort updates before either queue, rather than replacing those
/// ranges in Bevy's later PrepareResources stage after they have been queued.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum ChunkPublicationStage {
    Attributes,
    Geometry,
    LiquidSort,
    ModelSort,
}

pub(super) fn configure_chunk_publication(schedule: &mut Schedule) {
    schedule.configure_sets(
        (
            ChunkPublicationStage::Attributes,
            ChunkPublicationStage::Geometry,
            ChunkPublicationStage::LiquidSort,
            ChunkPublicationStage::ModelSort,
        )
            .chain()
            .after(RenderSystems::ManageViews)
            .before(RenderSystems::Queue),
    );
}

#[cfg(test)]
mod tests;
