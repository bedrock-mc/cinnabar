//! Optional local dimension backdrops retain their original resolution.

use super::{OreUiImages, OreUiPage, OreUiSprite, decode};
use std::{path::Path, sync::Arc};

pub(crate) struct DimensionArt {
    pub(crate) background: &'static str,
    pub(crate) block: &'static str,
    file: &'static str,
}

pub(crate) const DIMENSIONS: [DimensionArt; 3] = [
    DimensionArt {
        background: "@dimension/background/overworld",
        block: "minecraft:grass",
        file: "overworld.png",
    },
    DimensionArt {
        background: "@dimension/background/nether",
        block: "minecraft:netherrack",
        file: "nether.png",
    },
    DimensionArt {
        background: "@dimension/background/end",
        block: "minecraft:end_stone",
        file: "end.png",
    },
];

pub(crate) fn destination(dimension: i32) -> Option<&'static DimensionArt> {
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
            if bytes + pixels.len() > render_model::MAX_UI_TEXTURE_BYTES / 2 {
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
            Err(reason) => eprintln!(
                "Dimension backdrop {} unavailable ({reason})",
                path.display()
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            source: None,
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
