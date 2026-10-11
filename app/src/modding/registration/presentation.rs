//! Retained presentation surfaces for independently registered components.

use super::*;

/// Removes only the registered component; environment-loaded components retain their state.
pub(super) fn clear_owned_state(world: &mut World) -> Option<ModHost> {
    let Some(runtime) = world.get_resource::<ModRuntime>() else {
        clear_presentation(world);
        return None;
    };
    let index = runtime.registration_host?;
    if index > 0 {
        let mut runtime = world.resource_mut::<ModRuntime>();
        runtime.registration_host = None;
        runtime.registration_identity = None;
        runtime.registration_request = None;
        if runtime
            .hud_editor_owner
            .is_some_and(|owner| owner.host == index)
        {
            runtime.hud_editor_owner = None;
        }
        runtime.controls = mod_host::empty_controls();
        runtime.panel_fonts.remove(index);
        runtime.render_sources.clear();
        return Some(runtime.hosts.remove_companion(index));
    }
    let previous = world
        .remove_resource::<ModRuntime>()
        .map(|runtime| runtime.hosts.host);
    world.remove_resource::<VisualTimeOverride>();
    world.remove_resource::<ModInteraction>();
    clear_presentation(world);
    previous
}

/// Restores retained surfaces from the remaining hosts after a registration transition.
pub(super) fn refresh_presentation(world: &mut World) {
    if !world.contains_resource::<ModRuntime>() {
        return;
    }
    world.resource_scope(|world, mut runtime: Mut<ModRuntime>| {
        let Some(mut presentation) = world.get_resource_mut::<UiPresentationRuntime>() else {
            return;
        };
        let owner = runtime.hosts.panel_owner();
        let _ = presentation.set_mod_panel_font(runtime.panel_fonts[owner].clone());
        let _ = presentation.set_mod_panel(runtime.hosts.host(owner).panel());
        presentation.set_mod_panel_open(runtime.hosts.host(owner).panel_open());
        let hud = runtime
            .hosts
            .hud_owner()
            .and_then(|index| runtime.hosts.host(index).hud());
        let _ = presentation.set_mod_hud(hud);
        let crosshair = runtime
            .hosts
            .crosshair_owner()
            .and_then(|index| runtime.hosts.host(index).crosshair());
        let _ = presentation.set_mod_crosshair(crosshair);
        let _ = presentation.set_mod_label(runtime.hosts.merged_label());
    });
}

/// Releases extension-owned surfaces and gameplay presentation policies.
pub(super) fn clear_presentation(world: &mut World) {
    if let Some(mut physics) =
        world.get_resource_mut::<client_interaction::movement::LocalPhysicsController>()
    {
        physics.set_jump_pulse_scope(None);
    }
    if let Some(mut policy) = world.get_resource_mut::<crate::item_use::ModItemUsePolicy>() {
        policy.scope = None;
    }
    if let Some(mut scene) = world.get_resource_mut::<render::ModRenderScene>() {
        scene.clear();
    }
    if let Some(mut cues) = world.get_resource_mut::<super::super::ModCueFeed>() {
        cues.0.clear();
    }
    if let Some(mut camera) =
        world.get_resource_mut::<client_presentation::camera::CameraSettingsAuthority>()
    {
        camera.set_rig(None);
        camera.set_preserve_teleport_rotation(false);
        camera.set_view_scale(1.0, 1.0);
    }
    if let Some(mut presentation) = world.get_resource_mut::<UiPresentationRuntime>() {
        presentation.set_mod_panel_open(false);
        let _ = presentation.set_mod_panel(None);
        let _ = presentation.set_mod_label(None);
        let _ = presentation.set_mod_hud(None);
        let _ = presentation.set_mod_crosshair(None);
        if let Err(error) = presentation.set_mod_panel_font(None) {
            eprintln!("Personal-panel font could not be released: {error}");
        }
    }
}
