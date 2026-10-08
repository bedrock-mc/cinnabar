//! Named aim-assist policies and server-resolved actor priorities.

use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CameraAimAssistTargetMode {
    Angle,
    Distance,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CameraAimAssistAction {
    Set,
    Clear,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CameraAimAssistSettings {
    pub preset_id: Arc<str>,
    pub view_angle: [f32; 2],
    pub distance: f32,
    pub target_mode: CameraAimAssistTargetMode,
    pub action: CameraAimAssistAction,
    pub show_debug_render: bool,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct CameraAimAssistPresetSettings {
    pub preset_id: Option<Arc<str>>,
    pub target_mode: Option<CameraAimAssistTargetMode>,
    pub view_angle: Option<[f32; 2]>,
    pub distance: Option<f32>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct CameraAimAssistRegistry {
    pub categories: Arc<[CameraAimAssistCategory]>,
    pub presets: Arc<[CameraAimAssistPreset]>,
    pub replace: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CameraAimAssistCategory {
    pub name: Arc<str>,
    pub priorities: CameraAimAssistPriorities,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct CameraAimAssistPriorities {
    pub entities: Arc<[CameraAimAssistPriority]>,
    pub blocks: Arc<[CameraAimAssistPriority]>,
    pub block_tags: Arc<[CameraAimAssistPriority]>,
    pub entity_type_families: Arc<[CameraAimAssistPriority]>,
    pub entity_default: Option<i32>,
    pub block_default: Option<i32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CameraAimAssistPriority {
    pub identifier: Arc<str>,
    pub priority: i32,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct CameraAimAssistPreset {
    pub identifier: Arc<str>,
    pub exclusions: CameraAimAssistExclusions,
    pub liquid_targeting_list: Arc<[Arc<str>]>,
    pub item_settings: Arc<[CameraAimAssistItemSetting]>,
    pub default_item_settings: Option<Arc<str>>,
    pub hand_settings: Option<Arc<str>>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct CameraAimAssistExclusions {
    pub blocks: Arc<[Arc<str>]>,
    pub entities: Arc<[Arc<str>]>,
    pub block_tags: Arc<[Arc<str>]>,
    pub entity_type_families: Arc<[Arc<str>]>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CameraAimAssistItemSetting {
    pub item: Arc<str>,
    pub category: Arc<str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CameraAimAssistActorPriority {
    pub preset_index: i32,
    pub category_index: i32,
    pub actor_index: i32,
    pub priority: i32,
}
