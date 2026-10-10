//! Prepares landscape crops from the sample pack's horizontal panorama faces.

use std::{
    fs,
    io::{Cursor, Read},
    path::Path,
};

use assets::oreui_panorama::{self, BANNER_COUNT, HEIGHT, IMAGE_COUNT, OreUiPanoramas, WIDTH};
use image::{ImageFormat, ImageReader, Limits};

const SOURCE_SIDE: u32 = 1024;
const MAX_SOURCE_BYTES: u64 = 8 * 1024 * 1024;

/// A horizontal face and the top edge of its landscape crop; the crop is centered horizontally.
struct Crop {
    texture: &'static str,
    top: u32,
}

const BANNERS: [Crop; BANNER_COUNT] = [
    Crop {
        texture: "textures/ui/panorama_2.png",
        top: 256,
    },
    Crop {
        texture: "textures/ui/panorama_alternate_0.png",
        top: 144,
    },
    Crop {
        texture: "textures/ui/panorama_0.png",
        top: 256,
    },
    Crop {
        texture: "textures/ui/panorama_1.png",
        top: 288,
    },
    Crop {
        texture: "textures/ui/panorama_3.png",
        top: 256,
    },
    Crop {
        texture: "textures/ui/panorama_alternate_1.png",
        top: 320,
    },
    Crop {
        texture: "textures/ui/panorama_alternate_2.png",
        top: 336,
    },
    Crop {
        texture: "textures/ui/panorama_alternate_3.png",
        top: 320,
    },
];

/// Decodes a bounded panorama face and extracts the selected 16:9 crop without resampling.
fn crop(pack: &Path, source: &Crop) -> Result<Box<[u8]>, String> {
    let path = pack.join(source.texture);
    let mut bytes = Vec::new();
    fs::File::open(&path)
        .map_err(|error| format!("{}: {error}", path.display()))?
        .take(MAX_SOURCE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() as u64 > MAX_SOURCE_BYTES {
        return Err(format!(
            "{} exceeds the panorama byte limit",
            path.display()
        ));
    }
    let mut reader = ImageReader::with_format(Cursor::new(bytes), ImageFormat::Png);
    let mut limits = Limits::default();
    limits.max_image_width = Some(SOURCE_SIDE);
    limits.max_image_height = Some(SOURCE_SIDE);
    limits.max_alloc = Some(u64::from(SOURCE_SIDE * SOURCE_SIDE * 8));
    reader.limits(limits);
    let pixels = reader
        .decode()
        .map_err(|error| format!("{}: {error}", path.display()))?
        .into_rgba8();
    if pixels.dimensions() != (SOURCE_SIDE, SOURCE_SIDE) {
        return Err(format!(
            "{} must be {SOURCE_SIDE}×{SOURCE_SIDE}",
            path.display()
        ));
    }
    Ok(image::imageops::crop_imm(
        &pixels,
        (SOURCE_SIDE - WIDTH) / 2,
        source.top,
        WIDTH,
        HEIGHT,
    )
    .to_image()
    .into_raw()
    .into())
}

/// Compiles eight profile banners and reuses the lake view for the world preview.
pub fn compile_oreui_panoramas(pack: &Path) -> Result<Vec<u8>, String> {
    let mut images = Vec::with_capacity(IMAGE_COUNT);
    for source in BANNERS {
        images.push(crop(pack, &source)?);
    }
    images.push(images[0].clone());
    let panoramas = OreUiPanoramas {
        images: images
            .try_into()
            .map_err(|_| "invalid panorama image count")?,
    };
    oreui_panorama::encode(&panoramas)
}

/// Writes a complete panorama carrier through a temporary sibling file.
pub fn compile_oreui_panoramas_to_file(pack: &Path, out: &Path) -> Result<(), String> {
    let blob = compile_oreui_panoramas(pack)?;
    let temporary = out.with_extension("tmp");
    fs::write(&temporary, blob).map_err(|error| error.to_string())?;
    fs::rename(temporary, out).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panorama_crops_keep_the_selected_source_pixels() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("textures/ui")).unwrap();
        for (index, source) in BANNERS.iter().enumerate() {
            image::RgbaImage::from_fn(SOURCE_SIDE, SOURCE_SIDE, |x, y| {
                image::Rgba([index as u8, (x % 251) as u8, (y % 251) as u8, 255])
            })
            .save(dir.path().join(source.texture))
            .unwrap();
        }
        let decoded =
            oreui_panorama::decode(&compile_oreui_panoramas(dir.path()).unwrap()).unwrap();
        for (index, source) in BANNERS.iter().enumerate() {
            assert_eq!(
                &decoded.images[index][..4],
                &[
                    index as u8,
                    ((SOURCE_SIDE - WIDTH) / 2) as u8,
                    (source.top % 251) as u8,
                    255
                ]
            );
        }
        assert_eq!(decoded.images[BANNER_COUNT], decoded.images[0]);
    }

    #[test]
    fn missing_corrupt_and_wrong_sized_faces_fail_preparation() {
        let dir = tempfile::tempdir().unwrap();
        assert!(compile_oreui_panoramas(dir.path()).is_err());
        let path = dir.path().join(BANNERS[0].texture);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, b"not a PNG").unwrap();
        assert!(crop(dir.path(), &BANNERS[0]).is_err());
        image::RgbaImage::new(16, 16).save(path).unwrap();
        assert!(crop(dir.path(), &BANNERS[0]).is_err());
    }
}
