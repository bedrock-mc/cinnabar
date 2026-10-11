use bevy::prelude::Component;
/// Presentation-only exposure; removing this component releases the per-view blur resources.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct CameraMotionBlur {
    pub exposure_seconds: f32,
    pub samples: u32,
    pub reset_epoch: u64,
    pub delta_seconds: f32,
}
