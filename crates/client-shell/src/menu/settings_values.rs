//! Window and viewport settings retained by the menu.

use super::{EnhancedQuality, MenuRuntime, RenderMode};
use launcher::menu::MenuAction;

impl MenuRuntime {
    /// Mirrors the applied mode; a pending menu toggle wins until taken.
    pub fn sync_render_mode(&mut self, applied: RenderMode) {
        if self.render_mode_request.is_none() {
            self.render_mode = applied;
        }
    }

    /// Mirrors applied quality while retaining a pending menu choice.
    pub fn sync_enhanced_quality(&mut self, applied: EnhancedQuality) {
        if self.enhanced_quality_request.is_none() {
            self.enhanced_quality = applied;
        }
    }

    /// Consume the pending Video-section change.
    pub fn take_render_mode_request(&mut self) -> Option<RenderMode> {
        self.render_mode_request.take()
    }

    /// Consume the pending Enhanced quality change.
    pub fn take_enhanced_quality_request(&mut self) -> Option<EnhancedQuality> {
        self.enhanced_quality_request.take()
    }

    /// Routes extension edits through the saved graphics request.
    pub(super) fn activate_enhanced_settings(&mut self, action: MenuAction) {
        if !render_model::ENHANCED_RENDERING_ENABLED {
            return;
        }
        match action {
            MenuAction::ToggleRenderMode => {
                self.render_mode = self.render_mode.toggled();
                self.render_mode_request = Some(self.render_mode);
            }
            action if self.render_mode == RenderMode::Enhanced => {
                let quality = match action {
                    MenuAction::CycleEnhancedQuality => self.enhanced_quality.next(),
                    MenuAction::SetEnhancedQuality(quality) => quality,
                    _ => return,
                };
                if self.enhanced_quality != quality {
                    self.enhanced_quality = quality;
                    self.enhanced_quality_request = Some(quality);
                }
            }
            _ => {}
        }
    }

    /// A capture's fixed CLI scale, cleared when the native option is changed.
    pub fn gui_scale_preference(&self) -> Option<u8> {
        self.gui_scale_preference
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_gui_scale_preference(&mut self, preference: Option<u8>) {
        self.gui_scale_preference = preference
            .filter(|scale| *scale > 0)
            .map(|scale| scale.clamp(1, 4));
    }

    pub fn gui_scale_offset(&self) -> i8 {
        self.gui_scale_offset
    }

    pub(super) fn set_gui_scale_offset(&mut self, offset: i8) {
        if self
            .gui_scale_choices
            .iter()
            .any(|choice| choice.offset == offset)
        {
            self.gui_scale_preference = None;
            self.gui_scale_offset = offset;
            self.gui_scale_display_offset = offset;
        }
    }

    /// The native choices track the physical viewport; the saved modifier
    /// survives resize and is clamped when the rendering scale is evaluated.
    pub fn sync_gui_scale(
        &mut self,
        displayed_offset: i8,
        choices: Vec<ui::DesktopGuiScaleChoice>,
    ) {
        self.gui_scale_display_offset = displayed_offset.clamp(
            choices.first().map_or(0, |choice| choice.offset),
            choices.last().map_or(0, |choice| choice.offset),
        );
        if self.gui_scale_choices != choices {
            self.gui_scale_choices = choices;
        }
    }

    /// A settings press waiting to be applied to the primary window.
    pub fn take_fullscreen_change(&mut self) -> Option<bool> {
        self.fullscreen_change.take()
    }

    /// Mirror the window without queuing a new settings press.
    pub fn sync_fullscreen(&mut self, fullscreen: bool) {
        self.fullscreen = fullscreen;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn menu() -> MenuRuntime {
        MenuRuntime::new(true, 2, "Steve".to_owned())
    }

    #[test]
    fn native_gui_scale_choice_clears_the_fixed_cli_override() {
        let mut menu = menu();
        menu.set_gui_scale_preference(Some(2));
        menu.sync_gui_scale(
            0,
            ui::DesktopGuiScale::for_window([1280, 720])
                .choices()
                .collect(),
        );
        menu.activate(launcher::menu::MenuAction::SettingsScale(-1));
        assert_eq!(menu.gui_scale_preference(), None);
        assert_eq!(menu.gui_scale_offset(), -1);
    }

    #[test]
    fn fullscreen_mirroring_does_not_queue_a_setting_change() {
        let mut menu = menu();
        menu.sync_fullscreen(true);
        assert!(menu.view().fullscreen);
        assert_eq!(menu.take_fullscreen_change(), None);
        menu.activate(launcher::menu::MenuAction::SettingsFullscreen(false));
        assert!(!menu.view().fullscreen);
        assert_eq!(menu.take_fullscreen_change(), Some(false));
        assert_eq!(menu.take_fullscreen_change(), None);
    }
}
