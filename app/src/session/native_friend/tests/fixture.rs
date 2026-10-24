//! Same application resources and selected account, without a Core process or synthetic bootstrap.

use super::*;
use bevy::prelude::{App, AppExit, Update};
use client_shell::{menu::test_support::friend as menu_friend, ui_resources::UiRuntimeResource};
use client_ui::ui_runtime::UiRuntime;
use launcher::install_layout::Platform;
use launcher_host::native_account::test_support::friend::Fixture;

pub(super) struct Original {
    pub(super) app: App,
    pub(super) service: Fixture,
    pub(super) tasks: tokio_util::task::TaskTracker,
    pub(super) _root: tempfile::TempDir,
}

impl Original {
    /// Acquires the actual sealed account, then drives the ordinary catalogue worker into its menu.
    pub(super) async fn open(legacy: bool, deadline: Instant) -> Self {
        let root = tempfile::tempdir().unwrap();
        let layout = launcher::test_support::scratch_at(Platform::Linux, root.path());
        let service = if legacy {
            Fixture::legacy(layout.auth_cache(), layout.device_profile_file(), deadline).await
        } else {
            Fixture::messaging(layout.auth_cache(), layout.device_profile_file(), deadline).await
        };
        let tasks = tokio_util::task::TaskTracker::new();
        let (execution, _stop) = bridge::tasks::NativeExecution::for_process(
            tokio::runtime::Handle::current(),
            tasks.clone(),
        );
        let mut menu = MenuRuntime::new_with_layout(
            true,
            Some(2),
            "Controlled visitor".into(),
            layout,
            client_shell::player_skin::LocalPlayerSkin::generated_default("Controlled visitor"),
        )
        .with_io(execution);
        menu_friend::attach(&mut menu, &service).await.unwrap();
        test_time::eventually("original current friend catalogue applied", || {
            menu_friend::poll(&mut menu);
            !menu.view().friends.is_empty()
        });
        let controller = SessionController::default()
            .with_native_transport(Some(protocol::NativeTransport::Addressed))
            .with_client_random_id(Some(protocol::ClientRandomId::new(1).unwrap()));
        let generation = controller.generation;
        let mut app = App::new();
        app.add_message::<AppExit>()
            .insert_resource(menu)
            .insert_resource(controller)
            .insert_resource(NetworkHandle::<PackApplication>::disconnected())
            .insert_resource(BlobCache::default())
            .insert_resource(client_runtime::pack_admission::ResourcePackAdmissionState::default())
            .insert_resource(
                crate::runtime::network::session_registries::tests::collision_resource(),
            )
            .insert_resource(UiRuntimeResource(UiRuntime::new(generation)))
            .insert_resource(client_state::PlayerRuntime(player_state::PlayerState::new(
                generation,
            )))
            .init_resource::<crate::runtime::world::ClientWorld>()
            .init_resource::<crate::runtime::shutdown::FatalError>()
            .insert_resource(client_interaction::movement::MovementTicker::default())
            .insert_resource(client_interaction::movement::LocalPhysicsController::default())
            .insert_resource(client_presentation::local_player::LocalPlayerFrameCarrier::default())
            .insert_resource(
                client_presentation::local_player::InteractionOriginSnapshot::default(),
            )
            .add_systems(Update, super::super::super::drive_session);
        Self {
            app,
            service,
            tasks,
            _root: root,
        }
    }

    /// Uses only the ordinary row action and its current opaque selection checks.
    pub(super) fn play(&mut self) -> bool {
        self.app
            .world_mut()
            .resource_mut::<MenuRuntime>()
            .activate(launcher::menu::MenuAction::PlayFriend(0));
        self.app.update();
        self.app
            .world()
            .resource::<SessionController>()
            .native_friend_session
            && self.app.world().resource::<MenuRuntime>().is_connecting()
    }

    /// Uses the same ordinary disconnect action and generation-fenced retirement system.
    pub(super) fn disconnect(&mut self) {
        self.app
            .world_mut()
            .resource_mut::<MenuRuntime>()
            .activate(launcher::menu::MenuAction::PauseDisconnect);
        self.app.update();
    }

    /// Uses the connecting screen's actual cancel action, including its original cancellable-stage check.
    pub(super) fn cancel(&mut self) {
        self.app
            .world_mut()
            .resource_mut::<MenuRuntime>()
            .activate(launcher::menu::MenuAction::AddBack);
        self.app.update();
    }

    /// Observes actual full app preparation output; no GameData or Ready value is installed by the fixture.
    pub(super) async fn bootstrap(&mut self, deadline: Instant) -> bool {
        let result = tokio::time::timeout_at(deadline.into(), async {
            loop {
                self.app.update();
                let event = self
                    .app
                    .world_mut()
                    .resource_mut::<NetworkHandle<PackApplication>>()
                    .control_events_mut()
                    .try_recv();
                match event {
                    Ok(client_session::NetworkControlEvent::Bootstrap { world, packs, .. }) => {
                        let real = world.local_player_runtime_id
                            == protocol::test_support::friend_peer::RUNTIME_ID
                            && matches!(&packs.admission, resource_pack::PackAdmission::None);
                        let network = self
                            .app
                            .world()
                            .resource::<NetworkHandle<PackApplication>>();
                        let initialized = network.finish_loading();
                        network.flush_frame();
                        return real && initialized;
                    }
                    Ok(client_session::NetworkControlEvent::Failed { .. }) => return false,
                    Err(tokio::sync::mpsc::error::TryRecvError::Disconnected)
                        if self
                            .app
                            .world()
                            .resource::<SessionController>()
                            .join
                            .is_none() =>
                    {
                        return false;
                    }
                    Ok(_) | Err(_) => {}
                }
                tokio::task::yield_now().await;
            }
        })
        .await;
        result.unwrap_or(false)
    }

    /// Requires the actual worker token to retire before checking its original cleanup reservation.
    pub(super) async fn retired(&self, deadline: Instant) -> bool {
        self.tasks.close();
        let joined = tokio::time::timeout_at(deadline.into(), self.tasks.wait())
            .await
            .is_ok();
        let controller = self.app.world().resource::<SessionController>();
        joined && controller.friend_cleanup.observation() == (false, false)
    }

    /// Joins the original leave observation only after successful worker retirement; otherwise aborts
    /// and joins the actual service peers so a failed cleanup cannot wait forever for an unsent leave.
    pub(super) async fn finish(
        self,
        retired: bool,
    ) -> (
        Option<launcher_host::native_account::test_support::friend::Observation>,
        bool,
    ) {
        let Self {
            app,
            service,
            tasks,
            _root,
        } = self;
        drop(app);
        let observed = if retired {
            Some(service.finish().await)
        } else {
            service.abort().await;
            None
        };
        tasks.close();
        // This bounds only failure-path destruction after the service abort; it cannot extend
        // the original membership/game deadlines or turn unsuccessful retirement into success.
        let joined = tokio::time::timeout(test_time::DEFAULT_TIMEOUT, tasks.wait())
            .await
            .is_ok();
        drop(_root);
        (observed, joined)
    }
}
