//! Screen-space overlay model: which full-screen layers are visible and how strongly.
//! Alphas, ramps and warp amplitudes are provisional and need native measurement.

use bevy::prelude::{Resource, Vec3};
use sim::{Aabb, BlockPhysicsFlags, CollisionWorld, Vec3 as SimVec3};

pub const PUMPKIN_BLUR_TEXTURE: &str = "textures/misc/pumpkinblur.png";
pub const SPYGLASS_SCOPE_TEXTURE: &str = "textures/ui/spyglass_scope.png";
pub const PORTAL_TEXTURE: &str = "textures/blocks/portal";
pub const FIRE_TEXTURE: &str = "textures/blocks/fire_1";

const PORTAL_RISE_PER_SECOND: f32 = 0.25;
const PORTAL_FALL_PER_SECOND: f32 = 1.0;
const NAUSEA_ROLL_DEGREES: f32 = 4.0;
const NAUSEA_HZ: f32 = 0.35;
const EYE_PROBE_RADIUS: f64 = 0.1;

/// What the eye point is embedded in.
#[derive(Resource, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum HeadMedium {
    #[default]
    Air,
    Water,
    Lava,
    PowderSnow,
    Solid,
}

/// Classifies the medium at `eye`; unreadable cells count as open air so unloaded space never blackens the view.
#[must_use]
pub fn probe_head_medium(world: &impl CollisionWorld, eye: Vec3) -> HeadMedium {
    let block = [eye.x, eye.y, eye.z].map(|axis| axis.floor() as i32);
    if let Ok(sample) = world.block_physics(block) {
        for layer in sample.layers.iter() {
            let submerged = f64::from(eye.y) < f64::from(block[1]) + layer.fluid_height_blocks;
            if layer.flags.contains(BlockPhysicsFlags::LAVA) && submerged {
                return HeadMedium::Lava;
            }
            if layer.flags.contains(BlockPhysicsFlags::WATER) && submerged {
                return HeadMedium::Water;
            }
            if layer.flags.contains(BlockPhysicsFlags::POWDER_SNOW) {
                return HeadMedium::PowderSnow;
            }
        }
    }
    let centre = SimVec3::new(f64::from(eye.x), f64::from(eye.y), f64::from(eye.z));
    let radius = SimVec3::new(EYE_PROBE_RADIUS, EYE_PROBE_RADIUS, EYE_PROBE_RADIUS);
    let probe = Aabb::new(centre - radius, centre + radius);
    let solid = world
        .collision_boxes_camera_lenient(probe)
        .map(|boxes| boxes.value)
        .unwrap_or_default()
        .into_iter()
        .any(|collision| {
            (0..3).all(|axis| {
                collision.min[axis] < centre[axis] && collision.max[axis] > centre[axis]
            })
        });
    if solid {
        HeadMedium::Solid
    } else {
        HeadMedium::Air
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayKind {
    Suffocation,
    PowderSnow,
    Fire,
    Portal,
    PumpkinBlur,
    SpyglassScope,
    ServerFade,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OverlayLayer {
    pub kind: OverlayKind,
    pub alpha: f32,
    pub rgb: [f32; 3],
    /// Vanilla texture to draw; `None` means a flat tint or a texture that must come from context.
    pub texture: Option<&'static str>,
}

/// Every facts input the overlay derivation reads for one frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScreenEffectInputs {
    pub first_person: bool,
    pub head: HeadMedium,
    pub carved_pumpkin_worn: bool,
    pub on_fire: bool,
    pub spyglass_scoping: bool,
    /// Freezing strength `0..=1` from authoritative metadata.
    pub freezing_strength: f32,
    pub portal_progress: f32,
    pub server_fade: Option<([f32; 3], f32)>,
    /// "Distortion effects" scale for portal and nausea presentation.
    pub distortion_scale: f32,
}

impl Default for ScreenEffectInputs {
    fn default() -> Self {
        Self {
            first_person: true,
            head: HeadMedium::Air,
            carved_pumpkin_worn: false,
            on_fire: false,
            spyglass_scoping: false,
            freezing_strength: 0.0,
            portal_progress: 0.0,
            server_fade: None,
            distortion_scale: 1.0,
        }
    }
}

/// Visible overlay layers in back-to-front draw order.
#[derive(Resource, Debug, Default, Clone, PartialEq)]
pub struct ScreenOverlays {
    pub layers: Vec<OverlayLayer>,
}

fn unit(value: f32, default: f32) -> f32 {
    if value.is_finite() {
        value.clamp(0.0, 1.0)
    } else {
        default
    }
}

/// Portal overlay opacity for a `0..=1` portal progress.
#[must_use]
pub fn portal_alpha(progress: f32) -> f32 {
    let progress = unit(progress, 0.0);
    if progress <= 0.0 {
        return 0.0;
    }
    (progress.powi(4) * 0.8 + 0.2).min(1.0)
}

/// Derives the ordered overlay stack for one frame.
#[must_use]
pub fn compute_overlays(inputs: &ScreenEffectInputs) -> Vec<OverlayLayer> {
    let mut layers = Vec::new();
    let distortion = unit(inputs.distortion_scale, 1.0);
    if inputs.first_person {
        if inputs.head == HeadMedium::Solid {
            layers.push(OverlayLayer {
                kind: OverlayKind::Suffocation,
                alpha: 1.0,
                rgb: [0.1, 0.1, 0.1],
                texture: None,
            });
        }
        let freezing = if inputs.head == HeadMedium::PowderSnow {
            1.0
        } else {
            unit(inputs.freezing_strength, 0.0)
        };
        if freezing > 0.0 {
            layers.push(OverlayLayer {
                kind: OverlayKind::PowderSnow,
                alpha: freezing,
                rgb: [0.75, 0.9, 1.0],
                texture: None,
            });
        }
        if inputs.on_fire || inputs.head == HeadMedium::Lava {
            layers.push(OverlayLayer {
                kind: OverlayKind::Fire,
                alpha: 0.9,
                rgb: [1.0; 3],
                texture: Some(FIRE_TEXTURE),
            });
        }
        let portal = portal_alpha(inputs.portal_progress) * distortion;
        if portal > 0.0 {
            layers.push(OverlayLayer {
                kind: OverlayKind::Portal,
                alpha: portal,
                rgb: [1.0; 3],
                texture: Some(PORTAL_TEXTURE),
            });
        }
        if inputs.carved_pumpkin_worn {
            layers.push(OverlayLayer {
                kind: OverlayKind::PumpkinBlur,
                alpha: 1.0,
                rgb: [1.0; 3],
                texture: Some(PUMPKIN_BLUR_TEXTURE),
            });
        }
        if inputs.spyglass_scoping {
            layers.push(OverlayLayer {
                kind: OverlayKind::SpyglassScope,
                alpha: 1.0,
                rgb: [1.0; 3],
                texture: Some(SPYGLASS_SCOPE_TEXTURE),
            });
        }
    }
    if let Some((rgb, alpha)) = inputs.server_fade
        && alpha > 0.0
    {
        layers.push(OverlayLayer {
            kind: OverlayKind::ServerFade,
            alpha: unit(alpha, 0.0),
            rgb,
            texture: None,
        });
    }
    layers
}

/// Portal-transition progress: ramps up inside a portal and drains faster outside.
#[derive(Resource, Debug, Default, Clone, Copy, PartialEq)]
pub struct PortalProgress(f32);

impl PortalProgress {
    #[must_use]
    pub const fn value(&self) -> f32 {
        self.0
    }

    pub fn advance(&mut self, in_portal: bool, delta_seconds: f32) {
        if !(delta_seconds.is_finite() && delta_seconds > 0.0) {
            return;
        }
        let rate = if in_portal {
            PORTAL_RISE_PER_SECOND
        } else {
            -PORTAL_FALL_PER_SECOND
        };
        self.0 = (self.0 + rate * delta_seconds).clamp(0.0, 1.0);
    }
}

/// View roll in radians for the nausea/portal wobble.
#[must_use]
pub fn nausea_roll_radians(time_seconds: f32, strength: f32, distortion_scale: f32) -> f32 {
    if !time_seconds.is_finite() {
        return 0.0;
    }
    let strength = unit(strength, 0.0) * unit(distortion_scale, 1.0);
    (time_seconds * std::f32::consts::TAU * NAUSEA_HZ).sin()
        * strength
        * NAUSEA_ROLL_DEGREES.to_radians()
}

/// Continuous vision-effect strengths for the fog/lighting owners, each `0..=1`.
#[derive(Resource, Debug, Default, Clone, Copy, PartialEq)]
pub struct VisionEffects {
    pub blindness: f32,
    pub darkness: f32,
    pub night_vision: f32,
    pub nausea: f32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_first_person_view_has_no_layers() {
        assert!(compute_overlays(&ScreenEffectInputs::default()).is_empty());
    }

    #[test]
    fn third_person_hides_everything_but_the_server_fade() {
        let layers = compute_overlays(&ScreenEffectInputs {
            first_person: false,
            head: HeadMedium::Solid,
            carved_pumpkin_worn: true,
            on_fire: true,
            spyglass_scoping: true,
            freezing_strength: 1.0,
            server_fade: Some(([0.0; 3], 0.5)),
            ..Default::default()
        });
        assert_eq!(layers.len(), 1);
        assert_eq!(layers[0].kind, OverlayKind::ServerFade);
    }

    #[test]
    fn layers_stack_back_to_front_with_vanilla_textures() {
        let layers = compute_overlays(&ScreenEffectInputs {
            head: HeadMedium::Solid,
            carved_pumpkin_worn: true,
            on_fire: true,
            spyglass_scoping: true,
            freezing_strength: 0.5,
            portal_progress: 1.0,
            server_fade: Some(([0.0; 3], 1.0)),
            ..Default::default()
        });
        let kinds: Vec<_> = layers.iter().map(|layer| layer.kind).collect();
        assert_eq!(
            kinds,
            vec![
                OverlayKind::Suffocation,
                OverlayKind::PowderSnow,
                OverlayKind::Fire,
                OverlayKind::Portal,
                OverlayKind::PumpkinBlur,
                OverlayKind::SpyglassScope,
                OverlayKind::ServerFade,
            ]
        );
        assert_eq!(layers[4].texture, Some(PUMPKIN_BLUR_TEXTURE));
        assert_eq!(layers[5].texture, Some(SPYGLASS_SCOPE_TEXTURE));
    }

    #[test]
    fn distortion_scale_zero_removes_the_portal_layer() {
        let layers = compute_overlays(&ScreenEffectInputs {
            portal_progress: 1.0,
            distortion_scale: 0.0,
            ..Default::default()
        });
        assert!(layers.is_empty());
    }

    #[test]
    fn powder_snow_medium_forces_full_freeze() {
        let layers = compute_overlays(&ScreenEffectInputs {
            head: HeadMedium::PowderSnow,
            ..Default::default()
        });
        assert_eq!(layers[0].alpha, 1.0);
    }

    #[test]
    fn portal_progress_rises_slowly_and_drains_fast() {
        let mut progress = PortalProgress::default();
        progress.advance(true, 2.0);
        assert!((progress.value() - 0.5).abs() < 1e-6);
        progress.advance(false, 0.25);
        assert!((progress.value() - 0.25).abs() < 1e-6);
        progress.advance(false, 10.0);
        assert_eq!(progress.value(), 0.0);
        assert_eq!(portal_alpha(0.0), 0.0);
        assert!((portal_alpha(1.0) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn nausea_roll_is_bounded_and_scaled() {
        let max = NAUSEA_ROLL_DEGREES.to_radians();
        for step in 0..100 {
            let roll = nausea_roll_radians(step as f32 * 0.1, 1.0, 1.0);
            assert!(roll.abs() <= max + 1e-6);
        }
        assert_eq!(nausea_roll_radians(1.0, 1.0, 0.0), 0.0);
        assert_eq!(nausea_roll_radians(f32::NAN, 1.0, 1.0), 0.0);
    }
}
