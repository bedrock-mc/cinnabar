use bevy::{prelude::Component, render::extract_component::ExtractComponent};
use render_api::EnhancedQuality;

/// Per-camera opt-in for Enhanced rendering, with resolved depth for post effects.
#[derive(Component, ExtractComponent, Clone, Copy, Debug, PartialEq)]
#[extract_app(bevy::render::RenderApp)]
#[require(bevy::render::view::Msaa::Off, bevy::render::camera::TemporalJitter)]
pub struct EnhancedRendering {
    pub quality: EnhancedQuality,
    pub shadows: bool,
    pub shadow_resolution: u32,
    /// Sun shadow cascades, clamped to `2..=MAX_SHADOW_CASCADES`.
    pub shadow_cascades: u32,
    /// Blocks from the camera covered by the last cascade.
    pub shadow_distance: f32,
    pub bloom: bool,
    pub light_shafts: bool,
    pub waving: bool,
    pub water_reflections: bool,
    /// Enables the Cook-Torrance material response and linear PBR atlas layers.
    pub physically_based: bool,
    /// Enables depth-based ambient visibility and short sun contact shadows.
    pub ssao: bool,
    /// Enables the procedural 3D cloud layer composited over sky pixels.
    pub volumetric_clouds: bool,
    /// Enables reprojection, clipping, and disocclusion rejection of world history.
    pub temporal_aa: bool,
    /// Internal reflection views never sample their own captures or run post effects.
    pub reflection_capture: bool,
    /// Local diagnostic output; never changes the server or vanilla views.
    pub shadow_debug: EnhancedShadowDebug,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u32)]
pub enum EnhancedShadowDebug {
    #[default]
    Off = 0,
    Cascades = 1,
    Visibility = 2,
    DepthNear = 3,
    DepthMiddle = 4,
    DepthFar = 5,
}

pub const MAX_SHADOW_CASCADES: u32 = 3;

/// Smallest shadow target admitted by frame preparation.
pub const MIN_SHADOW_RESOLUTION: u32 = 256;

/// Maximum shadow reach admitted for hidden development captures.
pub const DIAGNOSTIC_SHADOW_DISTANCE: f32 = 24.0;

impl Default for EnhancedRendering {
    fn default() -> Self {
        Self::for_quality(EnhancedQuality::default())
    }
}

impl EnhancedRendering {
    /// Full Enhanced settings with the shadow budget of `quality`.
    #[must_use]
    pub fn for_quality(quality: EnhancedQuality) -> Self {
        let budget = enhanced_quality_budget(quality);
        Self {
            quality,
            shadows: true,
            shadow_resolution: budget.shadow_resolution,
            shadow_cascades: MAX_SHADOW_CASCADES,
            shadow_distance: budget.shadow_distance,
            bloom: true,
            light_shafts: true,
            waving: true,
            water_reflections: true,
            physically_based: true,
            ssao: true,
            volumetric_clouds: true,
            temporal_aa: true,
            reflection_capture: false,
            shadow_debug: EnhancedShadowDebug::Off,
        }
    }

    /// Bounds the actual Enhanced passes for hidden development captures.
    #[cfg(feature = "enhanced-diagnostics")]
    #[must_use]
    pub fn bounded_diagnostic(self) -> Self {
        Self {
            shadow_resolution: MIN_SHADOW_RESOLUTION,
            shadow_distance: DIAGNOSTIC_SHADOW_DISTANCE,
            waving: false,
            ..self
        }
    }
}

/// Sampling and allocation budgets for one Enhanced quality preset.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EnhancedQualityBudget {
    pub shadow_resolution: u32,
    pub shadow_distance: f32,
    pub cloud_steps: u32,
    pub cloud_environment_refresh: f32,
    pub surface_samples: [f32; 4],
    pub effects_divisor: u32,
    pub cloud_shadow_resolution: u32,
    pub irradiance_updates: u32,
    pub irradiance_rays: u32,
    pub reflection_samples: u32,
    pub reflection_refresh: f64,
    pub reflection_animation: f64,
}

/// Returns the budget every Enhanced pass reads for `quality`.
#[must_use]
pub const fn enhanced_quality_budget(quality: EnhancedQuality) -> EnhancedQualityBudget {
    match quality {
        EnhancedQuality::Performance => EnhancedQualityBudget {
            shadow_resolution: 1024,
            shadow_distance: 72.0,
            cloud_steps: 16,
            cloud_environment_refresh: 1.0,
            surface_samples: [24.0, 8.0, 4.0, 8.0],
            effects_divisor: 4,
            cloud_shadow_resolution: 64,
            irradiance_updates: 8,
            irradiance_rays: 16,
            reflection_samples: 16,
            reflection_refresh: 0.3,
            reflection_animation: 2.0,
        },
        EnhancedQuality::Balanced => EnhancedQualityBudget {
            shadow_resolution: 1536,
            shadow_distance: 96.0,
            cloud_steps: 24,
            cloud_environment_refresh: 0.5,
            surface_samples: [40.0, 12.0, 8.0, 12.0],
            effects_divisor: 2,
            cloud_shadow_resolution: 96,
            irradiance_updates: 16,
            irradiance_rays: 24,
            reflection_samples: 24,
            reflection_refresh: 0.15,
            reflection_animation: 1.5,
        },
        EnhancedQuality::Ultra => EnhancedQualityBudget {
            shadow_resolution: 2048,
            shadow_distance: 128.0,
            cloud_steps: 32,
            cloud_environment_refresh: 0.25,
            surface_samples: [64.0, 20.0, 12.0, 20.0],
            effects_divisor: 2,
            cloud_shadow_resolution: 128,
            irradiance_updates: 24,
            irradiance_rays: 32,
            reflection_samples: 48,
            reflection_refresh: 0.1,
            reflection_animation: 1.0,
        },
    }
}

/// The session and dimension that own a view's spatial lighting and history.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct RenderedWorld {
    pub session_generation: u64,
    pub dimension: i32,
}

impl RenderedWorld {
    /// The world `inputs` describe; `None` before any world publishes view inputs.
    #[must_use]
    pub fn of(inputs: Option<&crate::AtmosphereViewInputs>) -> Option<Self> {
        inputs.map(|inputs| Self {
            session_generation: inputs.session_generation,
            dimension: inputs.dimension,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::prelude::{Camera3d, World};

    #[test]
    fn rendered_world_carries_the_exact_session_and_dimension() {
        assert_eq!(RenderedWorld::of(None), None);
        for (session_generation, dimension) in [(1, -42), (1, 0), (2, 0), (3, 700)] {
            let inputs = crate::AtmosphereViewInputs {
                session_generation,
                dimension,
                ..Default::default()
            };
            assert_eq!(
                RenderedWorld::of(Some(&inputs)),
                Some(RenderedWorld {
                    session_generation,
                    dimension
                })
            );
        }
    }

    /// Enhanced's required components never propagate to an ordinary camera.
    #[test]
    fn temporal_jitter_is_required_only_for_opted_in_cameras() {
        use bevy::render::{camera::TemporalJitter, view::Msaa};
        let mut world = World::new();
        let vanilla = world.spawn(Camera3d::default()).id();
        let enhanced = world
            .spawn((Camera3d::default(), EnhancedRendering::default()))
            .id();
        assert!(world.get::<TemporalJitter>(vanilla).is_none());
        assert!(world.get::<TemporalJitter>(enhanced).is_some());
        assert_eq!(world.get::<Msaa>(enhanced), Some(&Msaa::Off));
    }
}

#[cfg(all(test, feature = "enhanced-diagnostics"))]
mod diagnostic_tests {
    use super::*;

    #[test]
    fn diagnostic_quality_changes_preserve_the_launch_shadow_budget() {
        for quality in [
            EnhancedQuality::Performance,
            EnhancedQuality::Balanced,
            EnhancedQuality::Ultra,
        ] {
            let settings = EnhancedRendering::for_quality(quality).bounded_diagnostic();
            assert_eq!(settings.quality, quality);
            assert_eq!(settings.shadow_resolution, MIN_SHADOW_RESOLUTION);
            assert_eq!(settings.shadow_distance, DIAGNOSTIC_SHADOW_DISTANCE);
            assert!(!settings.waving);
        }
    }
}
