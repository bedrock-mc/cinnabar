//! Immutable server-pack actor artwork shared by compilation and presentation.
use crate::{ActorArtworkBinding, ActorTexture, RuntimeEntityAssets};
use std::sync::Arc;
/// The pack's entity catalog with the artwork of its eligible rigs.
#[derive(Debug)]
pub struct SessionEntityPack {
    pub assets: Arc<RuntimeEntityAssets>,
    pub textures: Arc<[ActorTexture]>,
    pub bindings: Arc<[ActorArtworkBinding]>,
    /// The pack's attachable bindings and rasters for held and worn items.
    pub equipment: Option<Arc<crate::RuntimeEquipmentCatalog>>,
}
