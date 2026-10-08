use super::*;
use assets::{
    Animation, BlobProvenance, BlockFlags, BlockVisual, CompiledAssets, CompiledBiomeAssets,
    ContributorRole, LightProperties, Material, NO_ANIMATION, NO_MODEL_TEMPLATE, TextureArray,
    TextureMip, TexturePage, VisualKind, VisualSupport, encode_blob,
};

fn pack() -> RuntimeAssets {
    let mut faces = [0; 6];
    faces[BlockFace::Down as usize] = 1;
    let down = TextureRef::new(1, 1).unwrap();
    let compiled = CompiledAssets {
        visuals: vec![BlockVisual {
            faces,
            flags: BlockFlags::CUBE_GEOMETRY,
            kind: VisualKind::Cube,
            support: VisualSupport::Exact,
            contributor_role: ContributorRole::Primary,
            model_template: NO_MODEL_TEMPLATE,
            animation: NO_ANIMATION,
            variant: 0,
        }]
        .into_boxed_slice(),
        light_properties: vec![LightProperties::default()].into_boxed_slice(),
        hashed: Box::new([]),
        materials: vec![
            Material {
                texture: TextureRef::DIAGNOSTIC,
                animation: NO_ANIMATION,
                ..Material::unvaried()
            },
            Material {
                texture: down,
                animation: 0,
                ..Material::unvaried()
            },
        ]
        .into_boxed_slice(),
        model_templates: Box::new([]),
        model_quads: Box::new([]),
        animations: vec![Animation {
            frame_start: 0,
            frame_count: 3,
            ticks_per_frame: 2,
            atlas_index: 0,
            atlas_tile_variant: 0,
            replicate: 1,
            flags: ANIMATION_FLAG_BLEND,
        }]
        .into_boxed_slice(),
        animation_frames: vec![down, TextureRef::DIAGNOSTIC, down].into_boxed_slice(),
        texture_pages: vec![page(&[17]), page(&[31, 47])].into_boxed_slice(),
        biomes: CompiledBiomeAssets::diagnostic(),
        provenance: BlobProvenance {
            source_manifest_sha256: [1; 32],
            block_registry_sha256: [2; 32],
            light_registry_sha256: [3; 32],
            biome_registry_sha256: [4; 32],
        },
    };
    RuntimeAssets::decode(&encode_blob(&compiled).unwrap()).unwrap()
}

fn page(colors: &[u8]) -> TexturePage {
    TexturePage::new(TextureArray {
        layers: colors.len() as u32,
        mips: (0..assets::MIP_COUNT)
            .map(|level| {
                let size = assets::TILE_SIZE >> level;
                TextureMip {
                    size,
                    rgba8: colors
                        .iter()
                        .flat_map(|&color| vec![color; (size * size * 4) as usize])
                        .collect::<Vec<_>>()
                        .into_boxed_slice(),
                }
            })
            .collect::<Vec<_>>()
            .into_boxed_slice(),
    })
}

#[test]
fn camera_fire_uses_down_face_and_keeps_cross_page_pack_timeline() {
    let fire = ScreenFireTexture::from_assets(&pack(), 0).unwrap();
    assert_eq!(fire.side, assets::TILE_SIZE);
    assert_eq!(fire.frames, 2);
    let bytes = (assets::TILE_SIZE * assets::TILE_SIZE * 4) as usize;
    assert_eq!(fire.pixels, [vec![47; bytes], vec![17; bytes]].concat());
    assert_eq!(fire.timeline, [0, 1, 0]);
    assert_eq!(
        fire.sample(ChunkAnimationClock::from_parts(5, 0.5)),
        [0.0, 0.0, 0.75, 1.0]
    );
}

#[test]
fn diagnostic_and_unknown_blocks_do_not_invent_fire_art() {
    assert!(ScreenFireTexture::from_assets(&RuntimeAssets::diagnostic(), 0).is_none());
    assert!(ScreenFireTexture::from_assets(&pack(), u32::MAX).is_none());
}

#[test]
fn non_blending_timeline_steps_at_its_admitted_tick_rate() {
    let mut fire = ScreenFireTexture::from_assets(&pack(), 0).unwrap();
    fire.blend = false;
    assert_eq!(
        fire.sample(ChunkAnimationClock::from_parts(5, 0.5)),
        [0.0, 0.0, 0.0, 1.0]
    );
    assert_eq!(
        fire.sample(ChunkAnimationClock::from_parts(6, 0.0)),
        [0.0, 1.0, 0.0, 1.0]
    );
}
