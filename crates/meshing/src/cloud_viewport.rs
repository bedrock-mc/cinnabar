//! Finite vanilla cloud sampling window, rather than nine full texture periods.
//!
//! 1.26.50.26 rebuilds clouds after fifteen blocks of
//! sampling-space movement. Its task centres a 64*grid window on
//! floor(samplePosition)>>4, and the tessellator emits unit caps
//! and occupied→empty side edges. Alpha bytes greater than one are occupied.

use assets::AtmosphereTexture;

use crate::{CLOUD_CELL_BLOCKS, CLOUD_MASK_SIZE, CloudFace, CloudMeshError, MAX_CLOUD_QUADS};

pub const CLOUD_REBUILD_DISTANCE_SQUARED: f64 = 225.0;
pub const CLOUD_FADE_START: f32 = 0.9;
// Closing a finite window may add one side face per outer edge texel beyond
// the periodic mask's bound. Keep this allowance explicit and finite.
pub const MAX_VIEWPORT_CLOUD_QUADS: usize = MAX_CLOUD_QUADS + CLOUD_MASK_SIZE as usize * 4;
pub const MAX_VIEWPORT_CLOUD_BYTES: usize =
    MAX_VIEWPORT_CLOUD_QUADS * size_of::<ViewportCloudQuad>();

/// Six vertices reconstruct a single native texel face. Signed sampling cells
/// preserve negative coordinates and the256th boundary without truncating an
/// axis to the old periodic record's eight bits.
#[derive(Clone, Copy, Debug, Eq, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
#[repr(C)]
pub struct ViewportCloudQuad {
    pub cell: [i32; 2],
    pub face: u32,
    /// Native tessellator-shaded gamma RGBA8; alpha is the source texel's alpha.
    pub colour: u32,
}

/// An admitted immutable window; continuous scrolling is applied at draw time.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CloudViewport {
    sample: [f64; 2],
    start: [i32; 2],
    span: u16,
    above: bool,
    both_caps: bool,
}

impl CloudViewport {
    #[must_use]
    pub fn try_new(
        sample: [f64; 2],
        mesh_size: u16,
        grid_size: u8,
        above: bool,
        both_caps: bool,
    ) -> Option<Self> {
        let span = mesh_size.checked_mul(u16::from(grid_size))?;
        if span == 0 || span > CLOUD_MASK_SIZE as u16 || span % 2 != 0 {
            return None;
        }
        let half = i32::from(span / 2);
        let cells = sample.map(|coordinate| (coordinate / f64::from(CLOUD_CELL_BLOCKS)).floor());
        if cells.iter().any(|value| {
            !value.is_finite()
                || *value < f64::from(i32::MIN + half + 1)
                || *value > f64::from(i32::MAX - half - 1)
        }) {
            return None;
        }
        Some(Self {
            sample,
            start: cells.map(|value| value as i32 - half),
            span,
            above,
            both_caps,
        })
    }

    #[must_use]
    pub fn needs_rebuild(self, requested: Self) -> bool {
        self.span != requested.span
            || self.above != requested.above
            || self.both_caps != requested.both_caps
            || self
                .sample
                .iter()
                .zip(requested.sample)
                .map(|(a, b)| (a - b).powi(2))
                .sum::<f64>()
                > CLOUD_REBUILD_DISTANCE_SQUARED
    }

    #[must_use]
    pub fn centre(self) -> [f64; 2] {
        self.start.map(|cell| {
            (f64::from(cell) + f64::from(self.span) * 0.5) * f64::from(CLOUD_CELL_BLOCKS)
        })
    }

    #[must_use]
    pub const fn span(self) -> u16 {
        self.span
    }
}

pub fn mesh_cloud_viewport(
    texture: &AtmosphereTexture,
    viewport: CloudViewport,
) -> Result<Vec<ViewportCloudQuad>, CloudMeshError> {
    crate::cloud::validate_texture(texture)?;
    let texel = |x: i32, z: i32| {
        let x = x.rem_euclid(CLOUD_MASK_SIZE as i32) as usize;
        let z = z.rem_euclid(CLOUD_MASK_SIZE as i32) as usize;
        let offset = (z * CLOUD_MASK_SIZE as usize + x) * 4;
        <[u8; 4]>::try_from(&texture.rgba8[offset..offset + 4]).expect("validated RGBA8 texel")
    };
    let end = viewport.start.map(|cell| cell + i32::from(viewport.span));
    // Outer grid-neighbour bits are unset in native tessellation: close the
    // finite window even if its next texture texel is occupied. Internal
    // partition edges have neighbours and never duplicate those side faces.
    let occupied = |x: i32, z: i32| {
        x >= viewport.start[0]
            && x < end[0]
            && z >= viewport.start[1]
            && z < end[1]
            && texel(x, z)[3] > 1
    };
    // Six faces per cell is further bounded by the periodic mask plus a finite
    // boundary allowance. No Vec growth past the admitted GPU budget.
    let mut quads =
        Vec::with_capacity((usize::from(viewport.span).pow(2) * 6).min(MAX_VIEWPORT_CLOUD_QUADS));
    let mut push = |cell: [i32; 2], face: CloudFace| {
        let normal = match face {
            CloudFace::Down => [0.0, -1.0, 0.0],
            CloudFace::Up => [0.0, 1.0, 0.0],
            CloudFace::North | CloudFace::South => [0.0, 0.0, 1.0],
            CloudFace::West | CloudFace::East => [1.0, 0.0, 0.0],
        };
        let shade = crate::cloud::cloud_face_shade(normal);
        let colour = texel(cell[0], cell[1]);
        let colour = [
            ((f32::from(colour[0]) / 255.0) * shade * 255.0) as u8,
            ((f32::from(colour[1]) / 255.0) * shade * 255.0) as u8,
            ((f32::from(colour[2]) / 255.0) * shade * 255.0) as u8,
            colour[3],
        ];
        quads.push(ViewportCloudQuad {
            cell,
            face: face as u32,
            colour: u32::from_le_bytes(colour),
        });
    };
    for z in viewport.start[1]..viewport.start[1] + i32::from(viewport.span) {
        for x in viewport.start[0]..viewport.start[0] + i32::from(viewport.span) {
            if !occupied(x, z) {
                continue;
            }
            if !viewport.above || viewport.both_caps {
                push([x, z], CloudFace::Down);
            }
            if viewport.above || viewport.both_caps {
                push([x, z], CloudFace::Up);
            }
            for (face, neighbour) in [
                (CloudFace::North, [x, z - 1]),
                (CloudFace::South, [x, z + 1]),
                (CloudFace::West, [x - 1, z]),
                (CloudFace::East, [x + 1, z]),
            ] {
                if !occupied(neighbour[0], neighbour[1]) {
                    push([x, z], face);
                }
            }
        }
    }
    if quads.len() > MAX_VIEWPORT_CLOUD_QUADS {
        return Err(CloudMeshError::TooManyQuads {
            actual: quads.len(),
            max: MAX_VIEWPORT_CLOUD_QUADS,
        });
    }
    Ok(quads)
}

const _: () = assert!(size_of::<ViewportCloudQuad>() == 16);

/// The GPU uses the same face discriminants and fade control as the native
/// CPU mesh/atmosphere helpers. No shader-side copy of those shared values.
#[must_use]
pub fn shader_source(source: &str) -> String {
    let mut source = source.replace("CLOUD_FADE_START_VALUE", &format!("{CLOUD_FADE_START:?}"));
    for (token, face) in [
        ("CLOUD_FACE_DOWN_VALUE", CloudFace::Down),
        ("CLOUD_FACE_UP_VALUE", CloudFace::Up),
        ("CLOUD_FACE_NORTH_VALUE", CloudFace::North),
        ("CLOUD_FACE_SOUTH_VALUE", CloudFace::South),
        ("CLOUD_FACE_WEST_VALUE", CloudFace::West),
        ("CLOUD_FACE_EAST_VALUE", CloudFace::East),
    ] {
        source = source.replace(token, &format!("{}u", face as u32));
    }
    source
}
