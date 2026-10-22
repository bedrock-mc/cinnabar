//! Optional local dimension backdrops retain their original resolution.

use super::{MAX_IMAGE_SIDE, OreUiImages, OreUiPage, OreUiSprite, read_bounded};
use image::{ImageReader, Limits};
use std::{io::Cursor, path::Path, sync::Arc};

const MAX_IMAGE_BYTES: u64 = 8 * 1024 * 1024;

/// A dimension's loading backdrop and the block item its destination card shows.
pub struct DimensionArt {
    pub background: &'static str,
    pub icon: assets::facts::UiItemIcon,
    file: &'static str,
}

/// Each dimension's art, by protocol index.
pub const DIMENSIONS: [DimensionArt; 3] = [
    DimensionArt {
        background: "@dimension/background/overworld",
        icon: assets::facts::UiItemIcon::Overworld,
        file: "overworld.png",
    },
    DimensionArt {
        background: "@dimension/background/nether",
        icon: assets::facts::UiItemIcon::Nether,
        file: "nether.png",
    },
    DimensionArt {
        background: "@dimension/background/end",
        icon: assets::facts::UiItemIcon::End,
        file: "end.png",
    },
];

/// The art for `dimension`, by its protocol index; none for unknown dimensions.
pub fn destination(dimension: i32) -> Option<&'static DimensionArt> {
    usize::try_from(dimension)
        .ok()
        .and_then(|index| DIMENSIONS.get(index))
}

pub(super) fn install(images: &mut OreUiImages) {
    let Ok(layout) = launcher::install_layout::InstallLayout::discover() else {
        return;
    };
    append(
        images,
        &layout.user_data_root.join("assets/dimension-loading"),
    );
}

fn append(images: &mut OreUiImages, directory: &Path) {
    let mut bytes: usize = images.pages.iter().map(|page| page.pixels.len()).sum();
    for art in &DIMENSIONS {
        let path = directory.join(art.file);
        if !path.is_file() {
            continue;
        }
        let decoded = decode(&path).and_then(|(width, height, pixels)| {
            if bytes + pixels.len() > render_model::MAX_UI_FIXED_TEXTURE_BYTES / 2 {
                return Err("dimension artwork exceeds its texture budget".into());
            }
            Ok((width, height, pixels))
        });
        match decoded {
            Ok((width, height, pixels)) => {
                let page = images.pages.len() as u16;
                bytes += pixels.len();
                images.pages.push(OreUiPage {
                    dimensions: [width, height],
                    pixels: pixels.into(),
                });
                Arc::make_mut(&mut images.sprites).insert(
                    art.background.into(),
                    OreUiSprite {
                        page,
                        bounds: [0, 0, width as u16, height as u16],
                    },
                );
            }
            Err(reason) => tracing::warn!(
                "Dimension backdrop {} unavailable ({reason})",
                path.display()
            ),
        }
    }
}

/// The shader applies alpha, so uploaded pixels retain straight source RGB.
fn decode(path: &Path) -> Result<(u32, u32, Vec<u8>), String> {
    let bytes = read_bounded(path, MAX_IMAGE_BYTES)?;
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|error| error.to_string())?;
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_IMAGE_SIDE);
    limits.max_image_height = Some(MAX_IMAGE_SIDE);
    limits.max_alloc = Some(64 * 1024 * 1024);
    reader.limits(limits);
    let image = reader
        .decode()
        .map_err(|error| format!("{}: {error}", path.display()))?
        .into_rgba8();
    let (width, height) = image.dimensions();
    Ok((width, height, image.into_raw()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn destination_cards_resolve_registered_block_items() {
        let blocks = assets::read_registry_for_protocol(
            assets::pinned_block_registry_bytes(),
            assets::active_content_registry_protocol(),
        )
        .unwrap();
        let items = protocol::vanilla_item_registry();
        for dimension in 0..DIMENSIONS.len() as i32 {
            let art = destination(dimension).unwrap();
            assert!(
                blocks
                    .iter()
                    .any(|block| block.name.as_ref() == art.icon.identifier())
            );
            assert!(
                items
                    .iter()
                    .any(|item| item.identifier.as_ref() == art.icon.identifier())
            );
        }
    }

    #[test]
    fn backgrounds_retain_original_pixels_and_skip_only_the_missing_or_bad_destination() {
        let directory =
            std::env::temp_dir().join(format!("cinnabar-dimension-art-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let mut images = OreUiImages {
            pages: Vec::new(),
            sprites: Default::default(),
            loading_frames: Default::default(),
            animations: Default::default(),
        };
        let color = [72, 143, 199, 255];
        let source = image::RgbaImage::from_pixel(1672, 941, image::Rgba(color));
        source.save(directory.join(DIMENSIONS[0].file)).unwrap();
        std::fs::write(directory.join(DIMENSIONS[1].file), b"invalid image").unwrap();
        append(&mut images, &directory);
        assert_eq!(images.pages.len(), 1);
        assert_eq!(
            images.pages[0].dimensions,
            [source.width(), source.height()]
        );
        assert_eq!(images.pages[0].pixels.as_ref(), source.as_raw());
        assert!(images.sprites.contains_key(DIMENSIONS[0].background));
        assert!(!images.sprites.contains_key(DIMENSIONS[1].background));
        assert!(!images.sprites.contains_key(DIMENSIONS[2].background));
        assert!(destination(-1).is_none() && destination(37).is_none());
        std::fs::remove_dir_all(directory).unwrap();
    }
}
