//! Static menu artwork prepared from the same pack faces as the menu background.

use super::{OreUiImages, OreUiPage, OreUiSprite};
use assets::RuntimeUiAssets;
use render_model::{
    PanoramaFaces, PanoramaView,
    panorama::{PanoramaSet, pack_faces},
};
use std::sync::Arc;

const PROFILE_BANNER_COUNT: usize = 8;
const VIEW_SIZE: [u32; 2] = [640, 360];
const COLUMNS: usize = 3;
const GUTTER: u32 = 2;

/// One perspective camera and its stable sprite key.
pub struct ArtworkView {
    pub key: &'static str,
    pub set: PanoramaSet,
    pub yaw_degrees: f32,
    pub pitch_degrees: f32,
    pub vertical_fov_degrees: f32,
}

impl ArtworkView {
    /// Projects this camera at the image slots' 16:9 aspect, without the menu's dark tint.
    pub fn render(&self, faces: &PanoramaFaces) -> Vec<u8> {
        faces
            .project(
                PanoramaView {
                    yaw_radians: self.yaw_degrees.to_radians(),
                    pitch_radians: self.pitch_degrees.to_radians(),
                    vertical_fov_radians: self.vertical_fov_degrees.to_radians(),
                    aspect: VIEW_SIZE[0] as f32 / VIEW_SIZE[1] as f32,
                    tint: [0.0; 4],
                },
                VIEW_SIZE,
            )
            .expect("static artwork cameras are valid")
    }
}

/// Eight account banners followed by the Create World river-valley view.
pub const VIEWS: [ArtworkView; 9] = [
    ArtworkView {
        key: "@panorama/profile/0",
        set: PanoramaSet::Primary,
        yaw_degrees: 180.0,
        pitch_degrees: -8.0,
        vertical_fov_degrees: 48.0,
    },
    ArtworkView {
        key: "@panorama/profile/1",
        set: PanoramaSet::Primary,
        yaw_degrees: 150.0,
        pitch_degrees: -5.0,
        vertical_fov_degrees: 52.0,
    },
    ArtworkView {
        key: "@panorama/profile/2",
        set: PanoramaSet::Primary,
        yaw_degrees: 210.0,
        pitch_degrees: -8.0,
        vertical_fov_degrees: 52.0,
    },
    ArtworkView {
        key: "@panorama/profile/3",
        set: PanoramaSet::Primary,
        yaw_degrees: 105.0,
        pitch_degrees: -10.0,
        vertical_fov_degrees: 55.0,
    },
    ArtworkView {
        key: "@panorama/profile/4",
        set: PanoramaSet::Primary,
        yaw_degrees: 310.0,
        pitch_degrees: -6.0,
        vertical_fov_degrees: 55.0,
    },
    ArtworkView {
        key: "@panorama/profile/5",
        set: PanoramaSet::Primary,
        yaw_degrees: 15.0,
        pitch_degrees: -8.0,
        vertical_fov_degrees: 60.0,
    },
    ArtworkView {
        key: "@panorama/profile/6",
        set: PanoramaSet::Alternate,
        yaw_degrees: 0.0,
        pitch_degrees: -12.0,
        vertical_fov_degrees: 48.0,
    },
    ArtworkView {
        key: "@panorama/profile/7",
        set: PanoramaSet::Alternate,
        yaw_degrees: 90.0,
        pitch_degrees: -14.0,
        vertical_fov_degrees: 48.0,
    },
    ArtworkView {
        key: "@panorama/create-world",
        set: PanoramaSet::Primary,
        yaw_degrees: 178.0,
        pitch_degrees: -7.0,
        vertical_fov_degrees: 50.0,
    },
];
const _: () = assert!(VIEWS.len() == PROFILE_BANNER_COUNT + 1);

/// The shared account hash chooses a stable banner independently of which faces are available.
pub(crate) fn profile_key(xuid: &str) -> &'static str {
    VIEWS[launcher::menu::profile_banner_index(xuid, PROFILE_BANNER_COUNT).unwrap()].key
}

/// Sprite drawn above Create when no edited-world thumbnail takes priority.
pub(crate) const CREATE_WORLD_KEY: &str = VIEWS[PROFILE_BANNER_COUNT].key;

/// Generates the views once during startup and appends one bounded, immutable texture page.
/// Missing sets leave their slots plain. `available_bytes` excludes other runtime textures.
pub fn install(images: &mut OreUiImages, assets: Option<&RuntimeUiAssets>, available_bytes: usize) {
    let bright = assets.and_then(|assets| pack_faces(assets, PanoramaSet::Primary));
    let dungeon = assets.and_then(|assets| pack_faces(assets, PanoramaSet::Alternate));
    if bright.is_none() || dungeon.is_none() {
        tracing::warn!(
            "Some panorama artwork is unavailable; keeping plain panels for missing views"
        );
    }
    append(images, bright.as_ref(), dungeon.as_ref(), available_bytes);
}

/// Packs available views with a small transparent gutter, preserving the fixed texture budget.
fn append(
    images: &mut OreUiImages,
    bright: Option<&PanoramaFaces>,
    dungeon: Option<&PanoramaFaces>,
    available_bytes: usize,
) {
    if bright.is_none() && dungeon.is_none() {
        return;
    }
    let width = (VIEW_SIZE[0] + GUTTER) * COLUMNS as u32;
    let height = (VIEW_SIZE[1] + GUTTER) * VIEWS.len().div_ceil(COLUMNS) as u32;
    let bytes = width as usize * height as usize * 4;
    let existing: usize = images.pages.iter().map(|page| page.pixels.len()).sum();
    if existing + bytes > available_bytes {
        tracing::warn!("Panorama artwork exceeds the fixed texture budget; keeping plain panels");
        return;
    }
    let page = images.pages.len() as u16;
    let mut pixels = vec![0; bytes];
    let sprites = Arc::make_mut(&mut images.sprites);
    for (index, view) in VIEWS.iter().enumerate() {
        let Some(faces) = (match view.set {
            PanoramaSet::Primary => bright,
            PanoramaSet::Alternate => dungeon,
        }) else {
            continue;
        };
        let rendered = view.render(faces);
        let x = (index % COLUMNS) as u32 * (VIEW_SIZE[0] + GUTTER);
        let y = (index / COLUMNS) as u32 * (VIEW_SIZE[1] + GUTTER);
        for row in 0..VIEW_SIZE[1] as usize {
            let src = row * VIEW_SIZE[0] as usize * 4;
            let dest = ((y as usize + row) * width as usize + x as usize) * 4;
            pixels[dest..dest + VIEW_SIZE[0] as usize * 4]
                .copy_from_slice(&rendered[src..src + VIEW_SIZE[0] as usize * 4]);
        }
        sprites.insert(
            view.key.into(),
            OreUiSprite {
                page,
                bounds: [
                    x as u16,
                    y as u16,
                    (x + VIEW_SIZE[0]) as u16,
                    (y + VIEW_SIZE[1]) as u16,
                ],
            },
        );
    }
    images.pages.push(OreUiPage {
        dimensions: [width, height],
        pixels: pixels.into(),
    });
}

#[cfg(test)]
mod tests;
