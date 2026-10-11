//! Menu panorama cube faces and view, validated on the CPU.

/// Largest accepted face side.
pub const MAX_PANORAMA_FACE_SIDE: u32 = 2048;

/// The six square sRGB RGBA8 cube faces, in pack order (`panorama_0`..`panorama_5`).
#[derive(Debug, PartialEq, Eq)]
pub struct PanoramaFaces {
    side: u32,
    pixels: Vec<u8>,
}

impl PanoramaFaces {
    /// Rejects faces that are not all exactly `side` x `side` RGBA8.
    pub fn new(side: u32, faces: [Vec<u8>; 6]) -> Option<Self> {
        if side == 0 || side > MAX_PANORAMA_FACE_SIDE {
            return None;
        }
        let bytes = side as usize * side as usize * 4;
        if faces.iter().any(|face| face.len() != bytes) {
            return None;
        }
        Some(Self {
            side,
            pixels: faces.concat(),
        })
    }

    #[must_use]
    pub const fn side(&self) -> u32 {
        self.side
    }

    /// Face pixels, one face after another.
    #[must_use]
    pub fn layer_major(&self) -> &[u8] {
        &self.pixels
    }
}

/// Where the panorama camera looks this frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PanoramaView {
    pub yaw_radians: f32,
    pub pitch_radians: f32,
    pub vertical_fov_radians: f32,
    /// Viewport width over height.
    pub aspect: f32,
    /// Overlay tint composited over the faces (straight alpha).
    pub tint: [f32; 4],
}

impl PanoramaView {
    /// The `Panorama` uniform of `panorama.wgsl`: yaw, pitch, tan(half fov), aspect, then tint.
    #[must_use]
    pub fn shader_uniform(&self) -> [f32; 8] {
        let [r, g, b, a] = self.tint;
        [
            self.yaw_radians,
            self.pitch_radians,
            (self.vertical_fov_radians * 0.5).tan(),
            self.aspect,
            r,
            g,
            b,
            a,
        ]
    }
}

mod launcher;
pub use launcher::{built_in_faces, launcher_faces, launcher_view, overlay_tint};

/// Panorama shader shared by the setup window and game renderer.
pub const PANORAMA_WGSL: &str = include_str!("panorama.wgsl");
