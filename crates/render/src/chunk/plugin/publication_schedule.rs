use bevy::{ecs::schedule::Schedule, prelude::*, render::RenderSystems};

/// The queues capture allocation identities and water-ref ranges. Publish all
/// geometry and sort updates before either queue, rather than replacing those
/// ranges in Bevy's later PrepareResources stage after they have been queued.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum ChunkPublicationStage {
    CoverageBegin,
    FarTiles,
    Attributes,
    Geometry,
    Coverage,
    LiquidSort,
    ModelSort,
}

/// Publishes geometry and deferred admission changes before every terrain draw queue.
pub(in crate::chunk) fn configure_chunk_publication(schedule: &mut Schedule) {
    schedule.configure_sets(
        (
            ChunkPublicationStage::CoverageBegin,
            ChunkPublicationStage::FarTiles,
            ChunkPublicationStage::Attributes,
            ChunkPublicationStage::Geometry,
            ChunkPublicationStage::Coverage,
            ChunkPublicationStage::LiquidSort,
            ChunkPublicationStage::ModelSort,
        )
            .chain()
            .after(RenderSystems::PrepareViews)
            .before(RenderSystems::Queue),
    );
}

#[cfg(test)]
mod tests;
