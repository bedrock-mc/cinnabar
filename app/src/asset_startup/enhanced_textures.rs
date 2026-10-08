//! Optional authored terrain materials; the compiled carrier owns identity and timing.

use std::{collections::BTreeMap, sync::Arc};

use assets::{MaterialKeys, NO_ANIMATION, RuntimeAssets, TextureArray, TextureMip, TextureRef};
use image::{ImageBuffer, Rgba};
use pack_compiler::pbr::{PbrMipLayer, PbrPack, PbrSurface, build_pbr_mips, load_pbr_texture};

mod audit;
mod cache;
mod config;

const REF_FALLBACK: u32 = u32::MAX;
const LOW_PBR_TILE_SIZE: u32 = assets::TILE_SIZE * 2;

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
    size: u32,
    layers: u32,
    data: Vec<Vec<u8>>,
}

impl Page {
    fn new(size: u32) -> Self {
        Self {
            size,
            layers: 0,
            data: (0..=size.ilog2()).map(|_| Vec::new()).collect(),
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
                    size: self.size >> level,
                    rgba8: data.into_boxed_slice(),
                })
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        }
    }
}

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
    )
    .expect("constant authored fallback is valid")
}

fn load(
    groups: BTreeMap<String, Vec<Source>>,
    packs: &[PbrPack],
) -> (Option<cache::Payload>, Vec<audit::Alias>) {
    let pages = || {
        [
            Page::new(assets::PBR_TILE_SIZE),
            Page::new(LOW_PBR_TILE_SIZE),
        ]
    };
    let mut colors = pages();
    let mut normals = pages();
    let mut materials = pages();
    let mut references = vec![REF_FALLBACK; assets::MAX_TEXTURE_PAGES * assets::MAX_TEXTURE_LAYERS];
    let mut audit_aliases = Vec::with_capacity(groups.len());
    for (alias, sources) in groups {
        let mut audit_alias = audit::Alias::new(alias.clone(), sources.len());
        let texture = match load_pbr_texture(packs, &alias) {
            Ok(Some(texture)) => texture,
            Ok(None) => {
                audit_alias.missing = true;
                audit_alias.fallback = true;
                audit_aliases.push(audit_alias);
                continue;
            }
            Err(error) => {
                eprintln!("Enhanced PBR {alias}: {error}; retained carrier fallback");
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
            let key = (source.frame, source.count, source.cutout);
            let encoded = if let Some(&reference) = completed.get(&key) {
                reference
            } else {
                if colors[target_page].layers >= assets::MAX_TEXTURE_LAYERS as u32 {
                    audit_alias.fallback = true;
                    break;
                }
                let mip = texture
                    .frame(source.frame, source.count)
                    .and_then(|frame| build_pbr_mips(&frame, tile_size, source.cutout));
                let mip = match mip {
                    Ok(mip) => mip,
                    Err(error) => {
                        eprintln!("Enhanced PBR {alias} frame {}: {error}", source.frame);
                        audit_alias.error = true;
                        continue;
                    }
                };
                let layer = colors[target_page].layers;
                let Ok(texture_ref) = TextureRef::new(target_page as u32, layer) else {
                    audit_alias.error = true;
                    continue;
                };
                let reference = texture_ref.raw() | mip.flags;
                colors[target_page].append(&mip.color);
                normals[target_page].append(&mip.normal);
                materials[target_page].append(&mip.material);
                completed.insert(key, reference);
                reference
            };
            let page = (source.texture >> 31) as usize;
            let layer = (source.texture & 0x7ff) as usize;
            if page < assets::MAX_TEXTURE_PAGES {
                references[page * assets::MAX_TEXTURE_LAYERS + layer] = encoded;
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
            references: references.into_boxed_slice(),
        }),
        audit_aliases,
    )
}

pub(crate) fn load_optional_enhanced_textures(
    runtime: &RuntimeAssets,
    keys: &MaterialKeys,
) -> Option<Arc<render::EnhancedTextureAssets>> {
    let packs: Vec<_> = config::selected_packs()?
        .into_iter()
        .map(|pack| pack.with_terrain_aliases(keys.aliases()))
        .collect();
    let groups = sources(runtime, keys);
    let fingerprint = cache::fingerprint(&packs, &groups);
    if let Some(key) = fingerprint.as_ref() {
        if let Some(mut report) = audit::load(key)
            && let Some(cached) = cache::load(key)
        {
            report.source = "cached_with_provenance".to_owned();
            report.print();
            if let Some(cached) = cached.into_assets() {
                return Some(Arc::new(cached));
            }
        }
    }
    let (payload, aliases) = load(groups, &packs);
    let report = audit::Report::decoded(
        &fingerprint.unwrap_or([0; 32]),
        aliases,
        payload.as_ref().map_or(0, |payload| {
            payload
                .references
                .iter()
                .filter(|&&reference| reference != REF_FALLBACK)
                .count()
        }),
        payload.as_ref().map_or(0, |payload| {
            payload
                .references
                .iter()
                .filter(|&&reference| reference != REF_FALLBACK)
                .map(|reference| (reference >> 31, reference & 0x7ff))
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
