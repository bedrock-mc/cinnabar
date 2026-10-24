use super::*;
use bevy::{ecs::world::CommandQueue, prelude::World};

/// Native account/world attachment must not wait for a nonexistent legacy executable or endpoint.
#[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
#[test]
fn native_menu_attaches_actual_services_and_world_store_without_a_core_executable() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .unwrap();
    let tasks = tokio_util::task::TaskTracker::new();
    let layout = launcher::test_support::scratch("native-menu-without-core");
    let store = layout.local_worlds_dir();
    assert!(!store.exists());
    let (execution, _stop) =
        bridge::tasks::NativeExecution::for_process(runtime.handle().clone(), tasks.clone());
    let mut menu = MenuRuntime::new_with_layout(
        true,
        Some(2),
        "Fixture".into(),
        layout,
        crate::player_skin::LocalPlayerSkin::generated_default("Fixture"),
    )
    .with_io(execution);
    let mut slot = LauncherServices::default();
    let mut world = World::new();
    let mut worlds = LocalWorlds::default();
    let mut queue = CommandQueue::default();
    slot.drive(
        &mut Commands::new(&mut queue, &world),
        &mut menu,
        true,
        Some(&mut worlds),
    );
    queue.apply(&mut world);
    let account_attached = world.contains_resource::<LauncherAccount>();
    let original_store_opened = store.is_dir();
    drop(worlds);
    drop((slot, menu, world));
    tasks.close();
    runtime.block_on(async {
        tokio::time::timeout(test_time::DEFAULT_TIMEOUT, tasks.wait())
            .await
            .unwrap();
    });
    assert!(
        account_attached,
        "native menu services still required an unrelated Go Core executable"
    );
    assert!(
        original_store_opened,
        "the original native local-world manager was not attached"
    );
}
