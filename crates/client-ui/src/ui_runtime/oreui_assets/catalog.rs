//! Source discovery is complete; only artwork requested by a screen is decoded.

use std::{
    collections::{HashMap, VecDeque},
    path::PathBuf,
    sync::{Arc, Mutex},
};

use super::{
    MAX_IMAGE_BYTES, OreUiPage, OreUiSprite, alpha_mask, decode, decode_animation, is_mask,
    packing, read_bounded,
};

const MAX_DECODED_BYTES: usize = 256 * 1024 * 1024;
const MAX_PREPARED_BYTES: usize = render_model::MAX_UI_TEXTURE_BYTES;

pub struct SourceCatalog {
    files: HashMap<String, PathBuf>,
    cache: Mutex<Cache>,
}

#[derive(Default)]
struct Cache {
    decoded: HashMap<String, Arc<Vec<Frame>>>,
    decoded_order: VecDeque<String>,
    decoded_bytes: usize,
    prepared: VecDeque<(Vec<String>, Arc<Prepared>)>,
    prepared_bytes: usize,
}

struct Frame {
    width: u32,
    height: u32,
    pixels: Arc<[u8]>,
    millis: u32,
}

pub(super) struct Prepared {
    pub pages: Vec<OreUiPage>,
    pub sprites: HashMap<String, OreUiSprite>,
    pub animations: HashMap<String, Vec<(String, u32)>>,
}

impl SourceCatalog {
    pub(super) fn new(files: HashMap<String, PathBuf>) -> Self {
        Self {
            files,
            cache: Mutex::new(Cache::default()),
        }
    }

    pub fn contains(&self, key: &str) -> bool {
        self.files.contains_key(key)
    }
    pub fn len(&self) -> usize {
        self.files.len()
    }
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    pub(super) fn prepare(
        &self,
        keys: &[String],
        byte_limit: usize,
    ) -> Result<Arc<Prepared>, String> {
        let mut cache = self
            .cache
            .lock()
            .map_err(|_| "OreUI artwork cache is unavailable")?;
        if let Some((_, images)) = cache.prepared.iter().find(|(request, _)| request == keys) {
            if images
                .pages
                .iter()
                .map(|page| page.pixels.len())
                .sum::<usize>()
                > byte_limit
            {
                return Err("OreUI artwork exceeds the available texture budget".into());
            }
            return Ok(images.clone());
        }
        let mut rasters = Vec::new();
        for key in keys {
            let frames = if let Some(frames) = cache.decoded.get(key) {
                frames.clone()
            } else {
                let path = self
                    .files
                    .get(key)
                    .ok_or_else(|| format!("OreUI artwork is unavailable: {key}"))?;
                let frames: Vec<_> = if path
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("gif"))
                {
                    decode_animation(&read_bounded(path, MAX_IMAGE_BYTES)?)?
                        .into_iter()
                        .map(|(width, height, pixels, millis)| Frame {
                            width,
                            height,
                            pixels: pixels.into(),
                            millis,
                        })
                        .collect()
                } else {
                    let (width, height, pixels) = decode(path)?;
                    vec![Frame {
                        width,
                        height,
                        pixels: pixels.into(),
                        millis: 0,
                    }]
                };
                let bytes = frames.iter().map(|frame| frame.pixels.len()).sum::<usize>();
                while cache.decoded_bytes + bytes > MAX_DECODED_BYTES {
                    let Some(index) = cache
                        .decoded_order
                        .iter()
                        .position(|old| !keys.contains(old))
                    else {
                        return Err("OreUI requested artwork exceeds the decode budget".into());
                    };
                    let old = cache
                        .decoded_order
                        .remove(index)
                        .expect("decode cache key exists");
                    if let Some(frames) = cache.decoded.remove(&old) {
                        cache.decoded_bytes -=
                            frames.iter().map(|frame| frame.pixels.len()).sum::<usize>();
                    }
                }
                let frames = Arc::new(frames);
                cache.decoded_bytes += bytes;
                cache.decoded.insert(key.clone(), frames.clone());
                frames
            };
            if let Some(index) = cache.decoded_order.iter().position(|old| old == key) {
                cache.decoded_order.remove(index);
            }
            cache.decoded_order.push_back(key.clone());
            rasters.push((key, frames));
        }
        rasters.sort_by(|a, b| {
            let size = |frames: &[Frame]| {
                frames
                    .first()
                    .map_or((0, 0), |frame| (frame.height, frame.width))
            };
            size(&b.1).cmp(&size(&a.1)).then(a.0.cmp(b.0))
        });
        let area: u64 = rasters
            .iter()
            .flat_map(|(_, frames)| frames.iter())
            .filter(|frame| {
                frame.width <= super::OREUI_PAGE_SIDE && frame.height <= super::OREUI_PAGE_SIDE
            })
            .map(|frame| u64::from(frame.width) * u64::from(frame.height))
            .sum();
        let longest = rasters
            .iter()
            .flat_map(|(_, frames)| frames.iter())
            .filter(|frame| {
                frame.width <= super::OREUI_PAGE_SIDE && frame.height <= super::OREUI_PAGE_SIDE
            })
            .map(|frame| frame.width.max(frame.height))
            .max()
            .unwrap_or(1);
        let side = [256, 512, 1024, 2048, super::OREUI_PAGE_SIDE]
            .into_iter()
            .find(|side| *side >= longest && u64::from(*side).pow(2) >= area)
            .unwrap_or(super::OREUI_PAGE_SIDE);
        let mut pack = packing::Pages::new(side, byte_limit);
        let (mut sprites, mut animations) = (HashMap::new(), HashMap::new());
        for (key, frames) in rasters {
            let animated = frames.first().is_some_and(|frame| frame.millis != 0);
            let mut animation = Vec::new();
            for (index, frame) in frames.iter().enumerate() {
                let sprite = pack.insert(&frame.pixels, frame.width, frame.height)?;
                if index == 0 {
                    sprites.insert(key.clone(), sprite);
                }
                if animated {
                    let frame_key = format!("{key}#{index}");
                    sprites.insert(frame_key.clone(), sprite);
                    animation.push((frame_key, frame.millis));
                } else if is_mask(key) {
                    let mask =
                        pack.insert(&alpha_mask(&frame.pixels), frame.width, frame.height)?;
                    sprites.insert(format!("@mask/{key}"), mask);
                }
            }
            if animated {
                animations.insert(key.clone(), animation);
            }
        }
        let images = Arc::new(Prepared {
            pages: pack.finish(),
            sprites,
            animations,
        });
        let bytes = images
            .pages
            .iter()
            .map(|page| page.pixels.len())
            .sum::<usize>();
        while cache.prepared_bytes + bytes > MAX_PREPARED_BYTES {
            let Some((_, oldest)) = cache.prepared.pop_front() else {
                break;
            };
            cache.prepared_bytes -= oldest
                .pages
                .iter()
                .map(|page| page.pixels.len())
                .sum::<usize>();
        }
        cache.prepared_bytes += bytes;
        cache.prepared.push_back((keys.to_vec(), images.clone()));
        Ok(images)
    }
}
