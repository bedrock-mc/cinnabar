//! Private cache encoding for block overlays and dependency-free transform values.

use assets::{
    Animation, BlockVisual, CompiledBiomeAssets, LightProperties, Material, MaterialOverride,
    ModelQuad, ModelTemplate, TextureArray, TextureRef,
};
use block_transform::random_offset::{RandomOffsetAxis, RandomOffsetComponent};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Serde constructs the public type exhaustively, so new fields require a cache update.
#[derive(Serialize, Deserialize)]
#[serde(remote = "assets::BlockOverlay")]
pub(in crate::runtime::network) struct BlockOverlay {
    visuals: Vec<BlockVisual>,
    carried_cube_faces: Vec<(u32, [u32; 6])>,
    light_properties: Vec<LightProperties>,
    materials: Vec<Material>,
    model_templates: Vec<ModelTemplate>,
    #[serde(with = "random_offsets")]
    model_random_offsets: Vec<(u32, RandomOffsetComponent)>,
    model_quads: Vec<ModelQuad>,
    animations: Vec<Animation>,
    animation_frames: Vec<TextureRef>,
    base_texture_pages: u32,
    textures: Vec<TextureArray>,
    texture_source_sizes: Vec<[u16; 2]>,
    texture_source_grids: Vec<u8>,
    hashes: Vec<Option<u32>>,
    material_overrides: Vec<MaterialOverride>,
    face_overrides: Vec<(u32, [Option<u32>; 6])>,
    biomes: Option<CompiledBiomeAssets>,
    max_page_layers: u32,
}

/// Cache-only representation; the component and axis conversions name every public field.
#[derive(Serialize, Deserialize)]
struct RandomOffset {
    axes: [([f32; 2], u32); 3],
}

impl From<RandomOffsetComponent> for RandomOffset {
    /// Copies each authored range and sample count without resolving a position.
    fn from(component: RandomOffsetComponent) -> Self {
        let RandomOffsetComponent { axes } = component;
        Self {
            axes: axes.map(|RandomOffsetAxis { range, steps }| (range, steps)),
        }
    }
}

impl From<RandomOffset> for RandomOffsetComponent {
    /// Restores every axis, including explicit zero ranges and continuous sampling.
    fn from(offset: RandomOffset) -> Self {
        Self {
            axes: offset
                .axes
                .map(|(range, steps)| RandomOffsetAxis { range, steps }),
        }
    }
}

mod random_offsets {
    use super::*;

    /// Encodes the sparse template bindings without allocating another component table.
    pub(super) fn serialize<S: Serializer>(
        offsets: &[(u32, RandomOffsetComponent)],
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(
            offsets
                .iter()
                .map(|&(template, component)| (template, RandomOffset::from(component))),
        )
    }

    /// Restores the sparse bindings in their original order without filtering values.
    pub(super) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Vec<(u32, RandomOffsetComponent)>, D::Error> {
        Vec::<(u32, RandomOffset)>::deserialize(deserializer).map(|offsets| {
            offsets
                .into_iter()
                .map(|(template, component)| (template, component.into()))
                .collect()
        })
    }
}
