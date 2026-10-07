use std::sync::OnceLock;

use render_model::ActorVertex;

use super::{
    HEAD_PIVOT, PLAYER_EYE_HEIGHT, PLAYER_MODEL_SCALE, PREVIEW_FEET_Y, PREVIEW_HEIGHT,
    PREVIEW_PIXELS_PER_BLOCK, PREVIEW_WIDTH, SHOULDER_PIVOTS, bob_degrees,
};

/// A fixed orbit envelope prevents pointer look and idle sway from resizing the character.
#[derive(Clone, Copy)]
pub(super) struct OrbitBounds {
    radius: f32,
    low: f32,
    high: f32,
}

impl OrbitBounds {
    pub(super) fn new(vertices: &[ActorVertex]) -> Self {
        let mut bounds = Self {
            radius: 0.0,
            low: f32::INFINITY,
            high: f32::NEG_INFINITY,
        };
        let bob = bob_degrees(0.0).to_radians();
        for vertex in vertices {
            let [x, y, z] = vertex.position;
            match vertex.part {
                0 => {
                    let radius = ((x - HEAD_PIVOT[0]).powi(2)
                        + (y - HEAD_PIVOT[1]).powi(2)
                        + (z - HEAD_PIVOT[2]).powi(2))
                    .sqrt();
                    bounds.radius = bounds.radius.max(radius);
                    bounds.low = bounds.low.min(HEAD_PIVOT[1] - radius);
                    bounds.high = bounds.high.max(HEAD_PIVOT[1] + radius);
                }
                2 | 3 => {
                    let pivot = SHOULDER_PIVOTS[(vertex.part - 2) as usize];
                    let (low, high) = if vertex.part == 2 {
                        (-bob, 0.0)
                    } else {
                        (0.0, bob)
                    };
                    let (dx, dy) = (x - pivot[0], y - pivot[1]);
                    let x_extreme = (-dy).atan2(dx);
                    let y_extreme = dx.atan2(dy);
                    for angle in [
                        low,
                        high,
                        x_extreme - std::f32::consts::PI,
                        x_extreme,
                        x_extreme + std::f32::consts::PI,
                        y_extreme - std::f32::consts::PI,
                        y_extreme,
                        y_extreme + std::f32::consts::PI,
                    ] {
                        if angle >= low && angle <= high {
                            let (sin, cos) = angle.sin_cos();
                            bounds.include([
                                pivot[0] + dx * cos - dy * sin,
                                pivot[1] + dx * sin + dy * cos,
                                z,
                            ]);
                        }
                    }
                }
                _ => bounds.include(vertex.position),
            }
        }
        bounds
    }

    fn include(&mut self, [x, y, z]: [f32; 3]) {
        self.radius = self.radius.max(x.hypot(z));
        self.low = self.low.min(y);
        self.high = self.high.max(y);
    }

    pub(super) fn with_static_geometry(mut self, vertices: &[ActorVertex]) -> Self {
        for vertex in vertices {
            self.include(vertex.position);
        }
        self
    }

    pub(super) fn frame(self, control: [f32; 4], tilt: f32) -> [f32; 4] {
        let (sin, cos) = tilt.to_radians().sin_cos();
        let radius = self.radius * PLAYER_MODEL_SCALE;
        let endpoints = [self.low, self.high]
            .map(|y| PLAYER_EYE_HEIGHT + (y * PLAYER_MODEL_SCALE - PLAYER_EYE_HEIGHT) * cos);
        let low = endpoints[0].min(endpoints[1]) - radius * sin.abs();
        let high = endpoints[0].max(endpoints[1]) + radius * sin.abs();
        let projected = [
            PREVIEW_WIDTH as f32 * 0.5 - radius * PREVIEW_PIXELS_PER_BLOCK,
            PREVIEW_FEET_Y - high * PREVIEW_PIXELS_PER_BLOCK,
            PREVIEW_WIDTH as f32 * 0.5 + radius * PREVIEW_PIXELS_PER_BLOCK,
            PREVIEW_FEET_Y - low * PREVIEW_PIXELS_PER_BLOCK,
        ];
        let scale = ((control[2] - control[0]) / (projected[2] - projected[0]))
            .min((control[3] - control[1]) / (projected[3] - projected[1]));
        let left = (control[0] + control[2]) * 0.5 - PREVIEW_WIDTH as f32 * 0.5 * scale;
        let top = control[3] - projected[3] * scale;
        [
            left,
            top,
            left + PREVIEW_WIDTH as f32 * scale,
            top + PREVIEW_HEIGHT as f32 * scale,
        ]
    }
}

pub(super) fn standard() -> OrbitBounds {
    static BOUNDS: OnceLock<OrbitBounds> = OnceLock::new();
    *BOUNDS.get_or_init(|| {
        let mut vertices = render_model::standard_biped_vertices();
        vertices.extend(render_model::standard_biped_overlay_vertices());
        OrbitBounds::new(&vertices)
    })
}

pub(super) fn head_origin(frame: [f32; 4], tilt: f32) -> [f32; 2] {
    let scale = (frame[2] - frame[0]) / PREVIEW_WIDTH as f32;
    let y = PLAYER_EYE_HEIGHT
        + (HEAD_PIVOT[1] * PLAYER_MODEL_SCALE - PLAYER_EYE_HEIGHT) * tilt.to_radians().cos();
    [
        (frame[0] + frame[2]) * 0.5,
        frame[1] + (PREVIEW_FEET_Y - y * PREVIEW_PIXELS_PER_BLOCK) * scale,
    ]
}
