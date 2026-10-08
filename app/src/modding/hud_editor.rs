//! Fences native HUD editor drafts to their requesting component and live session.

use super::ModRuntime;
use client_ui::ui_runtime::presentation::UiPresentationRuntime;

#[derive(Clone, Copy)]
pub(super) struct Owner {
    pub host: usize,
    pub session: u64,
}

pub(super) fn cancel(
    runtime: &mut ModRuntime,
    presentation: &mut UiPresentationRuntime,
    deliver: bool,
) {
    presentation.cancel_mod_hud_editor();
    let result = presentation
        .take_mod_hud_editor_result()
        .unwrap_or_default();
    if let Some(owner) = runtime.hud_editor_owner.take()
        && deliver
        && owner.host < runtime.host_count()
        && runtime.host(owner.host).is_active()
    {
        let _ = runtime
            .host_mut(owner.host)
            .deliver_hud_editor_result(result);
    }
}

pub(super) fn guard(
    runtime: &mut ModRuntime,
    presentation: &mut UiPresentationRuntime,
    session: Option<u64>,
    focused: bool,
    absorbed: bool,
) {
    let Some(owner) = runtime.hud_editor_owner else {
        return;
    };
    if owner.host >= runtime.host_count()
        || owner.host != runtime.panel_owner()
        || !runtime.host(owner.host).is_active()
        || !runtime.host(owner.host).panel_open()
        || session != Some(owner.session)
        || !focused
        || absorbed
    {
        cancel(runtime, presentation, true);
        if !focused || absorbed || session != Some(owner.session) {
            let panel_owner = runtime.panel_owner();
            runtime.host_mut(panel_owner).set_panel_open(false);
            presentation.set_mod_panel_open(false);
        }
    }
}

pub(super) fn collect(runtime: &mut ModRuntime, presentation: &mut UiPresentationRuntime) {
    if let Some(result) = presentation.take_mod_hud_editor_result()
        && let Some(owner) = runtime.hud_editor_owner.take()
    {
        let _ = runtime
            .host_mut(owner.host)
            .deliver_hud_editor_result(result);
    }
}

pub(super) fn publish(
    runtime: &mut ModRuntime,
    presentation: &mut UiPresentationRuntime,
    session: Option<u64>,
    focused: bool,
) {
    guard(runtime, presentation, session, focused, false);
    collect(runtime, presentation);
    let panel_owner = runtime.panel_owner();
    for index in 0..runtime.host_count() {
        let Some(preview) = runtime.host_mut(index).take_hud_editor_request() else {
            continue;
        };
        if let Some(session) = session
            && focused
            && index == panel_owner
            && runtime.host(index).panel_open()
            && runtime.hud_editor_owner.is_none()
            && presentation.open_mod_hud_editor(&preview).is_ok()
        {
            runtime.hud_editor_owner = Some(Owner {
                host: index,
                session,
            });
        } else {
            let _ = runtime
                .host_mut(index)
                .deliver_hud_editor_result(Default::default());
        }
    }
}
