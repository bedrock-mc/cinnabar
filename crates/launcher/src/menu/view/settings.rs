use super::{MenuAction, SettingsSliderPointer};

/// Transient settings controls shared by the live menu and its presented views.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SettingsInteraction {
    pub section: u8,
    pub dropdown: Option<u16>,
    pub scale_picker: bool,
    /// Last interactive activation and its monotonic input revision.
    pub control_activation: Option<(MenuAction, u64)>,
    pub control_activation_navigation: bool,
    /// Continuous pointer position, independent of the persisted slider step.
    pub slider_pointer: Option<SettingsSliderPointer>,
    pub slider_hovered: Option<u16>,
    pub slider_selected: Option<u16>,
    pub key_remap: Option<u16>,
    pub advanced_graphics: bool,
}

impl SettingsInteraction {
    /// Records a fresh input activation even when the control and value repeat.
    pub fn record_activation(&mut self, action: MenuAction, navigation: bool) {
        let revision = self
            .control_activation
            .map_or(1, |(_, revision)| revision.wrapping_add(1));
        self.control_activation = Some((action, revision));
        self.control_activation_navigation = navigation;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeated_activations_refresh_the_revision_and_input_source() {
        let mut settings = SettingsInteraction::default();
        let action = MenuAction::SettingsAdvancedGraphics;
        settings.record_activation(action, false);
        let first = settings;
        settings.record_activation(action, true);

        assert_eq!(first.control_activation, Some((action, 1)));
        assert!(!first.control_activation_navigation);
        assert_eq!(settings.control_activation, Some((action, 2)));
        assert!(settings.control_activation_navigation);
        assert_ne!(first, settings);
    }

    #[test]
    fn activation_revision_wraps_without_losing_the_action() {
        let action = MenuAction::SettingsAdvancedGraphics;
        let mut settings = SettingsInteraction {
            control_activation: Some((action, u64::MAX)),
            ..Default::default()
        };
        settings.record_activation(action, false);
        assert_eq!(settings.control_activation, Some((action, 0)));
    }
}
