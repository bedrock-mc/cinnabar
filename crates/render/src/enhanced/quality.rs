//! Sampling and allocation budgets for the shared Enhanced quality setting.

use super::{EnhancedQuality, EnhancedRendering};

#[derive(Clone, Copy)]
#[cfg_attr(not(feature = "enhanced"), allow(dead_code))]
pub(crate) struct QualityBudget {
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

pub(crate) const fn budget(quality: EnhancedQuality) -> QualityBudget {
    match quality {
        EnhancedQuality::Performance => QualityBudget {
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
        EnhancedQuality::Balanced => QualityBudget {
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
        EnhancedQuality::Ultra => QualityBudget {
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

pub(crate) trait EnhancedQualityBudget {
    #[cfg(feature = "enhanced")]
    fn cloud_steps(self) -> u32;
    fn shadow_settings(self) -> (u32, f32);
    #[cfg(feature = "enhanced")]
    fn surface_samples(self) -> [f32; 4];
}

impl EnhancedQualityBudget for EnhancedQuality {
    #[cfg(feature = "enhanced")]
    fn cloud_steps(self) -> u32 {
        budget(self).cloud_steps
    }

    fn shadow_settings(self) -> (u32, f32) {
        let budget = budget(self);
        (budget.shadow_resolution, budget.shadow_distance)
    }

    #[cfg(feature = "enhanced")]
    fn surface_samples(self) -> [f32; 4] {
        budget(self).surface_samples
    }
}

impl EnhancedRendering {
    #[must_use]
    pub fn for_quality(quality: EnhancedQuality) -> Self {
        let (shadow_resolution, shadow_distance) = quality.shadow_settings();
        Self {
            quality,
            shadow_resolution,
            shadow_distance,
            ..Self::default()
        }
    }
}
