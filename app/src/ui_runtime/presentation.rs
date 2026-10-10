use super::UiRuntime;
use bevy::{
    camera::Camera,
    prelude::{Camera3d, GlobalTransform, Query, Res, ResMut, Time, With},
    time::Real,
    window::{PrimaryWindow, Window},
};
use render::{
    ChunkRenderQueue, ChunkUploadAcknowledgements, VisibilityDiagnostics,
    VisibilityDiagnosticsInput,
};
use ui::{DpiScale, SafeArea};
use {
    crate::runtime::{
        shutdown::record_fatal_error,
        visibility::CaveVisibilityCache,
        world::{ClientWorld, WorldStreamFramePoll},
    },
    client_presentation::camera::CameraSettingsAuthority,
};

#[cfg(test)]
use client_ui::ui_runtime::presentation::{HudFrame, menu_artwork};
use client_ui::ui_runtime::presentation::{
    LoadingStage, UiPresentationError, UiPresentationRuntime,
};
pub mod forms;
pub mod gui_scale_settings;
pub mod publish;
pub(crate) use forms::drive_menu_panorama;
pub(crate) use gui_scale_settings::apply_gui_scale_setting;
pub(crate) use publish::{
    observe_mount_jump_input, platform_safe_area_insets, prepare_ui_runtime, publish_ui_runtime,
};

#[cfg(test)]
pub(crate) mod tests;
