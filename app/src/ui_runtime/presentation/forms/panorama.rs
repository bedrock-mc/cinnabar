//! Feeds the launcher's panorama pass: decodes the six faces once and turns the
//! camera each frame while a launcher screen is up.

use std::{
    f32::consts::{PI, TAU},
    io::Cursor,
    sync::Arc,
    time::Instant,
};

use assets::RuntimeUiAssets;
use bevy::{
    prelude::{Local, Query, Res, ResMut, With},
    window::{PrimaryWindow, Window},
};
use image::{ImageFormat, ImageReader, Limits};
use render::{MAX_PANORAMA_FACE_SIDE, PanoramaFaces, PanoramaScene, PanoramaView};

use super::super::UiPresentationRuntime;
use crate::menu::MenuRuntime;
use crate::ui_runtime::UiRuntime;

// The reconstruction keeps these tuning values as unnamed data; they follow the
// title-screen cube convention and need native measurement.
/// Vertical field of view of the panorama camera.
const VERTICAL_FOV: f32 = 85.0 * PI / 180.0;
/// Turn rate: 0.1 degrees per 20 Hz tick, turning left.
const TURN_DEGREES_PER_SECOND: f32 = -2.0;
/// Downward tilt, swaying slowly by a few degrees.
const PITCH_DEGREES: f32 = 25.0;
const PITCH_SWAY_DEGREES: f32 = 5.0;
/// Sway phase (radians) per degree turned.
const SWAY_RATE: f32 = 0.001;

/// Cinnabar's own panorama, in vanilla face order (-Z, +X, +Z, -X, up, down).
const BUILT_IN_FACES: [&[u8]; 6] = [
    include_bytes!("../../../../../assets/panorama/panorama_0.jpg"),
    include_bytes!("../../../../../assets/panorama/panorama_1.jpg"),
    include_bytes!("../../../../../assets/panorama/panorama_2.jpg"),
    include_bytes!("../../../../../assets/panorama/panorama_3.jpg"),
    include_bytes!("../../../../../assets/panorama/panorama_4.jpg"),
    include_bytes!("../../../../../assets/panorama/panorama_5.jpg"),
];
/// Directory of `panorama_0..5.{png,jpg}` replacing the built-in faces.
const OVERRIDE_DIR_ENV: &str = "CINNABAR_PANORAMA_DIR";

/// Uploads the faces on first sight of the carrier and shows the panorama
/// behind launcher screens (never behind the in-game pause or death screens).
pub(crate) fn drive_menu_panorama(
    player_runtime: bevy::prelude::Res<crate::player_runtime::PlayerRuntime>,
    presentation: Res<UiPresentationRuntime>,
    runtime: Option<Res<UiRuntime>>,
    menu: Option<Res<MenuRuntime>>,
    scene: Option<ResMut<PanoramaScene>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut state: Local<Option<(Instant, f32, [f32; 4])>>,
) {
    let Some(mut scene) = scene else {
        return;
    };
    scene.set_game_visible(crate::screen_policy::renders_game(
        &player_runtime,
        runtime.as_deref(),
        menu.as_deref(),
        Some(&presentation),
    ));
    let Some(engine) = presentation.form_presentation.engine.as_deref() else {
        scene.show(None);
        return;
    };
    if state.is_none() {
        let assets = engine.assets();
        *state = Some((Instant::now(), 0.0, overlay_tint(assets)));
        scene.set_faces(launcher_faces(assets).map(Arc::new));
    }
    let shown = menu.as_deref().is_some_and(MenuRuntime::uses_panorama);
    let aspect = windows
        .iter()
        .next()
        .map(|window| window.width() / window.height().max(1.0))
        .unwrap_or(16.0 / 9.0);
    let Some((last, seconds, tint)) = state.as_mut() else {
        return;
    };
    let now = Instant::now();
    let speed = menu.as_ref().map_or(1.0, |menu| {
        menu.settings_snapshot().0.value("panorama_speed") as f32 / 100.0
    });
    *seconds += now.duration_since(*last).as_secs_f32() * speed;
    *last = now;
    let has_faces = scene.has_faces();
    scene.show((shown && has_faces).then(|| launcher_view(*seconds, aspect, *tint)));
}

/// The title-screen camera `seconds` after the panorama first showed.
pub(crate) fn launcher_view(seconds: f32, aspect: f32, tint: [f32; 4]) -> PanoramaView {
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

/// Cinnabar's own faces, decoded.
pub(crate) fn built_in_faces() -> Option<PanoramaFaces> {
    decode_faces(|face| Some(BUILT_IN_FACES[face].to_vec()))
}

/// The user's override faces, else the built-in ones, else the pack's.
fn launcher_faces(assets: &RuntimeUiAssets) -> Option<PanoramaFaces> {
    let overridden = std::env::var_os(OVERRIDE_DIR_ENV).and_then(|dir| {
        let dir = std::path::PathBuf::from(dir);
        let faces = decode_faces(|face| {
            ["png", "jpg"]
                .iter()
                .find_map(|ext| std::fs::read(dir.join(format!("panorama_{face}.{ext}"))).ok())
        });
        if faces.is_none() {
            bevy::log::warn!(dir = %dir.display(), "panorama override unreadable; using the built-in faces");
        }
        faces
    });
    overridden.or_else(built_in_faces).or_else(|| {
        decode_faces(|face| {
            assets
                .ui_file(&format!("textures/ui/panorama_{face}.png"))
                .map(<[u8]>::to_vec)
        })
    })
}

/// The six faces at their native, equal size; any missing or odd face drops them all.
fn decode_faces(mut read: impl FnMut(usize) -> Option<Vec<u8>>) -> Option<PanoramaFaces> {
    let mut side = None;
    let mut faces = Vec::with_capacity(6);
    for face in 0..6 {
        let (width, height, pixels) = decode_image(&read(face)?)?;
        if width != height || side.is_some_and(|side| side != width) {
            return None;
        }
        side = Some(width);
        faces.push(pixels);
    }
    let faces: [Vec<u8>; 6] = faces.try_into().ok()?;
    PanoramaFaces::new(side?, faces)
}

/// The 1x1 overlay's colour as straight-alpha floats; clear when absent.
fn overlay_tint(assets: &RuntimeUiAssets) -> [f32; 4] {
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

    #[test]
    fn pngs_decode_to_rgba_with_their_size() {
        let mut bytes = Vec::new();
        image::RgbaImage::from_pixel(2, 2, image::Rgba([10, 20, 30, 40]))
            .write_to(&mut Cursor::new(&mut bytes), ImageFormat::Png)
            .unwrap();
        let (width, height, pixels) = decode_image(&bytes).unwrap();
        assert_eq!((width, height), (2, 2));
        assert_eq!(&pixels[..4], &[10, 20, 30, 40]);
        assert!(decode_image(b"not a png").is_none());
    }
}
