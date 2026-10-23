//! Shows the render host's shader-pack load state: a pressless standing toast while a slow
//! load runs, an ordinary toast with the outcome, and the failure in the Shaders tab
//! while the failed pack is selected.

use std::{ops::DerefMut, sync::Arc, time::Duration};

use launcher::menu::shader_pack_status::{PackLoad, PackNotices, PackToast, failure_description};
use ui::{RenderMode, StandingToast};

use super::MenuRuntime;
use crate::ui_resources::UiRuntimeResource;

/// The standing toast's cause; Discord join toasts, the other standing toasts, use user ids.
const LOADING_TOAST: u64 = 0;

#[derive(Debug, Default)]
pub(super) struct ShaderPackStatusUi {
    notices: PackNotices,
    failure: Option<Failure>,
    loading: Option<String>, // the pack the render host is loading
}

/// The pack that failed, by name, with the Renderer row's description of why.
#[derive(Debug)]
struct Failure {
    pack: String,
    reason: String,
    description: Arc<str>,
}

impl MenuRuntime {
    /// Mirrors the render host's `load` state at `now`: raises the toast each change calls
    /// for and keeps the failure for the Renderer row. Touches `runtime` only on a change.
    pub fn sync_shader_pack_status(
        &mut self,
        load: PackLoad<'_>,
        runtime: &mut impl DerefMut<Target = UiRuntimeResource>,
        now: Duration,
    ) {
        let status = &mut self.shader_pack_status;
        match load {
            PackLoad::Failed { pack, reason } => {
                if !status
                    .failure
                    .as_ref()
                    .is_some_and(|failure| failure.pack == pack && failure.reason == reason)
                {
                    status.failure = Some(Failure {
                        pack: pack.to_owned(),
                        reason: reason.to_owned(),
                        description: Arc::from(failure_description(reason)),
                    });
                }
            }
            _ => status.failure = None,
        }
        match load {
            PackLoad::Loading { pack, .. } => {
                if status.loading.as_deref() != Some(pack) {
                    status.loading = Some(pack.to_owned());
                }
            }
            _ => status.loading = None,
        }
        let millis = u64::try_from(now.as_millis()).unwrap_or(u64::MAX);
        let standing = runtime.hud().standing_toast();
        let ours = standing.is_some_and(|toast| toast.id == LOADING_TOAST && toast.press.is_none());
        if !ours {
            status.notices.loading_toast_missing();
        }
        let can_stand = !standing.is_some_and(|toast| {
            toast.press.is_some()
                && millis
                    < toast
                        .until_millis
                        .saturating_add(ui::TOAST_SLIDE_OUT_MILLIS)
        });
        let Some(toast) = status.notices.observe(load, now, can_stand) else {
            return;
        };
        match toast {
            PackToast::Loading { title, message } => {
                runtime.stand_toast(StandingToast {
                    id: LOADING_TOAST,
                    title: title.into(),
                    message: message.into(),
                    press: None,
                    since_millis: millis,
                    until_millis: u64::MAX,
                });
            }
            PackToast::Outcome { title, message } => {
                if ours {
                    runtime.retire_standing_toast(millis);
                }
                runtime.push_client_toast(title.into(), message.into(), millis);
            }
            PackToast::Withdraw => {
                if ours {
                    runtime.retire_standing_toast(millis);
                }
            }
        }
    }

    /// Why the selected shader pack failed to load, while it is the chosen renderer.
    pub(super) fn renderer_failure(&self) -> Option<Arc<str>> {
        let failure = self.shader_pack_status.failure.as_ref()?;
        (self.render_mode == RenderMode::ShaderPack
            && self.shader_pack.as_deref() == Some(failure.pack.as_str()))
        .then(|| Arc::clone(&failure.description))
    }

    /// Whether the render host is still loading the selected shader pack.
    pub(super) fn renderer_loading(&self) -> bool {
        self.render_mode == RenderMode::ShaderPack
            && self.shader_pack_status.loading.is_some()
            && self.shader_pack_status.loading == self.shader_pack
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use client_ui::ui_runtime::UiRuntime;
    use launcher::menu::shader_pack_status::LOADING_TOAST_DELAY;
    use ui::ToastPress;

    const REASON: &str = "composite: composite.fsh:2: parse: Unknown variable: undefined_name";

    fn shader_pack_menu(selected: &str) -> MenuRuntime {
        let mut menu = MenuRuntime::new(true, 2, "Steve".to_owned());
        menu.sync_shader_packs(vec!["Dusk.zip".to_owned(), "Lumen".to_owned()]);
        menu.render_mode = RenderMode::ShaderPack;
        menu.shader_pack = Some(selected.to_owned());
        menu
    }

    fn millis(at: Duration) -> u64 {
        u64::try_from(at.as_millis()).unwrap()
    }

    fn shown(runtime: &UiRuntime, at: Duration) -> Option<(String, String)> {
        runtime
            .hud()
            .showing_toast(millis(at))
            .map(|toast| (toast.title.to_owned(), toast.message.to_owned()))
    }

    #[test]
    fn a_slow_load_stands_then_gives_way_to_its_outcome() {
        let mut menu = shader_pack_menu("Dusk.zip");
        let mut runtime = UiRuntimeResource(UiRuntime::new(1));
        let loading = |programs| PackLoad::Loading {
            pack: "Dusk.zip",
            programs,
        };
        menu.sync_shader_pack_status(loading(None), &mut &mut runtime, Duration::ZERO);
        assert!(runtime.hud().standing_toast().is_none());
        let stood = LOADING_TOAST_DELAY;
        menu.sync_shader_pack_status(loading(Some((4, 10))), &mut &mut runtime, stood);
        let held = stood + Duration::from_secs(20);
        assert_eq!(
            shown(&runtime, held),
            Some((
                "Loading shader pack Dusk…".to_owned(),
                "Translated 4 of 10 programs".to_owned()
            )),
            "it stands for as long as the load runs"
        );
        let ready = held + Duration::from_secs(1);
        menu.sync_shader_pack_status(
            PackLoad::Ready { pack: "Dusk.zip" },
            &mut &mut runtime,
            ready,
        );
        let after = ready + Duration::from_millis(ui::TOAST_SLIDE_OUT_MILLIS);
        assert_eq!(
            shown(&runtime, after),
            Some(("Shader pack Dusk loaded".to_owned(), String::new()))
        );
        let retired = runtime
            .hud()
            .standing_toast()
            .map(|toast| toast.until_millis);
        assert_eq!(retired, Some(millis(ready)), "the loading toast slid out");
    }

    #[test]
    fn the_join_toast_sync_leaves_the_loading_toast_standing() {
        let mut menu = shader_pack_menu("Dusk.zip");
        menu.show_world();
        let mut runtime = UiRuntimeResource(UiRuntime::new(1));
        let loading = PackLoad::Loading {
            pack: "Dusk.zip",
            programs: None,
        };
        menu.sync_shader_pack_status(loading, &mut &mut runtime, Duration::ZERO);
        menu.sync_shader_pack_status(loading, &mut &mut runtime, LOADING_TOAST_DELAY);
        menu.sync_join_toast(&mut &mut runtime, LOADING_TOAST_DELAY * 2);
        let standing = runtime.hud().standing_toast().unwrap();
        assert_eq!(standing.until_millis, u64::MAX, "no join request to retire");
    }

    #[test]
    fn a_failure_toasts_once_and_explains_itself_while_selected() {
        let mut menu = shader_pack_menu("Dusk.zip");
        let mut runtime = UiRuntimeResource(UiRuntime::new(1));
        let failed = PackLoad::Failed {
            pack: "Dusk.zip",
            reason: REASON,
        };
        for frame in 0..3 {
            menu.sync_shader_pack_status(failed, &mut &mut runtime, Duration::from_millis(frame));
        }
        assert_eq!(
            runtime.hud().toasts().len(),
            1,
            "one toast for the transition"
        );
        assert_eq!(
            shown(&runtime, Duration::ZERO).map(|toast| toast.1),
            Some(REASON.to_owned())
        );
        let description = menu.view().renderer_failure.expect("selected pack failed");
        assert!(description.contains(REASON), "{description}");
        menu.shader_pack = Some("Lumen".to_owned());
        assert_eq!(
            menu.view().renderer_failure,
            None,
            "another pack is selected"
        );
        menu.shader_pack = Some("Dusk.zip".to_owned());
        menu.render_mode = RenderMode::Vanilla;
        assert_eq!(menu.view().renderer_failure, None, "Vanilla is selected");
        menu.render_mode = RenderMode::ShaderPack;
        let retry = PackLoad::Loading {
            pack: "Dusk.zip",
            programs: None,
        };
        menu.sync_shader_pack_status(retry, &mut &mut runtime, Duration::from_secs(9));
        assert_eq!(menu.view().renderer_failure, None, "loading it again");
        assert!(menu.view().shader_pack_loading, "the retry is loading");
        menu.shader_pack = Some("Lumen".to_owned());
        assert!(!menu.view().shader_pack_loading, "another pack is selected");
    }

    #[test]
    fn unchanged_loading_returns_after_the_ui_session_resets() {
        let mut menu = shader_pack_menu("Dusk.zip");
        let mut runtime = UiRuntimeResource(UiRuntime::new(1));
        let loading = PackLoad::Loading {
            pack: "Dusk.zip",
            programs: Some((4, 4)),
        };
        menu.sync_shader_pack_status(loading, &mut &mut runtime, Duration::ZERO);
        menu.sync_shader_pack_status(loading, &mut &mut runtime, LOADING_TOAST_DELAY);
        assert!(runtime.hud().standing_toast().is_some());
        runtime.begin_session(2);
        assert!(runtime.hud().standing_toast().is_none());
        let now = LOADING_TOAST_DELAY + Duration::from_secs(1);
        menu.sync_shader_pack_status(loading, &mut &mut runtime, now);
        let standing = runtime.hud().standing_toast().expect("loading feedback");
        assert_eq!(standing.id, LOADING_TOAST);
        assert_eq!(standing.message.as_ref(), "Building pipelines");
        assert_eq!(standing.until_millis, u64::MAX);
        menu.sync_shader_pack_status(loading, &mut &mut runtime, now + Duration::from_secs(1));
        assert_eq!(
            runtime.hud().standing_toast().unwrap().since_millis,
            millis(now)
        );
    }

    #[test]
    fn unchanged_loading_returns_after_a_join_toast_expires() {
        let mut menu = shader_pack_menu("Dusk.zip");
        let mut runtime = UiRuntimeResource(UiRuntime::new(1));
        let loading = PackLoad::Loading {
            pack: "Dusk.zip",
            programs: Some((4, 4)),
        };
        menu.sync_shader_pack_status(loading, &mut &mut runtime, Duration::ZERO);
        menu.sync_shader_pack_status(loading, &mut &mut runtime, LOADING_TOAST_DELAY);
        runtime.stand_toast(StandingToast {
            id: 7,
            title: Arc::from("Join request"),
            message: Arc::from(""),
            press: Some(ToastPress::JoinRequests),
            since_millis: 0,
            until_millis: 30_000,
        });
        menu.sync_shader_pack_status(loading, &mut &mut runtime, Duration::from_secs(1));
        assert_eq!(runtime.hud().standing_toast().unwrap().id, 7);
        menu.sync_shader_pack_status(loading, &mut &mut runtime, Duration::from_secs(31));
        let standing = runtime.hud().standing_toast().unwrap();
        assert_eq!(standing.id, LOADING_TOAST);
        assert_eq!(standing.message.as_ref(), "Building pipelines");
    }

    #[test]
    fn a_blocked_loading_notice_retries_when_the_slot_is_retired() {
        let mut menu = shader_pack_menu("Dusk.zip");
        let mut runtime = UiRuntimeResource(UiRuntime::new(1));
        runtime.stand_toast(StandingToast {
            id: 7,
            title: Arc::from("Join request"),
            message: Arc::from(""),
            press: Some(ToastPress::JoinRequests),
            since_millis: 0,
            until_millis: 30_000,
        });
        let loading = PackLoad::Loading {
            pack: "Dusk.zip",
            programs: None,
        };
        menu.sync_shader_pack_status(loading, &mut &mut runtime, Duration::ZERO);
        menu.sync_shader_pack_status(loading, &mut &mut runtime, LOADING_TOAST_DELAY);
        runtime.retire_standing_toast(1000);
        menu.sync_shader_pack_status(loading, &mut &mut runtime, Duration::from_secs(2));
        assert_eq!(runtime.hud().standing_toast().unwrap().id, LOADING_TOAST);
    }

    #[test]
    fn a_join_request_toast_keeps_its_slot() {
        let mut menu = shader_pack_menu("Dusk.zip");
        let mut runtime = UiRuntimeResource(UiRuntime::new(1));
        runtime.stand_toast(StandingToast {
            id: 7,
            title: Arc::from("Alex wants to join"),
            message: Arc::from(""),
            press: Some(ToastPress::JoinRequests),
            since_millis: 0,
            until_millis: 30_000,
        });
        let loading = PackLoad::Loading {
            pack: "Dusk.zip",
            programs: None,
        };
        menu.sync_shader_pack_status(loading, &mut &mut runtime, Duration::ZERO);
        menu.sync_shader_pack_status(loading, &mut &mut runtime, LOADING_TOAST_DELAY);
        menu.sync_shader_pack_status(PackLoad::Idle, &mut &mut runtime, LOADING_TOAST_DELAY * 2);
        let standing = runtime.hud().standing_toast().unwrap();
        assert_eq!(standing.id, 7);
        assert_eq!(standing.until_millis, 30_000, "not retired by the pack");
    }
}
