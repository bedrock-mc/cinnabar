//! Fixture setup for app tests that exercise the real session controller.
use super::MenuRuntime;
impl MenuRuntime {
    /// Prevents catalog workers from starting in an isolated session fixture.
    pub fn suppress_catalog_for_test(&mut self) {
        self.catalog_started = true;
    }
    /// Points a fixture at a missing core without changing its user directories.
    pub fn use_missing_core_for_test(&mut self) {
        self.layout.core_executable = self.layout.user_data_root.join("missing-test-core");
    }
    /// Publishes a controlled external join stage for teardown fixtures.
    pub fn set_join_stage_for_test(&mut self, stage: launcher::menu::view::JoinStage) {
        self.feeds.join =
            launcher::menu::view::JoinProgress::new(launcher::menu::view::JoinKind::External);
        self.feeds.join.observe(Some(stage));
    }
    /// Queues cancellation without changing the retained connecting screen.
    pub fn request_disconnect_for_test(&mut self) {
        self.intents.disconnect = true;
    }
    /// Reports the menu's local-world lifecycle flags to session fixtures.
    pub fn local_world_state_for_test(&self) -> (bool, bool) {
        (self.local_world_active, self.local_world_joined)
    }
    /// Rebinds a physical control in an isolated input fixture.
    pub fn remap_control_for_test(
        &mut self,
        row: usize,
        control: semantic_input::PhysicalControl,
    ) -> bool {
        std::sync::Arc::make_mut(&mut self.settings_options).remap(row, control)
    }
    /// Arms a stored command without altering ordinary controls in an input fixture.
    pub fn arm_command_macro_for_test(
        &mut self,
        slot: usize,
        control: semantic_input::PhysicalControl,
        command: &str,
    ) -> bool {
        let settings = std::sync::Arc::make_mut(&mut self.settings_options);
        settings.remap_macro(slot, control) && settings.set_macro_command(slot, command)
    }
}
