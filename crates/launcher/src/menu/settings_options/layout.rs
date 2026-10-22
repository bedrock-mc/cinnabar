//! Typed sections and row membership for the shared settings registry.

use super::{
    SettingId,
    definitions::{SETTINGS_OPTIONS, SettingDefinition},
};

macro_rules! panels {
    ($($panel:ident),+ $(,)?) => {
        /// A group of ordered rows within a settings section.
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum SettingPanel { $($panel,)+ }

        impl SettingPanel {
            pub const ALL: &[Self] = &[$(Self::$panel,)+];
        }
    }
}

panels! {
AccessibilityTts,AccessibilityGameplay,AccessibilityUi,VideoBackend,VideoGeneral,VideoPerformance,VideoGraphics,VideoClouds,VideoCustomization,VideoAccessibility,VideoAdvanced,Audio,Keyboard,Controller,Touch,General,GeneralPause,GeneralNetwork,Updates,ModUpdates,GeneralMisc,GeneralAdvanced,CreatorCoordinates,CreatorContentLog,CreatorDebugger,CreatorDiagnostic,CreatorWatchdog,CreatorDevice,CreatorEditor,CreatorFiltering,Chat,Discord,DiscordDetails
}

/// Derives ordered rows once and rejects duplicate or missing row positions.
const fn panel_rows() -> [[Option<SettingId>; SETTINGS_OPTIONS.len()]; SettingPanel::ALL.len()] {
    let mut rows = [[None; SETTINGS_OPTIONS.len()]; SettingPanel::ALL.len()];
    let mut index = 0;
    while index < SETTINGS_OPTIONS.len() {
        let definition = SETTINGS_OPTIONS[index];
        let mut placement = 0;
        while placement < definition.section.len() {
            let (panel, order) = definition.section[placement];
            let cell = &mut rows[panel as usize][order as usize];
            assert!(cell.is_none(), "duplicate settings panel row");
            *cell = Some(definition.id);
            placement += 1;
        }
        index += 1;
    }
    let mut panel = 0;
    while panel < rows.len() {
        let mut gap = false;
        let mut row = 0;
        while row < rows[panel].len() {
            if rows[panel][row].is_none() {
                gap = true;
            } else {
                assert!(!gap, "missing settings panel row");
            }
            row += 1;
        }
        panel += 1;
    }
    rows
}

static PANEL_ROWS: [[Option<SettingId>; SETTINGS_OPTIONS.len()]; SettingPanel::ALL.len()] =
    panel_rows();

impl SettingPanel {
    /// Returns the settings category that owns this panel, including custom client tabs.
    pub const fn section(self) -> super::super::settings_sections::SettingSection {
        use super::super::settings_sections::SettingSection;
        match self {
            Self::AccessibilityTts | Self::AccessibilityGameplay | Self::AccessibilityUi => {
                SettingSection::Accessibility
            }
            Self::VideoBackend
            | Self::VideoGeneral
            | Self::VideoPerformance
            | Self::VideoGraphics
            | Self::VideoClouds
            | Self::VideoCustomization
            | Self::VideoAccessibility
            | Self::VideoAdvanced => SettingSection::Video,
            Self::Audio => SettingSection::Sound,
            Self::Keyboard => SettingSection::KeyboardAndMouse,
            Self::Controller => SettingSection::ControllerAndSwitch,
            Self::Touch => SettingSection::Touch,
            Self::General
            | Self::GeneralPause
            | Self::GeneralNetwork
            | Self::GeneralMisc
            | Self::GeneralAdvanced => SettingSection::General,
            Self::Updates | Self::ModUpdates => SettingSection::Updates,
            Self::CreatorCoordinates
            | Self::CreatorContentLog
            | Self::CreatorDebugger
            | Self::CreatorDiagnostic
            | Self::CreatorWatchdog
            | Self::CreatorDevice
            | Self::CreatorEditor
            | Self::CreatorFiltering => SettingSection::Creator,
            Self::Chat => SettingSection::Chat,
            Self::Discord | Self::DiscordDetails => SettingSection::Discord,
        }
    }

    /// Returns registered rows in display order without allocating during painting.
    pub fn options(self) -> impl Iterator<Item = &'static SettingDefinition> {
        PANEL_ROWS[self as usize]
            .iter()
            .take_while(|id| id.is_some())
            .flatten()
            .map(|id| &SETTINGS_OPTIONS[id.index()])
    }
}
