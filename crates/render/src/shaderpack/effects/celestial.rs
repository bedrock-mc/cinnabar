//! Java's sun, moon and stars (`SkyRenderer.renderSun/renderMoon/renderStars`), relative to
//! the camera like the sky discs.

use super::stars::STAR_QUADS;
use super::vertices::{Mesh, Vertex};
use bevy::math::{Mat3, Vec3};

const SUN_HALF: f32 = 30.0;
const MOON_HALF: f32 = 20.0;
const DISTANCE: f32 = 100.0;

/// What the celestial bodies depend on this frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Celestial {
    /// Java's time of day, 0 at noon.
    pub angle: f32,
    /// The pack's `sunPathRotation`, degrees.
    pub sun_path_rotation: f32,
    pub moon_phase: i32,
    pub rain: f32,
}

impl Celestial {
    /// Java's pose for the sun, moon and stars, with Iris's sun path tilt between its two
    /// rotations: `rotY(-90°)·rotZ(sunPathRotation)·rotX(angle·360°)`.
    pub(super) fn rotation(self) -> Mat3 {
        Mat3::from_rotation_y((-90.0f32).to_radians())
            * Mat3::from_rotation_z(self.sun_path_rotation.to_radians())
            * Mat3::from_rotation_x((self.angle * 360.0).to_radians())
    }

    /// Java's `ClientLevel.getStarBrightness` times the rain fade the sky renderer applies.
    pub(super) fn star_brightness(self) -> f32 {
        let brightness =
            (1.0 - ((self.angle * std::f32::consts::TAU).cos() * 2.0 + 0.25)).clamp(0.0, 1.0);
        brightness * brightness * 0.5 * (1.0 - self.rain)
    }

    fn quad(self, mesh: &mut Mesh, corners: [([f32; 3], [f32; 2]); 4]) {
        let rotation = self.rotation();
        let colour = [1.0, 1.0, 1.0, 1.0 - self.rain];
        let normal = rotation * Vec3::new(0.0, -corners[0].0[1].signum(), 0.0);
        mesh.quad(corners.map(|(position, uv)| Vertex {
            uv,
            mid_uv: [0.5; 2],
            ..Vertex::plain(rotation * Vec3::from_array(position), colour, normal)
        }));
    }

    /// The sun quad, `sun.png` over its whole face.
    pub(super) fn sun(self, mesh: &mut Mesh) {
        let (h, d) = (SUN_HALF, DISTANCE);
        self.quad(
            mesh,
            [
                ([-h, d, -h], [0.0, 0.0]),
                ([h, d, -h], [1.0, 0.0]),
                ([h, d, h], [1.0, 1.0]),
                ([-h, d, h], [0.0, 1.0]),
            ],
        );
    }

    /// The moon quad, opposite the sun, showing `moon_phases.png`'s tile for the phase
    /// (four columns, two rows).
    pub(super) fn moon(self, mesh: &mut Mesh) {
        let phase = self.moon_phase.rem_euclid(8);
        let (column, row) = ((phase % 4) as f32, (phase / 4 % 2) as f32);
        let (u0, v0, u1, v1) = (
            column / 4.0,
            row / 2.0,
            (column + 1.0) / 4.0,
            (row + 1.0) / 2.0,
        );
        let (h, d) = (MOON_HALF, DISTANCE);
        self.quad(
            mesh,
            [
                ([-h, -d, h], [u1, v1]),
                ([h, -d, h], [u0, v1]),
                ([h, -d, -h], [u0, v0]),
                ([-h, -d, -h], [u1, v0]),
            ],
        );
    }

    /// Java's stars in `starBrightness` (its colour modulator in every channel); nothing
    /// when they are not visible.
    pub(super) fn stars(self, mesh: &mut Mesh) {
        let brightness = self.star_brightness();
        if brightness <= 0.0 {
            return;
        }
        let rotation = self.rotation();
        let colour = [brightness; 4];
        for star in STAR_QUADS.iter() {
            let normal = rotation * -star[0].normalize_or_zero();
            mesh.quad(star.map(|corner| Vertex::plain(rotation * corner, colour, normal)));
        }
    }
}
