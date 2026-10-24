//! Retained desktop controls and help text for the debug overlay.

use launcher::menu::settings_options::BindingId;
use std::sync::Arc;

use bevy::prelude::{ButtonInput, KeyCode, MouseButton};
use client_shell::physical_input::{keyboard_usage, mouse_button_code};
use launcher::menu::settings_options::{SettingsOptions, key_name};
use semantic_input::PhysicalControl;

pub(super) struct DebugBindings {
    settings: Option<Arc<SettingsOptions>>,
    control: Option<PhysicalControl>,
    hint: String,
}

impl Default for DebugBindings {
    fn default() -> Self {
        let settings = SettingsOptions::default();
        Self {
            settings: None,
            control: settings.named_key_control(BindingId::DebugOverlay),
            hint: binding_hint(&settings),
        }
    }
}

impl DebugBindings {
    /// Resolves changed settings once; retaining the Arc makes in-place edits copy on write.
    pub(super) fn update(&mut self, settings: Option<&Arc<SettingsOptions>>) {
        if match (&self.settings, settings) {
            (Some(previous), Some(current)) => Arc::ptr_eq(previous, current),
            (None, None) => true,
            _ => false,
        } {
            return;
        }
        let defaults = SettingsOptions::default();
        let resolved = settings.map_or(&defaults, Arc::as_ref);
        self.control = resolved.named_key_control(BindingId::DebugOverlay);
        self.hint = binding_hint(resolved);
        self.settings = settings.cloned();
    }

    /// Toggles on a fresh configured key or mouse press, never on a held control.
    pub(super) fn pressed(
        &self,
        keys: &ButtonInput<KeyCode>,
        mouse: Option<&ButtonInput<MouseButton>>,
    ) -> bool {
        match self.control {
            Some(PhysicalControl::KeyboardUsage(usage)) => keys
                .get_just_pressed()
                .any(|key| keyboard_usage(*key) == Some(usage)),
            Some(PhysicalControl::MouseButton(button)) => mouse.is_some_and(|mouse| {
                mouse
                    .get_just_pressed()
                    .any(|pressed| mouse_button_code(*pressed) == Some(button))
            }),
            _ => false,
        }
    }

    /// Returns the help row already formatted for the current bindings.
    /// Whether the debug overlay key/button is currently held down.
    pub(super) fn is_down(
        &self,
        keys: &ButtonInput<KeyCode>,
        mouse: Option<&ButtonInput<MouseButton>>,
    ) -> bool {
        match self.control {
            Some(PhysicalControl::KeyboardUsage(usage)) => keys
                .get_pressed()
                .any(|key| keyboard_usage(*key) == Some(usage)),
            Some(PhysicalControl::MouseButton(button)) => mouse.is_some_and(|mouse| {
                mouse
                    .get_pressed()
                    .any(|pressed| mouse_button_code(*pressed) == Some(button))
            }),
            _ => false,
        }
    }

    pub(super) fn hint(&self) -> &str {
        &self.hint
    }
}

/// Formats both shortcuts from the same persisted layout used to activate them.
fn binding_hint(settings: &SettingsOptions) -> String {
    let name = |key| {
        settings
            .named_key_control(key)
            .map(key_name)
            .unwrap_or_default()
    };
    format!(
        "{}: hide debug | {}: screenshot",
        name(BindingId::DebugOverlay),
        name(BindingId::Screenshot),
    )
}
