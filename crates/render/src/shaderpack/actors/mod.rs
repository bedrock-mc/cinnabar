//! Actors, block entities and the first-person hand for shader packs: Cinnabar's actor rig
//! skinned into the Iris vertex stream and drawn with the pack's `gbuffers_entities`,
//! `gbuffers_entities_translucent`, `gbuffers_spidereyes`, `gbuffers_armor_glint`,
//! `gbuffers_block`, `gbuffers_block_translucent`, `gbuffers_beaconbeam`, `gbuffers_hand`,
//! `gbuffers_hand_water`, `shadow_entities` and `shadow_block` programs (each through Iris's
//! fallback chain, see [`super::draws::DrawKind`]).
//!
//! # Skinning
//! Each frame one compute pass ([`skin`], `skin.wesl`) poses every drawn rig instance with
//! the actor pass's own skinning (`render::actor_rig`) and writes one Iris vertex per rig
//! vertex: positions relative to the camera (the hand's in view space), world normals, the
//! instance's light as Java lightmap coordinates, its dye as the vertex colour, and an
//! explicit reversed back face for every two-sided triangle, so pack pipelines cull back
//! faces as Java's entity render types do. Block-entity vertex lists go through a second
//! entry point of the same kernel. Both stream into device-sized pages that every
//! actor draw (camera and shadow) reads; the geometry adapter's constant stream supplies
//! the attributes actors do not carry, as it does for terrain.
//!
//! # Draws
//! A rig instance is one draw per pass it appears in. Its texture layer is bound as
//! `gtexture` through a single-layer, non-sRGB view of Cinnabar's actor artwork (raw gamma
//! bytes, as Java textures read), with neutral LabPBR `normals`/`specular`. Iris's per-entity
//! uniforms are patched into the draw's own copy of the draw kind's uniform block:
//! `entityId` (the actor's Bedrock identifier through `entity.properties`), `entityColor`
//! (the hurt/creeper-flash overlay), `blockEntityId` and `currentRenderedItemId` (the held
//! item's `item.properties` id for first-person items). Glinted instances add a
//! `gbuffers_armor_glint` draw with sprite-local UVs, configured strength and scroll, the
//! glint raster and Java's texture rotation and scale. Block entities use
//! `mc_Entity`/`blockEntityId` -1: Cinnabar merges their
//! meshes without the source block state.
//!
//! # Not drawn by the pack
//! Entities with the glowing effect (`gbuffers_entities_glowing`; Cinnabar has no outline
//! state) and block-entity portals stay in the Vanilla scene and reach the pack through the
//! overlay. Lightning, block cracks and the selection outline are effects (`super::effects`).

mod classify;
pub(super) mod compatibility;
mod frame;
#[cfg(test)]
mod gpu_tests;
pub(super) mod layout;
mod pages;
mod skin;
mod sources;
#[cfg(test)]
mod tests;
mod view;

pub(crate) use classify::hand_projection;
pub(crate) use compatibility::instance_supported;
pub(crate) use frame::{prepare, record};
pub(crate) use skin::{ActorSkinPipelines, load_shaders, required_storage_buffers};
pub(crate) use sources::{
    ActorSources, BlockEntityList, BlockEntitySource, Hand, LayerTexture, RigBuffers, RigInstance,
    RigSource,
};
pub(crate) use view::ViewActors;
