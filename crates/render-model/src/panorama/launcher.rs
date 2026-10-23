//! Feeds the launcher's panorama pass: decodes the six faces once and turns the
//! camera each frame while a launcher screen is up.

use super::{MAX_PANORAMA_FACE_SIDE, PanoramaFaces, PanoramaView};
use assets::RuntimeUiAssets;
use image::{ImageFormat, ImageReader, Limits};
use std::{
    f32::consts::{PI, TAU},
    io::Cursor,
    path::PathBuf,
};

// These tuning values follow the title-screen cube convention and need native measurement.
/// Vertical field of view of the panorama camera.
const VERTICAL_FOV: f32 = 85.0 * PI / 180.0;
/// Turn rate: 0.1 degrees per 20 Hz tick, turning left.
const TURN_DEGREES_PER_SECOND: f32 = -2.0;
/// Downward tilt, swaying slowly by a few degrees.
const PITCH_DEGREES: f32 = 25.0;
const PITCH_SWAY_DEGREES: f32 = 5.0;
/// Sway phase (radians) per degree turned.
const SWAY_RATE: f32 = 0.001;

/// Directory of `panorama_0..5.{png,jpg}` replacing the pack's faces.
const OVERRIDE_DIR_ENV: &str = "CINNABAR_PANORAMA_DIR";

/// A face set carried by the fetched resource pack.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PanoramaSet {
    Primary,
    Alternate,
}

/// Loads a complete face set through the existing UI carrier.
pub fn pack_faces(assets: &RuntimeUiAssets, set: PanoramaSet) -> Option<PanoramaFaces> {
    let prefix = match set {
        PanoramaSet::Primary => "panorama_",
        PanoramaSet::Alternate => "panorama_alternate_",
    };
    decode_faces(|face| assets.ui_file(&format!("textures/ui/{prefix}{face}.png")))
}

/// The title-screen camera `seconds` after the panorama first showed.
pub fn launcher_view(seconds: f32, aspect: f32, tint: [f32; 4]) -> PanoramaView {
    let turned = seconds * TURN_DEGREES_PER_SECOND;
    let pitch = PITCH_DEGREES + PITCH_SWAY_DEGREES * (turned.abs() * SWAY_RATE).sin();
    PanoramaView {
        yaw_radians: turned.to_radians().rem_euclid(TAU),
        // The pass tilts up for positive pitch; this camera looks down.
        pitch_radians: -pitch.to_radians(),
        vertical_fov_radians: VERTICAL_FOV,
        aspect,
        tint,
    }
}

/// The user's override faces, else the pack's; `None` shows no panorama.
pub fn launcher_faces(assets: &RuntimeUiAssets) -> Option<PanoramaFaces> {
    override_or_pack_faces(
        std::env::var_os(OVERRIDE_DIR_ENV).map(PathBuf::from),
        assets,
    )
}

fn override_or_pack_faces(
    override_dir: Option<PathBuf>,
    assets: &RuntimeUiAssets,
) -> Option<PanoramaFaces> {
    let overridden = override_dir.and_then(|dir| {
        let faces = decode_faces(|face| {
            ["png", "jpg"]
                .iter()
                .find_map(|ext| std::fs::read(dir.join(format!("panorama_{face}.{ext}"))).ok())
        });
        if faces.is_none() {
            tracing::warn!(dir = %dir.display(), "panorama override unreadable; using the pack's faces");
        }
        faces
    });
    // The title screen shows the alternate set when a pack carries one.
    overridden.or_else(|| {
        [PanoramaSet::Alternate, PanoramaSet::Primary]
            .into_iter()
            .find_map(|set| pack_faces(assets, set))
    })
}

/// Decodes PNG or JPEG faces `0..6` at one native size; any missing or odd face drops them all.
/// Faces are appended to one buffer as they decode, so at most one decoded face is held besides it.
pub fn decode_faces<B: AsRef<[u8]>>(
    mut read: impl FnMut(usize) -> Option<B>,
) -> Option<PanoramaFaces> {
    let mut side = None;
    let mut pixels = Vec::new();
    for face in 0..6 {
        let (width, height, decoded) = decode_image(read(face)?.as_ref())?;
        if width != height || side.is_some_and(|side| side != width) {
            return None;
        }
        if side.is_none() {
            pixels.reserve_exact(decoded.len() * 6);
        }
        side = Some(width);
        pixels.extend_from_slice(&decoded);
    }
    PanoramaFaces::from_layer_major(side?, pixels)
}

/// The 1x1 overlay's colour as straight-alpha floats; clear when absent.
pub fn overlay_tint(assets: &RuntimeUiAssets) -> [f32; 4] {
    assets
        .ui_file("textures/ui/panorama_overlay.png")
        .and_then(decode_image)
        .and_then(|(_, _, pixels)| pixels.get(..4).map(|p| p.to_vec()))
        .map_or([0.0; 4], |p| {
            [p[0], p[1], p[2], p[3]].map(|channel| f32::from(channel) / 255.0)
        })
}

/// PNG or JPEG, sniffed from the bytes.
fn decode_image(bytes: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    let format = image::guess_format(bytes).ok()?;
    if !matches!(format, ImageFormat::Png | ImageFormat::Jpeg) {
        return None;
    }
    let mut reader = ImageReader::with_format(Cursor::new(bytes), format);
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_PANORAMA_FACE_SIDE);
    limits.max_image_height = Some(MAX_PANORAMA_FACE_SIDE);
    limits.max_alloc = Some(u64::from(MAX_PANORAMA_FACE_SIDE).pow(2) * 4);
    reader.limits(limits);
    let image = reader.decode().ok()?.into_rgba8();
    let (width, height) = image.dimensions();
    Some((width, height, image.into_raw()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn png(side: u32, rgba: [u8; 4]) -> Vec<u8> {
        let mut bytes = Vec::new();
        image::RgbaImage::from_pixel(side, side, image::Rgba(rgba))
            .write_to(&mut Cursor::new(&mut bytes), ImageFormat::Png)
            .unwrap();
        bytes
    }

    fn write_faces(dir: &Path, side: u32, red: u8) {
        write_face_set(dir, "panorama_", side, red);
    }

    fn write_face_set(dir: &Path, set: &str, side: u32, red: u8) {
        std::fs::create_dir_all(dir).unwrap();
        for face in 0..6u8 {
            let path = dir.join(format!("{set}{face}.png"));
            std::fs::write(path, png(side, [red, face, 0, 255])).unwrap();
        }
    }

    /// A UI carrier compiled from a pack holding only `textures/ui/white.png`, plus faces of
    /// `face_side` when given.
    fn ui_assets(face_side: Option<u32>) -> RuntimeUiAssets {
        ui_assets_with(face_side, false)
    }

    /// Like `ui_assets`, optionally adding an alternate face set of the same side, red 90.
    fn ui_assets_with(face_side: Option<u32>, alternate: bool) -> RuntimeUiAssets {
        let pack = tempfile::tempdir().unwrap();
        let ui = pack.path().join("textures/ui");
        std::fs::create_dir_all(&ui).unwrap();
        std::fs::create_dir_all(pack.path().join("ui")).unwrap();
        std::fs::write(ui.join("white.png"), png(1, [255; 4])).unwrap();
        if let Some(side) = face_side {
            write_faces(&ui, side, 200);
            if alternate {
                write_face_set(&ui, "panorama_alternate_", side, 90);
            }
        }
        let compiled = pack_compiler::compile_ui_assets(pack.path(), br#"{"schema":1}"#).unwrap();
        RuntimeUiAssets::decode(&compiled.bytes).unwrap()
    }

    #[test]
    fn no_override_and_no_pack_faces_show_no_panorama() {
        assert!(override_or_pack_faces(None, &ui_assets(None)).is_none());
    }

    #[test]
    fn pack_faces_feed_the_panorama_without_an_override() {
        let faces = override_or_pack_faces(None, &ui_assets(Some(4))).unwrap();
        assert_eq!(faces.side(), 4);
        let face_bytes = 4 * 4 * 4;
        for (face, pixels) in faces.layer_major().chunks_exact(face_bytes).enumerate() {
            assert_eq!(&pixels[..4], &[200, face as u8, 0, 255]);
        }
    }

    #[test]
    fn the_alternate_set_is_preferred_when_the_pack_has_one() {
        let faces = override_or_pack_faces(None, &ui_assets_with(Some(4), true)).unwrap();
        assert_eq!(faces.layer_major()[0], 90);
    }

    #[test]
    fn override_faces_replace_the_pack_and_an_unreadable_override_falls_back() {
        let dir = tempfile::tempdir().unwrap();
        write_faces(dir.path(), 2, 7);
        let assets = ui_assets(Some(4));
        let faces = override_or_pack_faces(Some(dir.path().to_path_buf()), &assets).unwrap();
        assert_eq!((faces.side(), faces.layer_major()[0]), (2, 7));
        let missing = dir.path().join("missing");
        let faces = override_or_pack_faces(Some(missing), &assets).unwrap();
        assert_eq!(faces.side(), 4);
    }

    #[test]
    fn pngs_decode_to_rgba_with_their_size() {
        let bytes = png(2, [10, 20, 30, 40]);
        let (width, height, pixels) = decode_image(&bytes).unwrap();
        assert_eq!((width, height), (2, 2));
        assert_eq!(&pixels[..4], &[10, 20, 30, 40]);
        assert!(decode_image(b"not a png").is_none());
    }
}
