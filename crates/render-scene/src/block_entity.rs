//! Bevy resources for CPU-built block geometry and static item placements.
use bevy::{
    prelude::{Deref, DerefMut, Resource},
    render::extract_resource::ExtractResource,
};

/// Mutable block-model cache owned by the main world.
#[derive(Debug, Default, Resource, Deref, DerefMut)]
pub struct BlockEntitySceneResource(pub render_model::BlockEntityScene);

/// Immutable block vertices extracted for the GPU pass.
#[derive(Clone, Debug, Default, Resource, ExtractResource, Deref, DerefMut)]
#[extract_app(bevy::render::RenderApp)]
pub struct BlockEntityFrameResource(pub render_model::BlockEntityFrame);

/// Selection geometry extracted for the overlay pass.
#[derive(Clone, Debug, Default, Resource, ExtractResource, Deref, DerefMut)]
#[extract_app(bevy::render::RenderApp)]
pub struct BlockSelectionFrameResource(pub render_model::BlockSelectionFrame);

/// Static dropped-item placements published with block-entity models.
#[derive(Clone, Debug, Default, PartialEq, Resource, Deref, DerefMut)]
pub struct StaticItemPlacementsResource(pub render_model::StaticItemPlacements);
