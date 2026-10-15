//! Collision-box flame geometry shared by world actors and the HUD preview.

use super::{ActorRigGeometry, ActorRigSurface, ActorRigVertex, EntityRigId};

/// Reserved procedural rig, outside the catalog, item and skin cache slots.
pub const FLAME_RIG_ID: EntityRigId = EntityRigId(u32::MAX - 1);
pub const FLAME_SCALE: f32 = 1.4;
pub const FLAME_V_INSET_TEXELS: f32 = 0.05;

/// Draw-based flame clock, paused while an actor is extinguished or absent from the view.
#[derive(Default)]
pub struct FlamePlayback {
    frame: usize,
    countdown: u8,
}

impl FlamePlayback {
    /// Advances at most one frame per visible draw, retaining phase while extinguished.
    pub fn advance(&mut self, burning: bool, frames: usize) {
        if !burning || frames == 0 {
            return;
        }
        self.countdown = self.countdown.saturating_sub(1);
        if self.countdown == 0 {
            self.countdown = 2;
            self.frame = (self.frame + 1) % frames;
        }
    }

    /// Current square-frame index in the admitted atlas.
    #[must_use]
    pub fn frame(&self) -> usize {
        self.frame
    }
}
/// Half width, bottom, top, depth and reversed winding of each authored face.
pub const FLAME_FACES: [(f32, f32, f32, f32, bool); 4] = [
    (0.5, 0.0, 1.4, 0.0, false),
    (0.45, 0.45, 1.85, 0.03, false),
    (0.5, 0.0, 1.4, 0.0, true),
    (0.45, 0.45, 1.85, -0.03, true),
];

/// Camera-facing depth offset before collision-box scaling; HUD reverses its sign.
#[must_use]
pub fn flame_depth(width: f32, height: f32) -> f32 {
    (height / (width * FLAME_SCALE)).floor() * 0.02 - 0.3
}

/// Builds one reusable four-face mesh with the native sub-texel vertical inset.
#[must_use]
pub fn flame_geometry(texture_side: u16) -> ActorRigGeometry {
    let inset = FLAME_V_INSET_TEXELS / f32::from(texture_side.max(1));
    let mut vertices = Vec::with_capacity(FLAME_FACES.len() * 6);
    for (half, low, high, depth, reverse) in FLAME_FACES {
        let positions = [
            [half, low, depth],
            [-half, low, depth],
            [-half, high, depth],
            [half, high, depth],
        ];
        let uvs = [
            [1.0, 1.0 - inset],
            [0.0, 1.0 - inset],
            [0.0, inset],
            [1.0, inset],
        ];
        let corners = [0, 1, 2, 0, 2, 3];
        for corner in corners.map(|corner| if reverse { 3 - corner } else { corner }) {
            vertices.push(ActorRigVertex {
                position: positions[corner],
                normal: [0.0, 0.0, if reverse { -1.0 } else { 1.0 }],
                uv: uvs[corner],
                back_uv: uvs[corner],
                surface: ActorRigSurface::SINGLE_FACE,
                ..Default::default()
            });
        }
    }
    ActorRigGeometry::new(FLAME_RIG_ID, vertices, vec![[0.0; 3]])
        .expect("authored flame geometry is valid")
}
