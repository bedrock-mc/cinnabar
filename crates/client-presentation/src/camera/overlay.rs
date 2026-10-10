//! Screen-space overlay model: which full-screen layers are visible and how strongly.
//! Fire color and alpha and portal progress follow vanilla screen effects.

use bevy::prelude::{Resource, Vec3};
use sim::{Aabb, BlockPhysicsFlags, CollisionWorld, Vec3 as SimVec3};

pub const PUMPKIN_BLUR_TEXTURE: &str = "textures/misc/pumpkinblur.png";
pub const SPYGLASS_SCOPE_TEXTURE: &str = "textures/ui/spyglass_scope.png";
pub const PORTAL_TEXTURE: &str = "textures/blocks/portal";

// Progress follows fixed player ticks with presentation interpolation.
const PORTAL_RISE_PER_TICK: f32 = 0.0125;
const PORTAL_FALL_PER_TICK: f32 = 0.05;
const CONFUSION_RISE_PER_TICK: f32 = 1.0 / 150.0;
// The portal travel cooldown is fifteen seconds at the fixed player tick rate.
const PORTAL_COOLDOWN_TICKS: u32 = 300;
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
    /// Confusion shares portal progress for the camera, but suppresses its texture overlay.
    pub confusion_active: bool,
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
            confusion_active: false,
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
        if inputs.on_fire {
            layers.push(OverlayLayer {
                kind: OverlayKind::Fire,
                alpha: 0.9,
                rgb: [1.0; 3],
                // Native resolves the fire block's destruction sprite from the active atlas.
                texture: None,
            });
        }
        let portal = if inputs.confusion_active {
            0.0
        } else {
            portal_alpha(inputs.portal_progress)
        };
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
pub struct PortalProgress {
    previous: f32,
    current: f32,
    accumulated_seconds: f64,
    elapsed_ticks: u64,
    session_id: Option<u64>,
    dimension: Option<i32>,
    cooldown_ticks: u32,
    contact_latched: bool,
    pub(super) confusion_active: bool,
}

impl PortalProgress {
    pub(super) fn observe_session(&mut self, session_id: Option<u64>) {
        if self.session_id != session_id {
            *self = Self {
                session_id,
                ..Self::default()
            };
        }
    }

    pub(super) fn elapsed_ticks(&self) -> f32 {
        self.elapsed_ticks as f32
            + (self.accumulated_seconds * f64::from(world::TICKS_PER_SECOND)) as f32
    }

    pub(super) const fn contact_state(&self) -> (u32, bool) {
        (self.cooldown_ticks, self.contact_latched)
    }

    pub(super) fn observe_dimension(&mut self, dimension: Option<i32>) {
        // Every transfer involving the Nether reloads the travel cooldown.
        if let (Some(previous), Some(current)) = (self.dimension, dimension)
            && previous != current
            && (previous == protocol::NETHER_DIMENSION_ID
                || current == protocol::NETHER_DIMENSION_ID)
        {
            self.cooldown_ticks = PORTAL_COOLDOWN_TICKS;
        }
        self.dimension = dimension;
    }

    #[must_use]
    pub fn value(&self) -> f32 {
        let partial = (self.accumulated_seconds * f64::from(world::TICKS_PER_SECOND)) as f32;
        self.previous + (self.current - self.previous) * partial.clamp(0.0, 1.0)
    }

    pub fn advance(&mut self, in_portal: bool, delta_seconds: f32) {
        self.advance_with_confusion(in_portal, None, delta_seconds);
    }

    pub(super) fn advance_with_confusion(
        &mut self,
        in_portal: bool,
        confusion_duration_ticks: Option<i32>,
        delta_seconds: f32,
    ) {
        // Removing confusion outside a portal immediately clears both samples.
        if self.confusion_active && confusion_duration_ticks.is_none() && !in_portal {
            self.previous = 0.0;
            self.current = 0.0;
            self.contact_latched = false;
        }
        self.confusion_active = confusion_duration_ticks.is_some();
        // Server-owned travel can allow fresh entry during our local cooldown.
        // Physical contact must still produce visual feedback on that entry.
        if in_portal {
            self.contact_latched = true;
        }
        if !(delta_seconds.is_finite() && delta_seconds > 0.0) {
            return;
        }
        let tick_seconds = 1.0 / f64::from(world::TICKS_PER_SECOND);
        self.accumulated_seconds += f64::from(delta_seconds);
        let ticks = ((self.accumulated_seconds + f64::EPSILON) / tick_seconds).floor() as u64;
        self.elapsed_ticks = self.elapsed_ticks.saturating_add(ticks);
        self.accumulated_seconds =
            (self.accumulated_seconds - ticks as f64 * tick_seconds).max(0.0);
        // Cooldown plus every progress rate reaches a stable state within this
        // bound. Keep catch-up bounded while still completing cooldown expiry.
        let catch_up_limit = u64::from(PORTAL_COOLDOWN_TICKS) + 151;
        for _ in 0..ticks.min(catch_up_limit) {
            // Consume the contact latch before clearing it, retaining one tick on exit.
            if in_portal {
                self.contact_latched = true;
            }
            let change = if self.contact_latched {
                PORTAL_RISE_PER_TICK
            } else if confusion_duration_ticks.is_some_and(|ticks| !(0..=60).contains(&ticks)) {
                CONFUSION_RISE_PER_TICK
            } else {
                -PORTAL_FALL_PER_TICK
            };
            self.previous = self.current;
            self.current = (self.current + change).clamp(0.0, 1.0);
            if !in_portal {
                self.contact_latched = false;
            }
            // Contact refreshes an active cooldown before its per-tick decrement.
            if self.cooldown_ticks > 0 {
                if in_portal {
                    self.cooldown_ticks = PORTAL_COOLDOWN_TICKS;
                }
                self.cooldown_ticks -= 1;
            }
        }
        if ticks > catch_up_limit {
            self.previous = self.current;
        }
    }
}

/// Continuous vision-effect strengths for the fog/lighting owners, each `0..=1`.
#[derive(Resource, Debug, Default, Clone, Copy, PartialEq)]
pub struct VisionEffects {
    pub blindness: f32,
    pub darkness: f32,
    pub night_vision: f32,
    pub nausea: f32,
}

impl VisionEffects {
    /// Applies the selected camera's player-state capability without resetting effect clocks.
    pub fn for_camera(self, camera: &super::ServerCameraView) -> Self {
        if camera.player_effects_enabled() {
            self
        } else {
            Self {
                nausea: self.nausea,
                ..Self::default()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fire_follows_the_actor_flag_and_has_no_fixed_texture_path() {
        let inputs = ScreenEffectInputs {
            on_fire: true,
            ..Default::default()
        };
        let layers = compute_overlays(&inputs);
        let fire = layers
            .iter()
            .find(|layer| layer.kind == OverlayKind::Fire)
            .unwrap();
        assert_eq!(fire.alpha, 0.9);
        assert_eq!(fire.rgb, [1.0; 3]);
        assert_eq!(fire.texture, None);
        let lava = ScreenEffectInputs {
            head: HeadMedium::Lava,
            ..Default::default()
        };
        assert!(
            !compute_overlays(&lava)
                .iter()
                .any(|layer| layer.kind == OverlayKind::Fire)
        );
    }

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
    fn distortion_scale_zero_preserves_the_portal_texture() {
        let layers = compute_overlays(&ScreenEffectInputs {
            portal_progress: 1.0,
            distortion_scale: 0.0,
            ..Default::default()
        });
        assert_eq!(layers.len(), 1);
        assert_eq!(layers[0].alpha, 1.0);
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
        assert!((progress.value() - 0.4875).abs() < 1e-6);
        progress.advance(false, 0.25);
        assert!((progress.value() - 0.3625).abs() < 1e-6);
        progress.advance(false, 10.0);
        assert_eq!(progress.value(), 0.0);
        assert_eq!(portal_alpha(0.0), 0.0);
        assert!((portal_alpha(1.0) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn portal_progress_interpolates_ticks_and_confusion_suppresses_texture() {
        let mut progress = PortalProgress::default();
        progress.advance(true, 0.05);
        assert!(progress.value() < 1e-6);
        progress.advance(true, 0.025);
        assert!((progress.value() - 0.00625).abs() < 1e-6);
        let layers = compute_overlays(&ScreenEffectInputs {
            portal_progress: 1.0,
            confusion_active: true,
            ..Default::default()
        });
        assert!(layers.is_empty());
    }

    #[test]
    fn session_changes_and_confusion_removal_clear_both_progress_samples() {
        let mut progress = PortalProgress::default();
        progress.observe_session(Some(1));
        progress.advance_with_confusion(false, Some(-1), 1.0);
        assert!(progress.value() > 0.0);
        progress.advance_with_confusion(false, None, 0.0);
        assert_eq!(progress.value(), 0.0);
        progress.advance(true, 1.0);
        progress.observe_session(Some(2));
        assert_eq!(progress.value(), 0.0);
        assert_eq!(progress.elapsed_ticks(), 0.0);
    }

    fn advance_ticks(progress: &mut PortalProgress, inside: bool, ticks: u32) {
        for _ in 0..ticks {
            progress.advance(inside, 1.0 / world::TICKS_PER_SECOND as f32);
        }
    }

    #[test]
    fn arrival_cooldown_is_tracked_without_suppressing_physical_contact() {
        let mut progress = PortalProgress::default();
        progress.observe_dimension(Some(0));
        progress.observe_dimension(Some(protocol::NETHER_DIMENSION_ID));
        assert_eq!(progress.cooldown_ticks, PORTAL_COOLDOWN_TICKS);
        advance_ticks(&mut progress, true, PORTAL_COOLDOWN_TICKS * 2);
        assert_eq!(progress.cooldown_ticks, PORTAL_COOLDOWN_TICKS - 1);
        assert_eq!(progress.value(), 1.0);
        advance_ticks(&mut progress, false, PORTAL_COOLDOWN_TICKS - 2);
        assert_eq!(progress.cooldown_ticks, 1);
        assert_eq!(progress.value(), 0.0);
        advance_ticks(&mut progress, true, 2);
        assert_eq!(progress.cooldown_ticks, PORTAL_COOLDOWN_TICKS - 1);
        assert!(progress.value() > 0.0);
        assert!(portal_alpha(progress.value()) > 0.0);
        advance_ticks(&mut progress, false, PORTAL_COOLDOWN_TICKS - 1);
        assert_eq!(progress.cooldown_ticks, 0);
        advance_ticks(&mut progress, true, 2);
        assert!((progress.value() - PORTAL_RISE_PER_TICK).abs() < 1e-6);
    }

    #[test]
    fn initial_nether_login_and_other_dimension_changes_have_no_cooldown() {
        let mut progress = PortalProgress::default();
        progress.observe_dimension(Some(protocol::NETHER_DIMENSION_ID));
        assert_eq!(progress.cooldown_ticks, 0);
        advance_ticks(&mut progress, true, 2);
        assert!(progress.value() > 0.0);
        progress.observe_dimension(Some(0));
        assert_eq!(progress.cooldown_ticks, PORTAL_COOLDOWN_TICKS);
        progress.observe_session(Some(1));
        assert_eq!(progress.cooldown_ticks, 0);
        assert!(!progress.contact_latched);
        progress.observe_dimension(Some(0));
        progress.observe_dimension(Some(2));
        assert_eq!(progress.cooldown_ticks, 0);
    }

    #[test]
    fn leaving_the_portal_clears_the_contact_latch_after_one_progress_tick() {
        let mut progress = PortalProgress::default();
        advance_ticks(&mut progress, true, 2);
        let inside = progress.current;
        advance_ticks(&mut progress, false, 1);
        assert!((progress.current - inside - PORTAL_RISE_PER_TICK).abs() < 1e-6);
        assert!(!progress.contact_latched);
        advance_ticks(&mut progress, false, 1);
        assert_eq!(progress.current, 0.0);
    }

    #[test]
    fn cooldown_uses_whole_game_ticks_and_completes_after_long_pauses() {
        let mut progress = PortalProgress::default();
        progress.observe_dimension(Some(0));
        progress.observe_dimension(Some(protocol::NETHER_DIMENSION_ID));
        progress.advance(false, 0.0);
        progress.advance(false, 0.5 / world::TICKS_PER_SECOND as f32);
        assert_eq!(progress.cooldown_ticks, PORTAL_COOLDOWN_TICKS);
        progress.advance(false, 0.5 / world::TICKS_PER_SECOND as f32);
        assert_eq!(progress.cooldown_ticks, PORTAL_COOLDOWN_TICKS - 1);
        progress.advance(
            false,
            PORTAL_COOLDOWN_TICKS as f32 / world::TICKS_PER_SECOND as f32 + 10.0,
        );
        assert_eq!(progress.cooldown_ticks, 0);
        advance_ticks(&mut progress, true, 2);
        assert!(progress.value() > 0.0);
    }

    #[test]
    fn confusion_still_rises_during_portal_cooldown() {
        let mut progress = PortalProgress::default();
        progress.observe_dimension(Some(0));
        progress.observe_dimension(Some(protocol::NETHER_DIMENSION_ID));
        progress.advance_with_confusion(false, Some(-1), 1.0);
        assert!(progress.value() > 0.0);
        assert!(!progress.contact_latched);
        assert_eq!(
            progress.cooldown_ticks,
            PORTAL_COOLDOWN_TICKS - world::TICKS_PER_SECOND
        );
    }

    #[test]
    fn dimension_change_preserves_existing_contact_until_an_outside_tick() {
        let mut progress = PortalProgress::default();
        progress.observe_dimension(Some(0));
        advance_ticks(&mut progress, true, 100);
        assert_eq!(progress.value(), 1.0);
        progress.observe_dimension(Some(protocol::NETHER_DIMENSION_ID));
        advance_ticks(&mut progress, true, PORTAL_COOLDOWN_TICKS);
        assert_eq!(progress.value(), 1.0);
        assert!(progress.contact_latched);
        advance_ticks(&mut progress, false, 1);
        assert!(!progress.contact_latched);
        advance_ticks(&mut progress, false, 25);
        assert_eq!(progress.value(), 0.0);
        advance_ticks(&mut progress, true, 100);
        assert_eq!(progress.value(), 1.0);
        assert!(progress.contact_latched);
    }

    #[test]
    fn contact_before_a_whole_tick_survives_an_immediate_dimension_change() {
        let mut progress = PortalProgress::default();
        progress.observe_dimension(Some(0));
        progress.advance(true, 0.25 / world::TICKS_PER_SECOND as f32);
        assert!(progress.contact_latched);
        assert_eq!(progress.value(), 0.0);
        progress.observe_dimension(Some(protocol::NETHER_DIMENSION_ID));
        progress.advance(true, 1.75 / world::TICKS_PER_SECOND as f32);
        assert!(progress.value() > 0.0);
        assert!(portal_alpha(progress.value()) > 0.0);
        assert_eq!(progress.cooldown_ticks, PORTAL_COOLDOWN_TICKS - 1);
    }

    #[test]
    fn loading_away_from_portal_expires_cooldown_in_both_transfer_directions() {
        let mut progress = PortalProgress::default();
        progress.observe_dimension(Some(0));
        for destination in [protocol::NETHER_DIMENSION_ID, 0] {
            progress.observe_dimension(Some(destination));
            progress.advance(
                false,
                (PORTAL_COOLDOWN_TICKS + 2) as f32 / world::TICKS_PER_SECOND as f32,
            );
            progress.observe_dimension(Some(destination));
            assert_eq!(progress.cooldown_ticks, 0);
            advance_ticks(&mut progress, true, 2);
            assert!(progress.value() > 0.0);
            assert!(portal_alpha(progress.value()) > 0.0);
            advance_ticks(&mut progress, false, 25);
            assert_eq!(progress.value(), 0.0);
        }
    }

    #[test]
    fn repeated_server_allowed_entries_show_feedback_during_local_cooldown() {
        let mut progress = PortalProgress::default();
        progress.observe_dimension(Some(0));
        for destination in [
            protocol::NETHER_DIMENSION_ID,
            0,
            protocol::NETHER_DIMENSION_ID,
            0,
        ] {
            progress.observe_dimension(Some(destination));
            advance_ticks(&mut progress, false, 25);
            assert!(progress.cooldown_ticks > 0);
            assert_eq!(progress.value(), 0.0);
            assert!(!progress.contact_latched);
            // BDS permits these repeat transfers before our local counter
            // expires. A brief fresh contact must still produce feedback.
            progress.advance(true, 0.25 / world::TICKS_PER_SECOND as f32);
            assert!(progress.contact_latched);
            progress.advance(true, 1.75 / world::TICKS_PER_SECOND as f32);
            assert!(progress.value() > 0.0);
            assert!(portal_alpha(progress.value()) > 0.0);
            assert_eq!(progress.cooldown_ticks, PORTAL_COOLDOWN_TICKS - 1);
        }
    }
}
