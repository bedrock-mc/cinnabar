use super::animate::select_updates;
use super::labpbr::{NEUTRAL_NORMAL, NEUTRAL_SPECULAR, labpbr_normal, labpbr_specular};
use super::pack::{SpriteRect, pack_sprites};
use super::pixels::{AtlasImages, MipImages, build_images, missing_texels};
use super::plan::{AtlasInputs, AtlasPlan, AuthoredPbr, LayerLayout, plan_atlas};
use super::*;
use assets::texture::{
    PBR_REF_HEIGHT, PBR_REF_LABPBR, PBR_REF_MATERIAL, PBR_REF_NORMAL, PBR_REF_SUBSURFACE,
};
use assets::{ANIMATION_FLAG_BLEND, Animation, Material, TextureArray, TextureMip, TextureRef};
use render_scene::select_animation_frames;

/// Original synthetic texel, distinct per page, layer, mip and position, with varied alpha.
pub(super) fn texel(page: u32, layer: u32, mip: u32, x: u32, y: u32) -> [u8; 4] {
    [
        (page * 64 + layer * 13 + mip * 5) as u8,
        (x * 7 + mip) as u8,
        (y * 11 + layer) as u8,
        (100 + x * 3 + y * 5 + layer * 9) as u8,
    ]
}

/// A page of `layers` square layers with a full mip chain of [`texel`] values.
pub(super) fn page(index: u32, size: u32, layers: u32) -> TextureArray {
    TextureArray {
        layers,
        mips: (0..=size.ilog2())
            .map(|mip| {
                let side = size >> mip;
                TextureMip {
                    size: side,
                    rgba8: (0..layers)
                        .flat_map(|layer| {
                            (0..side).flat_map(move |y| {
                                (0..side).flat_map(move |x| texel(index, layer, mip, x, y))
                            })
                        })
                        .collect(),
                }
            })
            .collect(),
    }
}

pub(super) fn reference(page: u32, layer: u32) -> TextureRef {
    TextureRef::new(page, layer).unwrap()
}

/// Page 0 carries 16-texel layers; page 1 is a 32-texel overlay page whose layer 0 exposes a
/// grid quarter and whose layer 1 has an 8-texel source.
pub(super) fn layout(texture: TextureRef) -> LayerLayout {
    match (texture.page(), texture.layer()) {
        (1, 0) => LayerLayout {
            grid: 1,
            source_side: 16,
        },
        (1, 1) => LayerLayout {
            grid: 0,
            source_side: 8,
        },
        _ => LayerLayout {
            grid: 0,
            source_side: 16,
        },
    }
}

pub(super) const FRAME_TICKS: u32 = 2;

/// Carrier tables: page-0 layer 1 is an overlay mask, layer 2 is unreferenced, layers 3..=5
/// are frames of a plain animation (0) and a blending one (1).
pub(super) struct Fixture {
    pub pages: Vec<TextureArray>,
    pub materials: Vec<Material>,
    pub animations: Vec<Animation>,
    pub frames: Vec<TextureRef>,
}

pub(super) fn fixture() -> Fixture {
    let material = |texture, flags, animation| Material {
        texture,
        flags,
        animation,
        ..Material::unvaried()
    };
    let animation = |flags| Animation {
        frame_start: 0,
        frame_count: 3,
        ticks_per_frame: FRAME_TICKS,
        atlas_index: 0,
        atlas_tile_variant: 0,
        replicate: 0,
        flags,
    };
    Fixture {
        pages: vec![page(0, 16, 6), page(1, 32, 2)],
        materials: vec![
            material(reference(0, 0), 0, assets::NO_ANIMATION),
            material(
                reference(0, 1),
                assets::MATERIAL_FLAG_OVERLAY_MASK,
                assets::NO_ANIMATION,
            ),
            material(reference(1, 0), 0, assets::NO_ANIMATION),
            material(reference(1, 1), 0, assets::NO_ANIMATION),
            material(reference(0, 3), 0, 0),
            material(reference(0, 3), 0, 1),
        ],
        animations: vec![animation(0), animation(ANIMATION_FLAG_BLEND)],
        frames: vec![reference(0, 3), reference(0, 4), reference(0, 5)],
    }
}

impl Fixture {
    /// Runs `f` with plain inputs borrowing this fixture.
    pub fn with_inputs<R>(
        &self,
        authored: Option<AuthoredPbr<'_>>,
        f: impl FnOnce(&AtlasInputs<'_>) -> R,
    ) -> R {
        let pages = self.pages.iter().collect::<Vec<_>>();
        f(&AtlasInputs {
            pages: &pages,
            materials: &self.materials,
            animations: &self.animations,
            animation_frames: &self.frames,
            layout: &layout,
            authored,
        })
    }
}

/// The rectangle a remap slot names.
pub(super) fn slot_rect(plan: &AtlasPlan, slot: u32) -> SpriteRect {
    let [position, size] = [
        plan.remap[slot as usize * 2],
        plan.remap[slot as usize * 2 + 1],
    ];
    assert_eq!(size & 0xffff, size >> 16, "sprites are square");
    SpriteRect {
        x: position & 0xffff,
        y: position >> 16,
        side: size & 0xffff,
    }
}

/// Texels of `source` layer at physical mip `mip`, cropped to its upper-left `side` square.
pub(super) fn expected(page: u32, layer: u32, mip: u32, side: u32) -> Vec<u8> {
    (0..side)
        .flat_map(|y| (0..side).flat_map(move |x| texel(page, layer, mip, x, y)))
        .collect()
}

fn plan_and_images(fixture: &Fixture) -> (AtlasPlan, AtlasImages) {
    fixture.with_inputs(None, |inputs| {
        let plan = plan_atlas(inputs, 4096).unwrap();
        let images = build_images(inputs, &plan);
        (plan, images)
    })
}

/// Deterministic pseudo-random power-of-two sides.
fn sides(count: u32, seed: u32) -> Vec<(u32, u32)> {
    let mut state = seed;
    (0..count)
        .map(|key| {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (key, 1 << ((state >> 24) % 7))
        })
        .collect()
}

#[test]
fn packed_sprites_are_disjoint_aligned_and_inside_a_bounded_atlas() {
    const MAX: u32 = 1024;
    for seed in 0..32 {
        let sprites = sides(40 + seed, seed);
        let packed = pack_sprites(&sprites, MAX).unwrap();
        assert!(packed.width <= MAX && packed.height <= MAX);
        assert!(packed.width == packed.height || packed.width == packed.height * 2);
        let area = sprites.iter().map(|&(_, side)| side * side).sum::<u32>();
        assert!(
            packed.width * packed.height < area * 2,
            "no larger than needed"
        );
        for (rect, &(_, side)) in packed.rects.iter().zip(&sprites) {
            assert_eq!(rect.side, side);
            assert_eq!(
                (rect.x % side, rect.y % side),
                (0, 0),
                "aligned to its side"
            );
            assert!(rect.x + side <= packed.width && rect.y + side <= packed.height);
        }
        for (index, a) in packed.rects.iter().enumerate() {
            for b in &packed.rects[index + 1..] {
                let apart = a.x + a.side <= b.x
                    || b.x + b.side <= a.x
                    || a.y + a.side <= b.y
                    || b.y + b.side <= a.y;
                assert!(apart, "{a:?} overlaps {b:?}");
            }
        }
        assert_eq!(pack_sprites(&sprites, MAX), Some(packed));
    }
    let five_squares = [(0, 32), (1, 32), (2, 32), (3, 32), (4, 32)];
    assert!(pack_sprites(&five_squares, 64).is_none());
    assert_eq!(pack_sprites(&five_squares[..4], 64).unwrap().width, 64);
}

#[test]
fn remap_slots_hold_the_sampled_region_at_source_resolution() {
    let fixture = fixture();
    let (plan, images) = plan_and_images(&fixture);
    assert_eq!(&plan.remap[..2], &[plan.atlas.width, plan.atlas.height]);
    assert_eq!(plan.scale, 0);
    // The smallest sprite is the 8-texel overlay source.
    assert_eq!(plan.mip_levels, 4);
    for (texture, physical_mip, side) in [
        (reference(0, 0), 0, 16),
        // Grid layers expose the upper-left quarter at full resolution.
        (reference(1, 0), 0, 16),
        // Source mip zero retains detail even when the physical page is larger.
        (reference(1, 1), 0, 8),
    ] {
        let rect = slot_rect(&plan, static_sprite_slot(texture.page(), texture.layer()));
        assert_eq!(rect.side, side, "{texture:?}");
        for level in 0..2 {
            assert_eq!(
                images.atlas.color.read(level, rect),
                {
                    let region = fixture.pages[texture.page() as usize].mips
                        [(physical_mip + level) as usize]
                        .size
                        >> layout(texture).grid;
                    pixels::resample(
                        &expected(
                            texture.page(),
                            texture.layer(),
                            physical_mip + level,
                            region,
                        ),
                        region,
                        side >> level,
                    )
                },
                "{texture:?} at mip {level}"
            );
        }
    }
    let missing = slot_rect(&plan, static_sprite_slot(0, 2));
    for absent in [
        static_sprite_slot(0, 6),
        static_sprite_slot(4, 0),
        static_sprite_slot(1, assets::TEXTURE_LAYER_MASK),
    ] {
        assert_eq!(slot_rect(&plan, absent), missing);
    }
    assert_eq!(
        images.atlas.color.read(0, missing),
        missing_texels(missing.side)
    );
    assert_eq!(
        images.atlas.color.read(1, missing),
        missing_texels(missing.side / 2)
    );
    assert!(
        images.atlas.pbr.is_none(),
        "unauthored PBR atlases upload neutral texels"
    );
}

#[test]
fn overlay_mask_textures_have_an_opaque_slot_with_unchanged_rgb() {
    let fixture = fixture();
    let (plan, images) = plan_and_images(&fixture);
    let static_rect = slot_rect(&plan, static_sprite_slot(0, 1));
    let opaque_rect = slot_rect(&plan, opaque_sprite_slot(0, 1));
    assert_ne!(static_rect, opaque_rect);
    for level in 0..plan.mip_levels {
        let original = images.atlas.color.read(level, static_rect);
        let opaque = images.atlas.color.read(level, opaque_rect);
        assert!(
            original
                .as_chunks::<4>()
                .0
                .iter()
                .any(|texel| texel[3] < 255)
        );
        for (original, opaque) in original
            .as_chunks::<4>()
            .0
            .iter()
            .zip(opaque.as_chunks::<4>().0)
        {
            assert_eq!(&opaque[..3], &original[..3]);
            assert_eq!(opaque[3], 255);
        }
    }
    for (page, layer) in [(0, 0), (1, 0), (0, 2)] {
        assert_eq!(
            slot_rect(&plan, opaque_sprite_slot(page, layer)),
            slot_rect(&plan, static_sprite_slot(page, layer))
        );
    }
}

#[test]
fn overflow_halves_every_sprite_deterministically() {
    let fixture = fixture();
    let (plan, images) = fixture.with_inputs(None, |inputs| {
        let plan = plan_atlas(inputs, 32).unwrap();
        assert_eq!(plan_atlas(inputs, 32).as_ref(), Some(&plan));
        let images = build_images(inputs, &plan);
        (plan, images)
    });
    assert_eq!(plan.scale, 1);
    assert!(plan.atlas.width <= 32 && plan.atlas.height <= 32);
    assert_eq!(plan.mip_levels, 3);
    let carrier = slot_rect(&plan, static_sprite_slot(0, 0));
    assert_eq!(carrier.side, 8);
    assert_eq!(images.atlas.color.read(0, carrier), expected(0, 0, 1, 8));
    assert_eq!(slot_rect(&plan, static_sprite_slot(1, 1)).side, 4);
    assert_eq!(slot_rect(&plan, animation_sprite_slot(0)).side, 8);
    assert!(
        fixture
            .with_inputs(None, |inputs| plan_atlas(inputs, 0))
            .is_none()
    );
}

#[test]
fn display_slots_follow_the_world_clock_frame_and_blend() {
    let fixture = fixture();
    let (plan, images) = plan_and_images(&fixture);
    let frames = images.frames.as_ref().unwrap();
    let clock = ChunkAnimationClock::from_parts(3, 0.5);
    let updates = select_updates(
        &fixture.animations,
        &fixture.frames,
        &plan.animations,
        &[None, None],
        clock,
    );
    assert_eq!(updates.len(), 2);
    for (index, update) in &updates {
        let material = Material {
            animation: *index as u32,
            ..Material::unvaried()
        };
        let oracle = select_animation_frames(material, &fixture.animations, &fixture.frames, clock);
        let layout = plan.animations[*index].as_ref().unwrap();
        assert_eq!(
            update.display,
            slot_rect(&plan, animation_sprite_slot(*index as u32))
        );
        assert_eq!(frames.color.read(0, update.current), expected(0, 4, 0, 16));
        assert_eq!(Some(update.current), layout.frame(oracle.current));
        if *index == 0 {
            assert_eq!((update.blend, update.next), (0.0, update.current));
        } else {
            assert_eq!(update.blend, oracle.blend);
            assert_eq!(update.blend, 0.75);
            assert_eq!(frames.color.read(1, update.next), expected(0, 5, 1, 8));
        }
    }
    let shown = updates
        .iter()
        .map(|&(_, update)| Some(update))
        .collect::<Vec<_>>();
    let unchanged = select_updates(
        &fixture.animations,
        &fixture.frames,
        &plan.animations,
        &shown,
        clock,
    );
    assert!(
        unchanged.is_empty(),
        "unchanged frames are not copied again"
    );
    let next_tick = ChunkAnimationClock::from_parts(3, 0.75);
    let refreshed = select_updates(
        &fixture.animations,
        &fixture.frames,
        &plan.animations,
        &shown,
        next_tick,
    );
    assert_eq!(
        refreshed
            .iter()
            .map(|&(index, _)| index)
            .collect::<Vec<_>>(),
        [1]
    );
    // Display sprites start on their first frame.
    assert_eq!(
        images
            .atlas
            .color
            .read(0, slot_rect(&plan, animation_sprite_slot(0))),
        expected(0, 3, 0, 16)
    );
}

#[test]
fn authored_maps_fill_carrier_slots_and_leave_others_neutral() {
    let fixture = fixture();
    let normals = page(7, 32, 1);
    let materials = page(8, 32, 1);
    let mut refs = vec![u32::MAX; STATIC_SPRITE_SLOTS as usize];
    let flags = PBR_REF_NORMAL | PBR_REF_HEIGHT | PBR_REF_MATERIAL | PBR_REF_LABPBR;
    refs[(static_sprite_slot(0, 0) - ATLAS_HEADER_SLOTS) as usize] = flags;
    let authored = AuthoredPbr {
        normal_pages: [&normals, &normals],
        mer_pages: [&materials, &materials],
        refs: &refs,
    };
    let (plan, images) = fixture.with_inputs(Some(authored), |inputs| {
        let plan = plan_atlas(inputs, 4096).unwrap();
        let images = build_images(inputs, &plan);
        (plan, images)
    });
    let [normal_atlas, specular_atlas] = images.atlas.pbr.as_ref().unwrap();
    let authored_rect = slot_rect(&plan, static_sprite_slot(0, 0));
    let convert = |images: &MipImages, map: fn([u8; 4], u32) -> [u8; 4], page: u32| {
        let expected = expected(page, 0, 1, 16)
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|&texel| map(texel, flags))
            .collect::<Vec<_>>();
        assert_eq!(images.read(0, authored_rect), expected);
    };
    convert(normal_atlas, labpbr_normal, 7);
    convert(specular_atlas, labpbr_specular, 8);
    let carrier_only = slot_rect(&plan, static_sprite_slot(1, 0));
    assert!(
        normal_atlas
            .read(0, carrier_only)
            .as_chunks::<4>()
            .0
            .iter()
            .all(|&t| t == NEUTRAL_NORMAL)
    );
    assert!(
        specular_atlas
            .read(0, carrier_only)
            .as_chunks::<4>()
            .0
            .iter()
            .all(|&t| t == NEUTRAL_SPECULAR)
    );
}

#[test]
fn labpbr_sources_round_trip_through_the_pack_compiler() {
    use pack_compiler::pbr::{PbrFormat, PbrPack, load_pbr_texture};
    let normals = [
        [128, 128, 255, 255],
        [150, 100, 200, 128],
        [90, 170, 128, 40],
        [128, 200, 64, 200],
    ];
    // A source emission of zero returns as 255; both mean no emission.
    let speculars = [
        [200, 10, 0, 255],
        [50, 230, 30, 100],
        [128, 255, 100, 254],
        [0, 0, 255, 1],
    ];
    let directory = tempfile::tempdir().unwrap();
    let block = directory.path().join("assets/minecraft/textures/block");
    std::fs::create_dir_all(&block).unwrap();
    let save = |name: &str, texels: [[u8; 4]; 4]| {
        image::RgbaImage::from_fn(2, 2, |x, y| image::Rgba(texels[(y * 2 + x) as usize]))
            .save(block.join(name))
            .unwrap();
    };
    save("stone.png", [[90, 90, 90, 255]; 4]);
    save("stone_n.png", normals);
    save("stone_s.png", speculars);
    let surface = load_pbr_texture(
        &[PbrPack::new(directory.path(), Some(PbrFormat::LabPbr13))],
        "textures/blocks/stone",
    )
    .unwrap()
    .unwrap()
    .frame(0, 1)
    .unwrap();
    assert_ne!(surface.flags & PBR_REF_LABPBR, 0);
    for (index, (normal, specular)) in normals.iter().zip(&speculars).enumerate() {
        let (x, y) = (index as u32 % 2, index as u32 / 2);
        assert_eq!(
            labpbr_normal(surface.normal.get_pixel(x, y).0, surface.flags),
            *normal
        );
        assert_eq!(
            labpbr_specular(surface.material.get_pixel(x, y).0, surface.flags),
            *specular
        );
    }
}

#[test]
fn mer_and_unauthored_layers_convert_to_labpbr_equivalents() {
    assert_eq!(labpbr_normal([1, 2, 3, 4], 0), NEUTRAL_NORMAL);
    assert_eq!(labpbr_normal([1, 2, 3, 4], PBR_REF_NORMAL), [1, 2, 3, 255]);
    assert_eq!(
        labpbr_specular([1, 2, 3, 4], PBR_REF_NORMAL),
        NEUTRAL_SPECULAR
    );
    // Metal MER: albedo metal, emission rescaled to LabPBR's 0..=254.
    assert_eq!(
        labpbr_specular([200, 50, 100, 0], PBR_REF_MATERIAL),
        [155, 255, 0, 50]
    );
    // Dielectric MER with the shaders' 0.04 reflectance and no emission.
    assert_eq!(
        labpbr_specular([10, 0, 255, 0], PBR_REF_MATERIAL),
        [0, 10, 0, 255]
    );
    let subsurface = PBR_REF_MATERIAL | PBR_REF_SUBSURFACE;
    assert_eq!(labpbr_specular([0, 0, 0, 255], subsurface)[2], 255);
    assert_eq!(labpbr_specular([0, 0, 0, 1], subsurface)[2], 66);
    assert_eq!(labpbr_specular([0, 0, 0, 255], PBR_REF_MATERIAL)[2], 0);
}

#[test]
fn atlas_rebuilds_for_added_replaced_and_removed_authored_maps() {
    let assets = ChunkTextureAssets::with_revision(Arc::new(RuntimeAssets::diagnostic()), 1);
    let authored = || {
        Arc::new(render_scene::EnhancedTextureAssets {
            color_pages: [page(0, 1, 1), page(0, 1, 1)],
            normal_pages: [page(0, 1, 1), page(0, 1, 1)],
            mer_pages: [page(0, 1, 1), page(0, 1, 1)],
            texture_refs: Box::new([]),
        })
    };
    let added = assets.with_updated_enhanced(Some(authored()));
    let replaced = added.with_updated_enhanced(Some(authored()));
    let removed = replaced.with_updated_enhanced(None);
    let mut build = AtlasBuild::default();
    for source in [&assets, &added, &replaced, &removed] {
        assert!(build.request(source));
        assert!(!build.request(&source.clone()));
    }
}

#[test]
fn terrain_source_lod_keeps_detail_on_a_larger_physical_page() {
    let source = |x: u32| {
        if x % 2 == 0 {
            [255, 0, 0, 255]
        } else {
            [0, 0, 255, 255]
        }
    };
    let texture = TextureArray {
        layers: 1,
        mips: (0..=6)
            .map(|level| {
                let size = 64 >> level;
                TextureMip {
                    size,
                    rgba8: (0..size * size)
                        .flat_map(|i| {
                            if level == 0 {
                                source(i % size / 4)
                            } else {
                                [127, 0, 127, 255]
                            }
                        })
                        .collect(),
                }
            })
            .collect(),
    };
    let inputs = AtlasInputs {
        pages: &[&texture],
        materials: &[],
        animations: &[],
        animation_frames: &[],
        layout: &|_| LayerLayout {
            grid: 0,
            source_side: 16,
        },
        authored: None,
    };
    let content = plan::SpriteContent::Layer {
        reference: reference(0, 0),
        opaque: false,
    };
    let base = pixels::color_texels(&inputs, content, 16);
    let expected = (0..16 * 16)
        .flat_map(|i| source(i % 16))
        .collect::<Vec<_>>();
    assert_eq!(base, expected);
    assert_eq!(
        pixels::color_texels(&inputs, content, 8),
        [127, 0, 127, 255].repeat(64)
    );
}

#[test]
fn animation_blend_targets_keep_encoded_colour() {
    let layout = bevy::render::render_resource::BindGroupLayoutDescriptor::new("test", &[]);
    let pipeline = blend::descriptor(&layout, "fragment_colour", &[upload::COLOR_STORAGE_FORMAT]);
    assert_eq!(
        pipeline.fragment.unwrap().targets[0]
            .as_ref()
            .unwrap()
            .format,
        wgpu::TextureFormat::Rgba8Unorm
    );
}

#[test]
fn malformed_pbr_pages_keep_neutral_texels() {
    let empty = TextureArray {
        layers: 1,
        mips: Box::new([]),
    };
    let authored = AuthoredPbr {
        normal_pages: [&empty, &empty],
        mer_pages: [&empty, &empty],
        refs: &[PBR_REF_NORMAL | PBR_REF_MATERIAL],
    };
    let content = plan::SpriteContent::Layer {
        reference: reference(0, 0),
        opaque: false,
    };
    assert_eq!(
        pixels::pbr_texels(&authored, content, 1),
        [NEUTRAL_NORMAL.to_vec(), NEUTRAL_SPECULAR.to_vec()]
    );
}

#[test]
fn animation_blend_shader_validates_without_a_gpu() {
    let module = naga::front::wgsl::parse_str(include_str!("blend.wgsl")).unwrap();
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    )
    .validate(&module)
    .unwrap();
}
