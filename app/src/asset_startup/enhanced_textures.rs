//! Optional 512-pixel authored terrain materials; the compiled carrier owns identity and timing.

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use assets::{MaterialKeys, NO_ANIMATION, RuntimeAssets, TextureArray, TextureMip, TextureRef};
use image::{ImageBuffer, Rgba};
use pack_compiler::pbr::{PbrMipLayer, PbrPack, PbrSurface, build_pbr_mips, load_pbr_texture};

mod cache;
mod config;

const REF_FALLBACK: u32 = u32::MAX;

#[derive(Clone, Copy)]
struct Source {
    texture: u32,
    frame: usize,
    count: usize,
    cutout: bool,
}

fn sources(runtime: &RuntimeAssets, keys: &MaterialKeys) -> BTreeMap<String, Vec<Source>> {
    let mut references = BTreeMap::<u32, (String, Source)>::new();
    for (key, alias) in keys.aliases() {
        for &id in keys.materials(key) {
            let Some(material) = runtime.materials().get(id as usize) else {
                continue;
            };
            let cutout = material.flags & assets::MATERIAL_FLAG_ALPHA_CUTOUT != 0;
            let animation = (material.animation != NO_ANIMATION)
                .then(|| runtime.animations().get(material.animation as usize))
                .flatten();
            let count = animation.map_or(1, |animation| animation.frame_count as usize);
            references
                .entry(material.texture.raw())
                .and_modify(|(_, source)| source.cutout |= cutout)
                .or_insert_with(|| {
                    (
                        alias.to_owned(),
                        Source {
                            texture: material.texture.raw(),
                            frame: 0,
                            count,
                            cutout,
                        },
                    )
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
                        .entry(frame.raw())
                        .and_modify(|(_, source)| {
                            source.cutout |= cutout;
                            if source.count == 1 {
                                source.frame = index;
                                source.count = count;
                            }
                        })
                        .or_insert_with(|| {
                            (
                                alias.to_owned(),
                                Source {
                                    texture: frame.raw(),
                                    frame: index,
                                    count,
                                    cutout,
                                },
                            )
                        });
                }
            }
        }
    }
    let mut groups = BTreeMap::<String, Vec<Source>>::new();
    for (_, (alias, source)) in references {
        groups.entry(alias).or_default().push(source);
    }
    groups
}

struct Page {
    layers: u32,
    data: Vec<Vec<u8>>,
}

impl Page {
    fn new() -> Self {
        Self {
            layers: 0,
            data: (0..=assets::PBR_TILE_SIZE.ilog2())
                .map(|_| Vec::new())
                .collect(),
        }
    }
    fn append(&mut self, mips: &[TextureMip]) {
        for (data, mip) in self.data.iter_mut().zip(mips) {
            data.extend_from_slice(&mip.rgba8);
        }
        self.layers += 1;
    }
    fn finish(self) -> TextureArray {
        TextureArray {
            layers: self.layers,
            mips: self
                .data
                .into_iter()
                .enumerate()
                .map(|(level, data)| TextureMip {
                    size: assets::PBR_TILE_SIZE >> level,
                    rgba8: data.into_boxed_slice(),
                })
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        }
    }
}

fn fallback() -> PbrMipLayer {
    let image = |pixel| ImageBuffer::from_pixel(1, 1, Rgba(pixel));
    build_pbr_mips(
        &PbrSurface {
            color: image([128, 128, 128, 255]),
            normal: image([128, 128, 255, 128]),
            material: image([0, 0, 255, 0]),
            flags: 0,
        },
        assets::PBR_TILE_SIZE,
        false,
    )
    .expect("constant authored fallback is valid")
}

fn one_page(mips: Box<[TextureMip]>) -> TextureArray {
    TextureArray { layers: 1, mips }
}

fn load(groups: BTreeMap<String, Vec<Source>>, packs: &[PbrPack]) -> Option<cache::Payload> {
    let catalog_aliases = groups.len();
    let mut colors = Page::new();
    let mut normals = Page::new();
    let mut materials = Page::new();
    let mut references = vec![REF_FALLBACK; assets::MAX_TEXTURE_PAGES * assets::MAX_TEXTURE_LAYERS];
    let mut authored_normals = BTreeSet::new();
    let mut authored_materials = BTreeSet::new();
    let mut authored_heights = BTreeSet::new();
    let mut loaded_aliases = 0_usize;
    let mut missing_color_aliases = 0;
    let mut aliases_with_normals = 0;
    let mut aliases_with_materials = 0;
    let mut aliases_with_heights = 0;
    let mut errors = 0;
    for (alias, sources) in groups {
        let texture = match load_pbr_texture(packs, &alias) {
            Ok(Some(texture)) => {
                loaded_aliases += 1;
                if texture.flags() & assets::PBR_REF_NORMAL != 0 {
                    aliases_with_normals += 1;
                }
                if texture.flags() & assets::PBR_REF_MATERIAL != 0 {
                    aliases_with_materials += 1;
                }
                if texture.has_height_map() {
                    aliases_with_heights += 1;
                }
                texture
            }
            Ok(None) => {
                missing_color_aliases += 1;
                continue;
            }
            Err(error) => {
                errors += 1;
                eprintln!("Enhanced PBR {alias}: {error}; retained carrier fallback");
                continue;
            }
        };
        let mut completed = BTreeMap::new();
        for source in sources {
            let key = (source.frame, source.count, source.cutout);
            let encoded = if let Some(&reference) = completed.get(&key) {
                reference
            } else {
                if colors.layers >= assets::MAX_TEXTURE_LAYERS as u32 {
                    break;
                }
                let mip = texture
                    .frame(source.frame, source.count)
                    .and_then(|frame| build_pbr_mips(&frame, assets::PBR_TILE_SIZE, source.cutout));
                let mip = match mip {
                    Ok(mip) => mip,
                    Err(error) => {
                        errors += 1;
                        eprintln!("Enhanced PBR {alias} frame {}: {error}", source.frame);
                        continue;
                    }
                };
                let layer = colors.layers;
                let reference = TextureRef::new(0, layer).ok()?.raw() | mip.flags;
                colors.append(&mip.color);
                normals.append(&mip.normal);
                materials.append(&mip.material);
                if mip.flags & assets::PBR_REF_NORMAL != 0 {
                    authored_normals.insert(layer);
                }
                if mip.flags & assets::PBR_REF_MATERIAL != 0 {
                    authored_materials.insert(layer);
                }
                if mip.flags & assets::PBR_REF_HEIGHT != 0 {
                    authored_heights.insert(layer);
                }
                completed.insert(key, reference);
                reference
            };
            let page = (source.texture >> 31) as usize;
            let layer = (source.texture & 0x7ff) as usize;
            if page < assets::MAX_TEXTURE_PAGES {
                references[page * assets::MAX_TEXTURE_LAYERS + layer] = encoded;
            }
        }
    }
    if colors.layers == 0 {
        eprintln!(
            "Enhanced authored terrain: no color maps matched ({missing_color_aliases} of {catalog_aliases} catalog aliases; {errors} invalid sources)"
        );
        return None;
    }
    let mapped_texture_refs = references
        .iter()
        .filter(|&&value| value != REF_FALLBACK)
        .count();
    eprintln!(
        "Enhanced authored terrain: {loaded_aliases}/{catalog_aliases} color aliases, {mapped_texture_refs} mapped texture refs, {} albedo layers at {}x{}; {aliases_with_normals} normal, {aliases_with_materials} material, {aliases_with_heights} height aliases; {} normal, {} material, {} height layers; {} without normal, {} without material, {} without height; {missing_color_aliases} missing colors, {errors} skipped invalid sources",
        colors.layers,
        assets::PBR_TILE_SIZE,
        assets::PBR_TILE_SIZE,
        authored_normals.len(),
        authored_materials.len(),
        authored_heights.len(),
        loaded_aliases.saturating_sub(aliases_with_normals),
        loaded_aliases.saturating_sub(aliases_with_materials),
        loaded_aliases.saturating_sub(aliases_with_heights),
    );
    let fallback = fallback();
    Some(cache::Payload {
        color: [colors.finish(), one_page(fallback.color)],
        normal: [normals.finish(), one_page(fallback.normal)],
        material: [materials.finish(), one_page(fallback.material)],
        references: references.into_boxed_slice(),
    })
}

pub(crate) fn load_optional_enhanced_textures(
    runtime: &RuntimeAssets,
    keys: &MaterialKeys,
) -> Option<Arc<render::EnhancedTextureAssets>> {
    let packs = config::selected_packs()?;
    let groups = sources(runtime, keys);
    let fingerprint = cache::fingerprint(&packs, &groups);
    if let Some(key) = fingerprint.as_ref() {
        if let Some(cached) = cache::load(key).and_then(cache::Payload::into_assets) {
            return Some(Arc::new(cached));
        }
    }
    let payload = load(groups, &packs)?;
    if let Some(key) = fingerprint.as_ref() {
        cache::save(key, &payload);
    }
    payload.into_assets().map(Arc::new)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_references_are_sparse_and_keep_server_texture_slots_on_fallback() {
        let runtime = RuntimeAssets::diagnostic();
        let keys = MaterialKeys::from_entries([(0, "stone")])
            .with_aliases([("stone", "textures/blocks/stone")]);
        let groups = sources(&runtime, &keys);
        assert_eq!(groups.len(), 1);
        let matched = &groups["textures/blocks/stone"];
        assert_eq!(matched.len(), 1);
        assert_eq!(matched[0].texture, runtime.materials()[0].texture.raw());
        assert_eq!(matched[0].frame, 0);
        assert_eq!(matched[0].count, 1);
    }
}
