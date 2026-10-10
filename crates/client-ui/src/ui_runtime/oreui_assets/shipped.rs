//! Decodes embedded originals and substitutes optional prepared panorama crops.

use std::{
    collections::HashMap,
    path::Path,
    sync::{Arc, OnceLock},
};

use super::{
    LOADING_ANIMATION, OreUiImages, PROFILE_BANNERS, WORLD_PREVIEW, alpha_mask, decode_animation,
    embedded, is_mask, packing, read_bounded,
};
use assets::oreui_panorama::{self, BANNER_COUNT, OreUiPanoramas};

/// Shares the original artwork without reading any filesystem path.
pub fn shipped_oreui_images() -> OreUiImages {
    static IMAGES: OnceLock<OreUiImages> = OnceLock::new();
    IMAGES
        .get_or_init(|| load(None).expect("valid embedded OreUI artwork"))
        .clone()
}

/// Loads an optional carrier beside the other prepared assets; invalid or absent crops use originals.
pub fn load_oreui_images(compiled_dir: &Path) -> OreUiImages {
    let path = compiled_dir.join(assets::carriers::OREUI_PANORAMAS.output);
    let mut images = if path.is_file() {
        match read_bounded(&path, oreui_panorama::MAX_CARRIER_BYTES as u64)
            .and_then(|bytes| oreui_panorama::decode(&bytes))
            .and_then(|panoramas| load(Some(&panoramas)))
        {
            Ok(images) => images,
            Err(reason) => {
                eprintln!(
                    "OreUI panoramas unavailable at {} ({reason}); using shipped banners",
                    path.display()
                );
                shipped_oreui_images()
            }
        }
    } else {
        shipped_oreui_images()
    };
    super::dimensions::install(&mut images);
    images
}

/// Decodes one set of original images, replacing banner pixels only when crops were admitted.
fn load(panoramas: Option<&OreUiPanoramas>) -> Result<OreUiImages, String> {
    let mut sources = embedded::ALL.to_vec();
    sources.sort_by_key(|source| std::cmp::Reverse((source.size[1], source.size[0])));
    let mut pages = packing::Pages::default();
    let mut sprites = HashMap::new();
    let mut animations = HashMap::new();
    for source in sources {
        if source.key == LOADING_ANIMATION {
            let mut animation = Vec::new();
            for (index, (width, height, pixels, millis)) in
                decode_animation(source.bytes)?.into_iter().enumerate()
            {
                let sprite = pages.insert(&pixels, width, height)?;
                if index == 0 {
                    sprites.insert(source.key.to_owned(), sprite);
                }
                let frame = format!("{}#{index}", source.key);
                sprites.insert(frame.clone(), sprite);
                animation.push((frame, millis));
            }
            animations.insert(source.key.to_owned(), animation);
            continue;
        }
        let panorama = panoramas.and_then(|panoramas| {
            PROFILE_BANNERS
                .iter()
                .position(|key| *key == source.key)
                .or_else(|| (source.key == WORLD_PREVIEW).then_some(BANNER_COUNT))
                .map(|index| &panoramas.images[index])
        });
        let (width, height, pixels) = if let Some(pixels) = panorama {
            (
                oreui_panorama::WIDTH,
                oreui_panorama::HEIGHT,
                pixels.to_vec(),
            )
        } else {
            let pixels = image::load_from_memory(source.bytes)
                .map_err(|error| error.to_string())?
                .into_rgba8();
            if [pixels.width(), pixels.height()] != source.size {
                return Err(format!("{} does not match the art manifest", source.key));
            }
            (pixels.width(), pixels.height(), pixels.into_raw())
        };
        sprites.insert(source.key.to_owned(), pages.insert(&pixels, width, height)?);
        if is_mask(source.key) {
            sprites.insert(
                format!("@mask/{}", source.key),
                pages.insert(&alpha_mask(&pixels), width, height)?,
            );
        }
    }
    Ok(OreUiImages {
        pages: pages.finish(),
        sprites: Arc::new(sprites),
        loading_frames: Arc::new(
            animations
                .get(LOADING_ANIMATION)
                .cloned()
                .unwrap_or_default(),
        ),
        animations: Arc::new(animations),
    })
}
