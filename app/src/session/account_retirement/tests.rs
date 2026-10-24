//! Account storage must wait for cleanup on the original session worker.

use super::*;
use bevy::prelude::{App, Update};
use client_session::SessionCompanion;
use client_shell::{menu::test_support, ui_resources::UiRuntimeResource};
use client_ui::ui_runtime::UiRuntime;
use std::io;
use tokio::sync::oneshot;

struct Cleanup {
    preparing: Option<oneshot::Sender<()>>,
    closing: Option<oneshot::Sender<()>>,
    release: oneshot::Receiver<()>,
}

impl SessionCompanion for Cleanup {
    async fn prepare(
        &mut self,
    ) -> io::Result<
        Option<(
            protocol::NativeConnectionTarget,
            protocol::NativeAuthentication,
        )>,
    > {
        let _ = self.preparing.take().unwrap().send(());
        std::future::pending().await
    }

    async fn close(&mut self) {
        let _ = self.closing.take().unwrap().send(());
        receive(&mut self.release).await;
    }
}

/// Observes the actual worker boundary within the fixture's finite scheduling budget.
async fn receive(receiver: &mut oneshot::Receiver<()>) {
    tokio::time::timeout(test_time::DEFAULT_TIMEOUT, receiver)
        .await
        .expect("original session worker did not reach its fixture boundary")
        .expect("original session fixture boundary was dropped");
}

/// Uses the real menu-service chain and SessionResources, with no renderer or account service.
fn app(
    menu: MenuRuntime,
    network: NetworkHandle<PackApplication>,
    controller: SessionController,
) -> App {
    let generation = controller.generation;
    let mut app = App::new();
    app.insert_resource(menu)
        .insert_resource(controller)
        .insert_resource(network)
        .init_resource::<client_shell::menu::LauncherServices>()
        .init_resource::<client_runtime::pack_admission::ResourcePackAdmissionState>()
        .insert_resource(crate::runtime::network::session_registries::tests::collision_resource())
        .insert_resource(UiRuntimeResource(UiRuntime::new(generation)))
        .insert_resource(client_state::PlayerRuntime(player_state::PlayerState::new(
            generation,
        )))
        .init_resource::<crate::runtime::world::ClientWorld>()
        .init_resource::<crate::runtime::shutdown::FatalError>()
        .insert_resource(client_interaction::movement::MovementTicker::default())
        .insert_resource(client_interaction::movement::LocalPhysicsController::default())
        .insert_resource(client_presentation::local_player::LocalPlayerFrameCarrier::default())
        .insert_resource(client_presentation::local_player::InteractionOriginSnapshot::default())
        .add_systems(Update, crate::shell_adapters::menu_services());
    app
}

#[test]
fn account_storage_waits_for_original_session_cleanup() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    let tasks = tokio_util::task::TaskTracker::new();
    let (execution, _stop) =
        bridge::tasks::NativeExecution::for_process(runtime.handle().clone(), tasks.clone());
    let layout = launcher::test_support::scratch("account-session-retirement");
    let menu = MenuRuntime::new_with_layout(
        true,
        Some(2),
        "Cleanup fixture".into(),
        layout,
        client_shell::player_skin::LocalPlayerSkin::generated_default("Cleanup fixture"),
    );
    let (preparing, mut prepared) = oneshot::channel();
    let (closing, mut close_entered) = oneshot::channel();
    let (release, cleanup) = oneshot::channel();
    let controller = SessionController::default();
    let config = client_session::NetworkConfig::new(
        simulation_clock::identity::SessionGeneration::new(1),
        protocol::SessionEndpoint::Native(protocol::NativeConnectionTarget::new(
            "unused.invalid:19132".into(),
            protocol::NativeTransport::RakNet,
        )),
        "Cleanup fixture".into(),
        protocol::ClientRandomId::new(1).unwrap(),
        BlobCache::default().cache(),
        menu.player_skin().to_client_skin(),
        protocol::LoginSettings::default(),
    )
    .with_retirement_tracker(controller.network_workers.clone());
    let network = client_session::spawn_network_with_companion::<PackApplication>(
        config,
        execution,
        |_, _, _| None,
        client_session::SessionTrace::default(),
        None,
        Cleanup {
            preparing: Some(preparing),
            closing: Some(closing),
            release: cleanup,
        },
    )
    .unwrap();
    runtime.block_on(receive(&mut prepared));
    let mut app = app(menu, NetworkHandle::native(network), controller);
    {
        let mut menu = app.world_mut().resource_mut::<MenuRuntime>();
        menu.activate(launcher::menu::MenuAction::SignOut);
        menu.sign_out_locally();
    }
    app.update();
    runtime.block_on(receive(&mut close_entered));
    let admitted_during_cleanup =
        test_support::account_storage_started(app.world().resource::<MenuRuntime>());

    let _ = release.send(());
    tasks.close();
    runtime.block_on(async {
        tokio::time::timeout(test_time::DEFAULT_TIMEOUT, tasks.wait())
            .await
            .expect("original session worker did not retire after cleanup");
    });
    app.update();
    let admitted_after_cleanup =
        test_support::account_storage_started(app.world().resource::<MenuRuntime>());
    drop(app);

    assert!(
        !admitted_during_cleanup,
        "account storage was admitted before original session cleanup retired"
    );
    assert!(
        admitted_after_cleanup,
        "original session cleanup did not release the queued account operation"
    );
}
