//! Dev-only local-originals mode: with `CINNABAR_OREUI_LOCAL_ASSETS` set, the
//! OreUI sprite atlases are read at runtime from the developer's own Minecraft
//! install and packed into one UI page, so the drawn look can be compared with
//! the originals. The images are Mojang's: never copied, packed or shipped.

use std::{
    collections::HashMap,
    fs::File,
    io::{Cursor, Read},
    path::{Path, PathBuf},
};

use image::{ImageFormat, ImageReader, Limits};
use serde::Deserialize;

/// Side of the packed OreUI page.
pub(crate) const OREUI_PAGE_SIDE: u32 = 1024;
const MAX_ATLAS_JSON_BYTES: u64 = 1024 * 1024;
const MAX_IMAGE_BYTES: u64 = 8 * 1024 * 1024;
const MAX_IMAGE_SIDE: u32 = 1024;
const GUTTER: u32 = 1;
/// Standalone category images from the installed OreUI bundle, in sidebar order.
pub(crate) const INBOX_ICONS: [&str; 5] = [
    "assets/News-f81489154ff3c38f5b5f.png",
    "assets/Realms-c7419af9abd527149fcc.png",
    "assets/Invites-6a62211ac071c98b2994.png",
    "assets/MarketplacePass-8eb08ee1dd714dd15307.png",
    "assets/Feedback-ecfce4d670046c25d3df.png",
];

/// The packed OreUI page: RGBA8 pixels (premultiplied) and each bundle image's
/// pixel rect `[x0, y0, x1, y1]`, keyed by its bundle path (`assets/<name>.png`).
pub(crate) struct OreUiImages {
    pub(crate) rgba: Vec<u8>,
    pub(crate) sprites: HashMap<String, [u16; 4]>,
}

#[derive(Deserialize)]
struct AtlasFile {
    name: String,
    width: u32,
    height: u32,
    coordinates: HashMap<String, AtlasRect>,
}

#[derive(Deserialize)]
struct AtlasRect {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

/// The bundle named by `CINNABAR_OREUI_LOCAL_ASSETS`: an install root or its
/// `data/gui/dist/hbui`. Dev-only; nothing is read without it.
fn bundle_dir() -> Option<PathBuf> {
    let root = PathBuf::from(std::env::var_os("CINNABAR_OREUI_LOCAL_ASSETS")?);
    [root.clone(), root.join("data/gui/dist/hbui")]
        .into_iter()
        .find(|dir| dir.join("atlas.json").is_file())
}

/// The packed originals when the dev mode names a usable bundle.
pub(crate) fn load_optional_oreui_images() -> Option<OreUiImages> {
    let Some(dir) = bundle_dir() else {
        if std::env::var_os("CINNABAR_OREUI_LOCAL_ASSETS").is_some() {
            eprintln!(
                "CINNABAR_OREUI_LOCAL_ASSETS has no OreUI bundle; OreUI screens use the drawn look"
            );
        }
        return None;
    };
    match load(&dir) {
        Ok(images) => {
            eprintln!(
                "OreUI local-originals mode: {} ({} sprites)",
                dir.display(),
                images.sprites.len()
            );
            Some(images)
        }
        Err(reason) => {
            eprintln!(
                "OreUI bundle at {} unusable ({reason}); OreUI screens use the drawn look",
                dir.display()
            );
            None
        }
    }
}

fn load(dir: &Path) -> Result<OreUiImages, String> {
    let json = read_bounded(&dir.join("atlas.json"), MAX_ATLAS_JSON_BYTES)?;
    let atlases: Vec<AtlasFile> =
        serde_json::from_slice(&json).map_err(|error| format!("atlas.json: {error}"))?;
    let side = OREUI_PAGE_SIDE as usize;
    let mut rgba = vec![0u8; side * side * 4];
    let mut sprites = HashMap::new();
    let (mut x, mut y, mut shelf) = (0u32, 0u32, 0u32);
    for atlas in &atlases {
        let (width, height, pixels) = decode(&dir.join(&atlas.name))?;
        if width != atlas.width || height != atlas.height {
            return Err(format!("{} does not match atlas.json", atlas.name));
        }
        if x + width > OREUI_PAGE_SIDE {
            x = 0;
            y += shelf + GUTTER;
            shelf = 0;
        }
        if y + height > OREUI_PAGE_SIDE {
            return Err("atlases do not fit one page".to_owned());
        }
        blit(&mut rgba, side, x, y, &pixels, width, height);
        for (path, rect) in &atlas.coordinates {
            if rect.x + rect.width > width || rect.y + rect.height > height {
                continue;
            }
            let (left, top) = ((x + rect.x) as u16, (y + rect.y) as u16);
            sprites.insert(
                path.clone(),
                [
                    left,
                    top,
                    left + rect.width as u16,
                    top + rect.height as u16,
                ],
            );
        }
        x += width + GUTTER;
        shelf = shelf.max(height);
    }
    for key in INBOX_ICONS {
        if !dir.join(key).is_file() {
            continue;
        }
        let (width, height, pixels) = decode(&dir.join(key))?;
        if x + width > OREUI_PAGE_SIDE {
            x = 0;
            y += shelf + GUTTER;
            shelf = 0;
        }
        if y + height > OREUI_PAGE_SIDE {
            return Err("inbox images do not fit one page".into());
        }
        blit(&mut rgba, side, x, y, &pixels, width, height);
        sprites.insert(
            key.into(),
            [x as u16, y as u16, (x + width) as u16, (y + height) as u16],
        );
        x += width + GUTTER;
        shelf = shelf.max(height);
    }
    Ok(OreUiImages { rgba, sprites })
}

fn blit(target: &mut [u8], side: usize, x: u32, y: u32, source: &[u8], width: u32, height: u32) {
    let row = width as usize * 4;
    for line in 0..height as usize {
        let start = ((y as usize + line) * side + x as usize) * 4;
        target[start..start + row].copy_from_slice(&source[line * row..(line + 1) * row]);
    }
}

fn read_bounded(path: &Path, max: u64) -> Result<Vec<u8>, String> {
    let file = File::open(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let mut bytes = Vec::new();
    file.take(max + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() as u64 > max {
        return Err(format!("{} is too large", path.display()));
    }
    Ok(bytes)
}

/// A PNG as premultiplied RGBA8, matching the UI pipeline's blending.
fn decode(path: &Path) -> Result<(u32, u32, Vec<u8>), String> {
    let bytes = read_bounded(path, MAX_IMAGE_BYTES)?;
    let mut reader = ImageReader::with_format(Cursor::new(bytes), ImageFormat::Png);
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_IMAGE_SIDE);
    limits.max_image_height = Some(MAX_IMAGE_SIDE);
    reader.limits(limits);
    let image = reader
        .decode()
        .map_err(|error| format!("{}: {error}", path.display()))?
        .into_rgba8();
    let (width, height) = image.dimensions();
    let mut pixels = image.into_raw();
    for pixel in pixels.chunks_exact_mut(4) {
        let alpha = u16::from(pixel[3]);
        for channel in &mut pixel[..3] {
            *channel = ((u16::from(*channel) * alpha + 127) / 255) as u8;
        }
    }
    Ok((width, height, pixels))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atlases_pack_side_by_side_with_their_sprite_rects() {
        let dir = std::env::temp_dir().join(format!("oreui-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        image::RgbaImage::from_pixel(4, 2, image::Rgba([255, 0, 0, 255]))
            .save(dir.join("a.png"))
            .unwrap();
        image::RgbaImage::from_pixel(2, 2, image::Rgba([0, 255, 0, 128]))
            .save(dir.join("b.png"))
            .unwrap();
        std::fs::write(
            dir.join("atlas.json"),
            r#"[{"name":"a.png","width":4,"height":2,"size":1,"coordinates":{"assets/x.png":{"x":1,"y":0,"width":2,"height":2}}},
               {"name":"b.png","width":2,"height":2,"size":1,"coordinates":{"assets/y.png":{"x":0,"y":0,"width":2,"height":2},"assets/bad.png":{"x":1,"y":1,"width":5,"height":5}}}]"#,
        )
        .unwrap();
        let images = load(&dir).unwrap();
        assert_eq!(images.sprites["assets/x.png"], [1, 0, 3, 2]);
        assert_eq!(images.sprites["assets/y.png"], [5, 0, 7, 2]);
        assert!(!images.sprites.contains_key("assets/bad.png"));
        // Premultiplied: half-alpha green halves its channel.
        let pixel = (5 * 4) as usize;
        assert_eq!(&images.rgba[pixel..pixel + 4], &[0, 128, 0, 128]);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
