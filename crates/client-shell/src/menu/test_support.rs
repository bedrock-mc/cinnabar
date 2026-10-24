//! Fixture setup for app tests that exercise the real session controller.
use super::MenuRuntime;

#[cfg(feature = "native-networking")]
pub mod friend;

/// Observes admission of the real account-store worker, without starting or completing it.
pub fn account_storage_started(menu: &MenuRuntime) -> bool {
    menu.accounts.work.is_some()
}

/// Publishes a controlled external join stage for teardown fixtures.
pub fn set_join_stage(menu: &mut MenuRuntime, stage: launcher::menu::view::JoinStage) {
    menu.feeds.join =
        launcher::menu::view::JoinProgress::new(launcher::menu::view::JoinKind::External);
    menu.feeds.join.observe(Some(stage));
}

/// Queues cancellation without changing the retained connecting screen.
pub fn request_disconnect(menu: &mut MenuRuntime) {
    menu.intents.disconnect = true;
}

/// Reports the menu's local-world lifecycle flags to session fixtures.
pub fn local_world_state(menu: &MenuRuntime) -> (bool, bool) {
    (menu.local_world_active, menu.local_world_joined)
}

/// Rebinds a physical control in an isolated input fixture.
pub fn remap_control(
    menu: &mut MenuRuntime,
    row: usize,
    control: semantic_input::PhysicalControl,
) -> bool {
    std::sync::Arc::make_mut(&mut menu.settings_options).remap(row, control)
}

/// Arms a stored command without altering ordinary controls in an input fixture.
pub fn arm_command_macro(
    menu: &mut MenuRuntime,
    slot: usize,
    control: semantic_input::PhysicalControl,
    command: &str,
) -> bool {
    let settings = std::sync::Arc::make_mut(&mut menu.settings_options);
    settings.remap_macro(slot, control) && settings.set_macro_command(slot, command)
}
