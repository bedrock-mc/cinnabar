//! What the actor adapter draws from, borrowed from the owning renderers' resources each
//! frame: rig sources (the world's actors and the first-person hand, which share the actor
//! rig's packed buffers) and the block-entity vertex lists.

use bevy::math::Vec3;
use bevy::render::render_resource::{Buffer, Texture};
use render_model::ActorGpuInstance;
use render_scene::shaderpack::ShaderPackActorIds;

use super::skin::SkinKernels;

/// The storage buffers a rig is pulled from, in the actor pass's group 0 order.
#[derive(Clone, Copy)]
pub(crate) struct RigBuffers<'a> {
    pub instances: &'a Buffer,
    pub vertices: &'a Buffer,
    pub spans: &'a Buffer,
    pub previous_bones: &'a Buffer,
    pub current_bones: &'a Buffer,
}

/// One layer of a texture: a page of actor artwork, a player-skin class, an item atlas, or
/// layer 0 of a plain 2D texture.
#[derive(Clone, Copy)]
pub(crate) struct LayerTexture<'a> {
    pub texture: &'a Texture,
    pub layer: u32,
}

/// Which hand a first-person item instance is held in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Hand {
    Main,
    Off,
}

/// One rig instance as its renderer draws it.
#[derive(Clone, Copy)]
pub(crate) struct RigInstance<'a> {
    /// Its index in the source's instance buffer.
    pub index: u32,
    pub data: &'a ActorGpuInstance,
    /// Vertices of its geometry span (whole triangles).
    pub vertex_count: u32,
    pub texture: LayerTexture<'a>,
    /// The actor it belongs to, for `entityId`.
    pub runtime_id: Option<u64>,
    /// Drawn by the camera; shadow-only bodies (the local player in first person) are not.
    pub camera: bool,
    /// Block and sky light, `0..=15` each.
    pub light: [u8; 2],
    /// Blended, overriding the material's state (first-person items).
    pub blend: Option<bool>,
    /// The hand holding it, for first-person item instances.
    pub held: Option<Hand>,
}

/// A renderer whose actor-rig instances the pack draws.
pub(crate) trait RigSource {
    fn buffers(&self) -> RigBuffers<'_>;
    /// Calls `visit` with every instance that can be drawn, in instance order.
    fn for_each_instance(&self, visit: &mut dyn FnMut(RigInstance<'_>));
    /// The enchantment-glint raster of actors (`item == false`) or of held items.
    fn glint(&self, item: bool) -> Option<&Texture>;
}

/// One uploaded block-entity vertex list (`render_model::BlockEntityVertex` triangles).
#[derive(Clone, Copy)]
pub(crate) struct BlockEntityList<'a> {
    pub buffer: &'a Buffer,
    pub count: u32,
}

/// The block-entity layers the pack draws, all textured from one atlas.
#[derive(Clone, Copy)]
pub(crate) struct BlockEntitySource<'a> {
    pub atlas: &'a Texture,
    /// Alpha-tested models: `gbuffers_block`, and `shadow_block`.
    pub solid: Option<BlockEntityList<'a>>,
    /// Blended layers: `gbuffers_block_translucent`.
    pub overlay: Option<BlockEntityList<'a>>,
}

/// Everything the actor adapter reads for one view and frame.
#[derive(Clone, Copy, Default)]
pub(crate) struct ActorSources<'a> {
    pub kernels: Option<SkinKernels<'a>>,
    pub actors: Option<&'a dyn RigSource>,
    pub hand: Option<&'a dyn RigSource>,
    pub block_entities: Option<BlockEntitySource<'a>>,
    /// The pack's `entityId` of each rendered actor.
    pub entity_ids: Option<&'a ShaderPackActorIds>,
    /// `heldItemId` and `heldItemId2`, which first-person items render as.
    pub held_items: [i32; 2],
    /// World-space camera position; actor vertices are written relative to it.
    pub camera: Vec3,
    /// Camera forward direction for sorting blended instances.
    pub view_forward: Vec3,
    /// Seconds since the client started, which the glint texture matrix scrolls by.
    pub seconds: f32,
}
