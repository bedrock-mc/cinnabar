//! Optional authored terrain materials; the compiled carrier owns identity and timing.

use std::{collections::BTreeMap, sync::Arc};

use assets::{
    NO_ANIMATION, RuntimeAssets, TextureRef,
    material_keys::MaterialKeys,
    texture::{TextureArray, TextureMip},
};
use image::{ImageBuffer, Rgba};
use pack_compiler::pbr::{
    PbrMipLayer, PbrPack, PbrSurface, build_pbr_mips, load_pbr_texture_for_key,
};

mod audit;
mod cache;
mod config;
#[cfg(test)]
mod tint_tests;

/// Semicolon- or newline-separated pack directories that replace the local PBR configuration.
pub(crate) const ENHANCED_PBR_DIR_ENVIRONMENT: &str = "CINNABAR_ENHANCED_PBR_DIR";
const REF_FALLBACK: u32 = u32::MAX;
/// Authored layers sit below the `PBR_REF_*` flag bits of their packed reference.
const MAX_AUTHORED_LAYERS: u32 = assets::texture::PBR_REF_COLOR;
const AUTHORED_LAYER_MASK: u32 = MAX_AUTHORED_LAYERS - 1;
const _: () = assert!(MAX_AUTHORED_LAYERS as usize <= assets::MAX_TEXTURE_LAYERS);
const LOW_PBR_TILE_SIZE: u32 = assets::texture::TILE_SIZE * 2;

#[derive(Clone, Copy)]
struct Source {
    material: u32,
    texture: u32,
    frame: usize,
    count: usize,
    cutout: bool,
    overlay_mask: bool,
    baked_tint: Option<[u8; 3]>,
}

/// Keeps every alias and material owner until its authored image has been resolved.
fn sources(
    runtime: &RuntimeAssets,
    keys: &MaterialKeys,
) -> BTreeMap<(String, String), Vec<Source>> {
    let mut references = BTreeMap::<(String, String, u32, u32), Source>::new();
    for (key, alias) in keys.aliases() {
        for &id in keys.materials(key) {
            // This resolver admits only the catalog's variant-zero source.
            if keys.material_source(id).is_some_and(|source| {
                source.key.as_ref() != key || source.variant != 0 || source.path.as_ref() != alias
            }) || keys.flipbook_selector(id) != (0, 0)
            {
                continue;
            }
            let Some(material) = runtime.materials().get(id as usize) else {
                continue;
            };
            let overlay_mask = material.flags & assets::MATERIAL_FLAG_OVERLAY_MASK != 0;
            let baked_tint = keys
                .material_source(id)
                .and_then(|source| source.baked_tint);
            let cutout = material.flags & assets::MATERIAL_FLAG_ALPHA_CUTOUT != 0;
            let animation = (material.animation != NO_ANIMATION)
                .then(|| runtime.animations().get(material.animation as usize))
                .flatten();
            let count = animation.map_or(1, |animation| animation.frame_count as usize);
            references
                .entry((key.to_owned(), alias.to_owned(), id, material.texture.raw()))
                .and_modify(|source| {
                    source.cutout |= cutout;
                    source.overlay_mask |= overlay_mask;
                })
                .or_insert_with(|| Source {
                    material: id,
                    texture: material.texture.raw(),
                    frame: 0,
                    count,
                    cutout,
                    overlay_mask,
                    baked_tint,
                });
            if let Some(animation) = animation {
                let start = animation.frame_start as usize;
                let end = start.saturating_add(count);
                for (index, frame) in runtime
                    .animation_frames()
                    .get(start..end)
                    .into_iter()
                    .flatten()
                    .enumerate()
                {
                    references
                        .entry((key.to_owned(), alias.to_owned(), id, frame.raw()))
                        .and_modify(|source| {
                            source.cutout |= cutout;
                            source.overlay_mask |= overlay_mask;
                            if source.count == 1 {
                                source.frame = index;
                                source.count = count;
                            }
                        })
                        .or_insert_with(|| Source {
                            material: id,
                            texture: frame.raw(),
                            frame: index,
                            count,
                            cutout,
                            overlay_mask,
                            baked_tint,
                        });
                }
            }
        }
    }
    let mut groups = BTreeMap::<(String, String), Vec<Source>>::new();
    for ((key, alias, _, _), source) in references {
        groups.entry((key, alias)).or_default().push(source);
    }
    groups
}

struct Page {
    size: u32,
    layers: u32,
    data: Vec<Vec<u8>>,
}

impl Page {
    /// Creates empty layer buffers for every mip of a power-of-two page.
    fn new(size: u32) -> Self {
        Self {
            size,
            layers: 0,
            data: (0..=size.ilog2()).map(|_| Vec::new()).collect(),
        }
    }
    /// Adds one complete layer to the page in mip order.
    fn append(&mut self, mips: &[TextureMip]) {
        for (data, mip) in self.data.iter_mut().zip(mips) {
            data.extend_from_slice(&mip.rgba8);
        }
        self.layers += 1;
    }
    /// Freezes accumulated layer bytes into a texture array.
    fn finish(self) -> TextureArray {
        TextureArray {
            layers: self.layers,
            mips: self
                .data
                .into_iter()
                .enumerate()
                .map(|(level, data)| TextureMip {
                    size: self.size >> level,
                    rgba8: data.into_boxed_slice(),
                })
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        }
    }
}

/// Builds a neutral layer for an otherwise empty authored page.
fn fallback(size: u32) -> PbrMipLayer {
    let image = |pixel| ImageBuffer::from_pixel(1, 1, Rgba(pixel));
    build_pbr_mips(
        &PbrSurface {
            color: image([128, 128, 128, 255]),
            normal: image([128, 128, 255, 128]),
            material: image([0, 0, 255, 0]),
            flags: 0,
        },
        size,
        false,
        false,
    )
    .expect("constant authored fallback is valid")
}

/// Resolves material aliases into sparse authored pages and coverage records.
fn load(
    groups: BTreeMap<(String, String), Vec<Source>>,
    packs: &[PbrPack],
) -> (Option<cache::Payload>, Vec<audit::Alias>) {
    let pages = || {
        [
            Page::new(assets::texture::PBR_TILE_SIZE),
            Page::new(LOW_PBR_TILE_SIZE),
        ]
    };
    let mut colors = pages();
    let mut normals = pages();
    let mut materials = pages();
    let mut references = BTreeMap::new();
    let mut audit_aliases = Vec::with_capacity(groups.len());
    for ((key, alias), sources) in groups {
        let mut audit_alias = audit::Alias::new(format!("{key} ({alias})"), sources.len());
        let texture = match load_pbr_texture_for_key(packs, &key, &alias) {
            Ok(Some(texture)) => texture,
            Ok(None) => {
                audit_alias.missing = true;
                audit_alias.fallback = true;
                audit_aliases.push(audit_alias);
                continue;
            }
            Err(error) => {
                bevy::log::warn!("Enhanced PBR {alias}: {error}; retained carrier fallback");
                audit_alias.error = true;
                audit_alias.fallback = true;
                audit_aliases.push(audit_alias);
                continue;
            }
        };
        audit_alias.apply_source(&texture);
        let target_page = usize::from(texture.native_frame_size() <= LOW_PBR_TILE_SIZE);
        let tile_size = colors[target_page].size;
        audit_alias.upload_tile_size = Some(tile_size);
        let mut completed = BTreeMap::new();
        for source in sources {
            let key = (
                source.frame,
                source.count,
                source.cutout,
                source.overlay_mask,
                texture.inherited_tint(source.baked_tint),
            );
            let encoded = if let Some(&reference) = completed.get(&key) {
                reference
            } else {
                if colors[target_page].layers >= MAX_AUTHORED_LAYERS {
                    audit_alias.fallback = true;
                    break;
                }
                let mip = texture
                    .frame_with_inherited_tint(source.frame, source.count, source.baked_tint)
                    .and_then(|frame| {
                        build_pbr_mips(&frame, tile_size, source.cutout, source.overlay_mask)
                    });
                let mip = match mip {
                    Ok(mip) => mip,
                    Err(error) => {
                        bevy::log::warn!("Enhanced PBR {alias} frame {}: {error}", source.frame);
                        audit_alias.error = true;
                        continue;
                    }
                };
                let layer = colors[target_page].layers;
                // Authored references keep their page in the top bit, above the PBR flags.
                let reference = ((target_page as u32) << 31) | layer | mip.flags;
                colors[target_page].append(&mip.color);
                normals[target_page].append(&mip.normal);
                materials[target_page].append(&mip.material);
                completed.insert(key, reference);
                reference
            };
            if reference_slot(source.texture).is_some() {
                references.insert((source.material, source.texture), encoded);
                audit_alias.apply_reference(encoded);
            } else {
                audit_alias.fallback = true;
            }
        }
        audit_alias.finish();
        audit_aliases.push(audit_alias);
    }
    if colors.iter().all(|page| page.layers == 0) {
        return (None, audit_aliases);
    }
    for index in 0..2 {
        if colors[index].layers == 0 {
            let fallback = fallback(colors[index].size);
            colors[index].append(&fallback.color);
            normals[index].append(&fallback.normal);
            materials[index].append(&fallback.material);
        }
    }
    (
        Some(cache::Payload {
            color: colors.map(Page::finish),
            normal: normals.map(Page::finish),
            material: materials.map(Page::finish),
            references: render_scene::EnhancedTextureAssets::material_references(references),
        }),
        audit_aliases,
    )
}

/// The reference-table slot of a carrier texture, indexed like the shader's authored lookup.
fn reference_slot(texture: u32) -> Option<usize> {
    let texture = TextureRef::from_raw(texture).ok()?;
    Some(texture.page() as usize * assets::MAX_TEXTURE_LAYERS + texture.layer() as usize)
}

/// Loads selected packs from a valid cache or decodes their authored maps.
pub(crate) fn load_optional_enhanced_textures(
    runtime: &RuntimeAssets,
    keys: &MaterialKeys,
) -> Option<Arc<render_scene::EnhancedTextureAssets>> {
    let packs: Vec<_> = config::selected_packs()?
        .into_iter()
        .map(|pack| pack.with_terrain_aliases(keys.aliases()))
        .collect();
    let groups = sources(runtime, keys);
    let fingerprint = cache::fingerprint(&packs, &groups);
    if let Some(key) = fingerprint.as_ref()
        && let Some(mut report) = audit::load(key)
        && let Some(cached) = cache::load(key)
    {
        report.source = "cached_with_provenance".to_owned();
        report.print();
        if let Some(cached) = cached.into_assets() {
            return Some(Arc::new(cached));
        }
    }
    let (payload, aliases) = load(groups, &packs);
    let report = audit::Report::decoded(
        &fingerprint.unwrap_or([0; 32]),
        aliases,
        payload.as_ref().map_or(0, |payload| {
            render_scene::EnhancedTextureAssets::reference_values(&payload.references)
                .filter(|&reference| reference != REF_FALLBACK)
                .count()
        }),
        payload.as_ref().map_or(0, |payload| {
            render_scene::EnhancedTextureAssets::reference_values(&payload.references)
                .filter(|&reference| reference != REF_FALLBACK)
                .map(|reference| (reference >> 31, reference & AUTHORED_LAYER_MASK))
                .collect::<std::collections::BTreeSet<_>>()
                .len() as u32
        }),
        payload.as_ref().map(|payload| {
            std::array::from_fn(|index| payload.color[index].mips.first().map_or(0, |mip| mip.size))
        }),
    );
    report.print();
    audit::save(&report);
    let payload = payload?;
    if let Some(key) = fingerprint.as_ref() {
        cache::save(key, &payload);
    }
    payload.into_assets().map(Arc::new)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Creates two distinct materials that share one carrier image.
    pub(super) fn shared_carrier() -> (RuntimeAssets, MaterialKeys) {
        let base = RuntimeAssets::diagnostic();
        let material = assets::Material {
            texture: TextureRef::new(1, 0).unwrap(),
            ..base.material(0)
        };
        let runtime = base
            .with_block_overlay(
                base.visual_count() as u32,
                &assets::BlockOverlay {
                    materials: vec![material; 2],
                    textures: vec![base.texture_array().clone()],
                    ..Default::default()
                },
            )
            .unwrap();
        let keys = MaterialKeys::from_entries([(1, "cauldron_inner"), (2, "hopper_inside")])
            .with_aliases([
                ("cauldron_inner", "textures/blocks/cauldron_inner"),
                ("hopper_inside", "textures/blocks/hopper_inside"),
            ]);
        (runtime, keys)
    }

    /// Reads an authored layer's first albedo texel through the production reference lookup.
    pub(super) fn albedo(
        maps: &render_scene::EnhancedTextureAssets,
        runtime: &RuntimeAssets,
        id: u32,
    ) -> Option<[u8; 4]> {
        let reference = maps.material_reference(id, runtime.material(id).texture);
        if reference == REF_FALLBACK {
            return None;
        }
        let page = &maps.color_pages[(reference >> 31) as usize];
        let mip = &page.mips[0];
        let start =
            (reference & AUTHORED_LAYER_MASK) as usize * mip.size as usize * mip.size as usize * 4;
        Some(mip.rgba8[start..start + 4].try_into().unwrap())
    }

    #[test]
    fn a_pack_supplying_only_the_second_shared_alias_is_used() {
        let (runtime, keys) = shared_carrier();
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("textures/blocks")).unwrap();
        ImageBuffer::from_pixel(2, 2, Rgba([20u8, 80, 140, 255]))
            .save(root.path().join("textures/blocks/hopper_inside.png"))
            .unwrap();
        let pack = PbrPack::new(root.path(), None).with_terrain_aliases(keys.aliases());
        let (payload, _) = load(sources(&runtime, &keys), &[pack]);
        let maps = payload.unwrap().into_assets().unwrap();
        assert_eq!(albedo(&maps, &runtime, 2), Some([20, 80, 140, 255]));
        assert_eq!(albedo(&maps, &runtime, 1), None);
    }

    #[test]
    fn distinct_authored_aliases_split_a_shared_carrier_reference() {
        let (runtime, keys) = shared_carrier();
        assert_eq!(runtime.material(1).texture, runtime.material(2).texture);
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("textures/blocks")).unwrap();
        for (alias, pixel) in [
            ("cauldron_inner", [20u8, 80, 140, 255]),
            ("hopper_inside", [140, 80, 20, 255]),
        ] {
            ImageBuffer::from_pixel(2, 2, Rgba(pixel))
                .save(root.path().join(format!("textures/blocks/{alias}.png")))
                .unwrap();
        }
        let pack = PbrPack::new(root.path(), None).with_terrain_aliases(keys.aliases());
        let (payload, _) = load(sources(&runtime, &keys), &[pack]);
        let maps = payload.unwrap().into_assets().unwrap();
        assert_eq!(albedo(&maps, &runtime, 1), Some([20, 80, 140, 255]));
        assert_eq!(albedo(&maps, &runtime, 2), Some([140, 80, 20, 255]));
        assert_ne!(
            maps.material_reference(1, runtime.material(1).texture),
            maps.material_reference(2, runtime.material(2).texture)
        );
        assert_eq!(
            maps.material_reference(2, TextureRef::DIAGNOSTIC),
            REF_FALLBACK
        );
        assert_eq!(maps.authored_layer_count(), 2);
    }

    #[test]
    fn terrain_keys_with_one_image_alias_keep_distinct_pack_overrides() {
        let (runtime, keys) = shared_carrier();
        let keys = keys.with_aliases([
            ("cauldron_inner", "textures/blocks/shared"),
            ("hopper_inside", "textures/blocks/shared"),
        ]);
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("textures/blocks")).unwrap();
        for (alias, pixel) in [
            ("cauldron", [20u8, 80, 140, 255]),
            ("hopper", [140, 80, 20, 255]),
        ] {
            ImageBuffer::from_pixel(2, 2, Rgba(pixel))
                .save(root.path().join(format!("textures/blocks/{alias}.png")))
                .unwrap();
        }
        std::fs::write(
            root.path().join("textures/terrain_texture.json"),
            br#"{
            "texture_data":{
                "cauldron_inner":{"textures":"textures/blocks/cauldron"},
                "hopper_inside":{"textures":"textures/blocks/hopper"}
            }
        }"#,
        )
        .unwrap();
        let pack = PbrPack::new(root.path(), None).with_terrain_aliases(keys.aliases());
        let (payload, _) = load(sources(&runtime, &keys), &[pack]);
        let maps = payload.unwrap().into_assets().unwrap();
        assert_eq!(albedo(&maps, &runtime, 1), Some([20, 80, 140, 255]));
        assert_eq!(albedo(&maps, &runtime, 2), Some([140, 80, 20, 255]));
    }

    #[test]
    fn catalog_references_are_sparse_and_keep_server_texture_slots_on_fallback() {
        let runtime = RuntimeAssets::diagnostic();
        let keys = MaterialKeys::from_entries([(0, "stone")])
            .with_aliases([("stone", "textures/blocks/stone")]);
        let groups = sources(&runtime, &keys);
        assert_eq!(groups.len(), 1);
        let matched = &groups[&("stone".to_owned(), "textures/blocks/stone".to_owned())];
        assert_eq!(matched.len(), 1);
        assert_eq!(matched[0].texture, runtime.materials()[0].texture.raw());
        assert_eq!(matched[0].frame, 0);
        assert_eq!(matched[0].count, 1);
    }

    #[test]
    fn unsupported_material_variants_keep_their_carrier_fallback() {
        let (runtime, _) = shared_carrier();
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("textures/blocks")).unwrap();
        for (alias, pixel) in [
            ("primary", [20u8, 80, 140, 255]),
            ("secondary", [140, 80, 20, 255]),
        ] {
            ImageBuffer::from_pixel(2, 2, Rgba(pixel))
                .save(root.path().join(format!("textures/blocks/{alias}.png")))
                .unwrap();
        }
        std::fs::write(root.path().join("textures/terrain_texture.json"), br#"{
            "texture_data":{"fixture":{"textures":["textures/blocks/primary","textures/blocks/secondary"]}}
        }"#).unwrap();
        for (variant, path, selector_only) in [
            (1, "textures/blocks/secondary", false),
            (0, "textures/blocks/secondary", false),
            (1, "textures/blocks/primary", true),
        ] {
            let mut keys = MaterialKeys::from_entries([(1, "fixture"), (2, "fixture")])
                .with_aliases([("fixture", "textures/blocks/primary")]);
            if selector_only {
                keys = keys.with_flipbook_selectors([(2, (variant, 0))]);
            } else {
                keys = keys.with_sources(
                    [],
                    [(
                        2,
                        assets::material_keys::MaterialTextureSource {
                            key: "fixture".into(),
                            variant,
                            path: path.into(),
                            raster: format!("{path}.png").into(),
                            baked_tint: None,
                        },
                    )],
                    [],
                );
            }
            let pack = PbrPack::new(root.path(), None).with_terrain_aliases(keys.aliases());
            let (payload, _) = load(sources(&runtime, &keys), &[pack]);
            let maps = payload.unwrap().into_assets().unwrap();
            assert_eq!(albedo(&maps, &runtime, 1), Some([20, 80, 140, 255]));
            assert_eq!(
                albedo(&maps, &runtime, 2),
                None,
                "unsupported variant must retain its carrier"
            );
        }
    }

    /// Carrier layers above the old 11-bit range must not alias lower table slots.
    #[test]
    fn reference_slots_use_the_full_carrier_layer_range() {
        let high = TextureRef::new(1, assets::TEXTURE_LAYER_MASK)
            .unwrap()
            .raw();
        assert_eq!(
            reference_slot(high),
            Some(assets::MAX_TEXTURE_LAYERS + assets::TEXTURE_LAYER_MASK as usize)
        );
        let low = TextureRef::new(1, 2048).unwrap().raw();
        assert_eq!(reference_slot(low), Some(assets::MAX_TEXTURE_LAYERS + 2048));
    }
}
