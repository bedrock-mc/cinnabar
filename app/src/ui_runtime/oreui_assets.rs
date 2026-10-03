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

use image::{AnimationDecoder, ImageDecoder, ImageFormat, ImageReader, Limits};
use serde::Deserialize;

/// Side of the packed OreUI page.
pub(crate) const OREUI_PAGE_SIDE: u32 = 3072;
const MAX_ATLAS_JSON_BYTES: u64 = 1024 * 1024;
const MAX_IMAGE_BYTES: u64 = 8 * 1024 * 1024;
const MAX_IMAGE_SIDE: u32 = 1024;
const GUTTER: u32 = 1;
const MAX_ANIMATION_FRAMES: usize = 32;
const MAX_ANIMATION_PIXELS_BYTES: usize = 1024 * 1024;
/// The pixelated loading animation selected by OreUI `ep`.
pub(crate) const LOADING_ANIMATION: &str = "assets/animation-074ed0ba8c16bb30e36c.gif";
/// Standalone category images from the installed OreUI bundle, in sidebar order.
pub(crate) const INBOX_ICONS: [&str; 5] = [
    "assets/News-f81489154ff3c38f5b5f.png",
    "assets/Realms-c7419af9abd527149fcc.png",
    "assets/Invites-6a62211ac071c98b2994.png",
    "assets/MarketplacePass-8eb08ee1dd714dd15307.png",
    "assets/Feedback-ecfce4d670046c25d3df.png",
];

/// Standalone Profile assets named by the version-matched OreUI components.
pub(crate) const PROFILE_STAT_ICONS: [&str; 4] = [
    "assets/IconClockGrey-ea5642bd84ad58714dd4.png",
    "assets/IconPickaxeGrey-cb118f7d544ff4fce2e2.png",
    "assets/IconSwordGrey-9087f66e9056b3b959aa.png",
    "assets/IconBootsGrey-142a375cbe9eecc3879d.png",
];
/// The deterministic fallback banner choices used by OreUI `mZ`.
pub(crate) const PROFILE_BANNERS: [&str; 8] = [
    "assets/screenshot_1-48404d5a8f0097356091.jpg",
    "assets/screenshot_2-7c721593cd419875cf29.jpg",
    "assets/screenshot_3-9dd41022b2ee8d8c2c08.jpg",
    "assets/screenshot_4-72ff980d133dc323944c.jpg",
    "assets/screenshot_5-5636d6b539bf7f23759d.jpg",
    "assets/screenshot_6-380c0205af54e86438ac.jpg",
    "assets/screenshot_7-6cfec1e1e1009ea73447.jpg",
    "assets/screenshot_8-b5f240429090c0ddd613.jpg",
];
/// Profile error art, loaded only from an install at runtime.
pub(crate) const PROFILE_ERRORS: [&str; 3] = [
    "assets/nothing_to_see-1107fc6902173eb98f28.png",
    "assets/generic_error-aa90619c4c1746eb7ac0.png",
    "assets/connection_error-01bc20883b3a2f7e5fb3.png",
];
/// The Overview row art selected by OreUI `g2`.
pub(crate) const PROFILE_SUMMARY_ICONS: [&str; 4] = [
    "assets/friends-9e435522a799e248f3c5.png",
    "assets/followers-bec3d954895866890570.png",
    "assets/gallery-10d21b3ca655e32b1ac8.png",
    "assets/achievements-a42ab3d4b8e49c24b217.png",
];
/// Gamerscore art shared by Overview and achievement cards.
pub(crate) const PROFILE_GAMERSCORE: &str = "assets/gamerscore_icon-53b10130cb6f6c0271cb.png";
/// All standalone Profile images to pack alongside the atlases.
const PROFILE_IMAGES: [&str; 20] = [
    PROFILE_STAT_ICONS[0],
    PROFILE_STAT_ICONS[1],
    PROFILE_STAT_ICONS[2],
    PROFILE_STAT_ICONS[3],
    PROFILE_BANNERS[0],
    PROFILE_BANNERS[1],
    PROFILE_BANNERS[2],
    PROFILE_BANNERS[3],
    PROFILE_BANNERS[4],
    PROFILE_BANNERS[5],
    PROFILE_BANNERS[6],
    PROFILE_BANNERS[7],
    PROFILE_ERRORS[0],
    PROFILE_ERRORS[1],
    PROFILE_ERRORS[2],
    PROFILE_SUMMARY_ICONS[0],
    PROFILE_SUMMARY_ICONS[1],
    PROFILE_SUMMARY_ICONS[2],
    PROFILE_SUMMARY_ICONS[3],
    PROFILE_GAMERSCORE,
];

/// The packed OreUI page: RGBA8 pixels (premultiplied) and each bundle image's
/// pixel rect `[x0, y0, x1, y1]`, keyed by its bundle path (`assets/<name>.png`).
pub(crate) struct OreUiImages {
    pub(crate) rgba: Vec<u8>,
    pub(crate) sprites: HashMap<String, [u16; 4]>,
    pub(crate) loading_frames: Vec<(String, u32)>,
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
    for key in INBOX_ICONS.into_iter().chain(PROFILE_IMAGES) {
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
            return Err("OreUI images do not fit one page".into());
        }
        blit(&mut rgba, side, x, y, &pixels, width, height);
        sprites.insert(
            key.into(),
            [x as u16, y as u16, (x + width) as u16, (y + height) as u16],
        );
        x += width + GUTTER;
        shelf = shelf.max(height);
    }
    let mut loading_frames = Vec::new();
    if dir.join(LOADING_ANIMATION).is_file() {
        for (index, (width, height, pixels, millis)) in decode_animation(&read_bounded(
            &dir.join(LOADING_ANIMATION),
            MAX_IMAGE_BYTES,
        )?)?
        .into_iter()
        .enumerate()
        {
            if x + width > OREUI_PAGE_SIDE {
                x = 0;
                y += shelf + GUTTER;
                shelf = 0;
            }
            if y + height > OREUI_PAGE_SIDE {
                return Err("OreUI animation does not fit one page".into());
            }
            blit(&mut rgba, side, x, y, &pixels, width, height);
            let key = format!("{LOADING_ANIMATION}#{index}");
            sprites.insert(
                key.clone(),
                [x as u16, y as u16, (x + width) as u16, (y + height) as u16],
            );
            loading_frames.push((key, millis));
            x += width + GUTTER;
            shelf = shelf.max(height);
        }
    }
    Ok(OreUiImages {
        rgba,
        sprites,
        loading_frames,
    })
}

/// A decoded GIF frame: width, height, premultiplied RGBA8 pixels and duration in milliseconds.
type AnimationFrame = (u32, u32, Vec<u8>, u32);

/// Decodes bounded GIF frames with their own timing and premultiplied pixels.
fn decode_animation(bytes: &[u8]) -> Result<Vec<AnimationFrame>, String> {
    let mut decoder = image::codecs::gif::GifDecoder::new(Cursor::new(bytes))
        .map_err(|error| error.to_string())?;
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_IMAGE_SIDE);
    limits.max_image_height = Some(MAX_IMAGE_SIDE);
    limits.max_alloc = Some(MAX_ANIMATION_PIXELS_BYTES as u64);
    decoder
        .set_limits(limits)
        .map_err(|error| error.to_string())?;
    let mut frames = Vec::new();
    let mut decoded_bytes = 0;
    for frame in decoder.into_frames() {
        if frames.len() == MAX_ANIMATION_FRAMES {
            return Err("OreUI animation has too many frames".into());
        }
        let frame = frame.map_err(|error| error.to_string())?;
        let (numerator, denominator) = frame.delay().numer_denom_ms();
        let millis = numerator.div_ceil(denominator).max(1);
        let buffer = frame.into_buffer();
        let (width, height) = buffer.dimensions();
        let mut pixels = buffer.into_raw();
        decoded_bytes += pixels.len();
        if decoded_bytes > MAX_ANIMATION_PIXELS_BYTES {
            return Err("OreUI animation pixels are too large".into());
        }
        for pixel in pixels.chunks_exact_mut(4) {
            let alpha = u16::from(pixel[3]);
            for channel in &mut pixel[..3] {
                *channel = ((u16::from(*channel) * alpha + 127) / 255) as u8;
            }
        }
        frames.push((width, height, pixels, millis));
    }
    Ok(frames)
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
    let format = if path.extension().is_some_and(|ext| ext == "jpg") {
        ImageFormat::Jpeg
    } else {
        ImageFormat::Png
    };
    let mut reader = ImageReader::with_format(Cursor::new(bytes), format);
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

    /// Makes authored GIF bytes without requiring any installed assets.
    fn synthetic_animation(count: usize) -> Vec<u8> {
        let mut bytes = Vec::new();
        {
            let mut encoder = image::codecs::gif::GifEncoder::new(&mut bytes);
            for index in 0..count {
                let pixels =
                    image::RgbaImage::from_pixel(2, 2, image::Rgba([index as u8, 120, 40, 255]));
                encoder
                    .encode_frame(image::Frame::from_parts(
                        pixels,
                        0,
                        0,
                        image::Delay::from_numer_denom_ms(100, 1),
                    ))
                    .unwrap();
            }
        }
        bytes
    }

    #[test]
    fn animated_loading_keeps_each_frame_and_delay() {
        let frames = decode_animation(&synthetic_animation(3)).unwrap();
        assert_eq!(frames.len(), 3);
        for (index, (width, height, pixels, millis)) in frames.iter().enumerate() {
            assert_eq!((*width, *height, *millis), (2, 2, 100));
            assert_eq!(&pixels[..4], &[index as u8, 120, 40, 255]);
        }
        assert!(decode_animation(&synthetic_animation(MAX_ANIMATION_FRAMES + 1)).is_err());
        assert!(decode_animation(b"invalid GIF").is_err());
    }

    #[test]
    fn profile_banners_fit_beside_existing_atlas_without_resizing() {
        let dir = std::env::temp_dir().join(format!("oreui-profile-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("assets")).unwrap();
        image::RgbaImage::from_pixel(1024, 1024, image::Rgba([0, 0, 0, 255]))
            .save(dir.join("base.png"))
            .unwrap();
        std::fs::write(
            dir.join("atlas.json"),
            r#"[{"name":"base.png","width":1024,"height":1024,"coordinates":{}}]"#,
        )
        .unwrap();
        // The reference's eight banner images are 960 by 540 pixels.
        for name in PROFILE_BANNERS {
            image::RgbImage::from_pixel(960, 540, image::Rgb([80, 120, 160]))
                .save(dir.join(name))
                .unwrap();
        }
        let images = load(&dir).unwrap();
        for name in PROFILE_BANNERS {
            let [left, top, right, bottom] = images.sprites[name];
            assert_eq!((right - left, bottom - top), (960, 540));
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }

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
