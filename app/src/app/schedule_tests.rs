use bevy::ecs::schedule::Schedules;

use super::*;

#[test]
fn network_config_call_sites_share_the_process_blob_cache() {
    let owner = ClientBlobCacheOwner::default();
    let first = NetworkConfig {
        session_generation: 7,
        socket_dir: std::path::PathBuf::from("first-core.sock"),
        display_name: "cache-owner".to_owned(),
        client_blob_cache: owner.cache(),
        player_skin: crate::player_skin::LocalPlayerSkin::generated_default("cache-owner"),
        actor_artwork: None,
    };
    let hash = first
        .client_blob_cache
        .insert(b"verified-across-session")
        .expect("seed verified blob before replacement");
    let replacement = NetworkConfig {
        session_generation: 8,
        socket_dir: std::path::PathBuf::from("replacement-core.sock"),
        display_name: "cache-owner".to_owned(),
        client_blob_cache: owner.cache(),
        player_skin: crate::player_skin::LocalPlayerSkin::generated_default("cache-owner"),
        actor_artwork: None,
    };

    assert!(replacement.client_blob_cache.contains(hash));
}

#[test]
fn production_update_schedule_initializes_without_dependency_cycles() {
    let mut app = App::new();
    configure_client_frame_schedule(&mut app);
    app.add_plugins(FlyCameraPlugin::default());
    configure_client_production_frame_systems(&mut app);
    configure_client_runtime_frame_systems(&mut app);
    configure_acceptance_finish_system(&mut app);

    let mut schedules = app
        .world_mut()
        .remove_resource::<Schedules>()
        .expect("Schedules resource");
    let result = schedules
        .get_mut(Update)
        .expect("production Update schedule")
        .initialize(app.world_mut());
    app.world_mut().insert_resource(schedules);

    assert!(result.is_ok(), "production Update schedule: {result:?}");
}
