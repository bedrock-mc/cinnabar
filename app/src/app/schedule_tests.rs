use crate::runtime::network::session::NetworkConfig;
use bevy::ecs::schedule::Schedules;
use simulation_clock::identity::SessionGeneration;

use super::*;

#[test]
fn network_config_call_sites_share_the_process_blob_cache() {
    let owner = ClientBlobCacheOwner::default();
    let registry_base = crate::runtime::network::session_registries::tests::registry_base();
    let first = NetworkConfig {
        registry_base: registry_base.clone(),
        session_generation: SessionGeneration::new(7),
        endpoint: protocol::SessionEndpoint::Core(std::path::PathBuf::from("first-core.sock")),
        #[cfg(feature = "native-networking")]
        native_account: None,
        #[cfg(feature = "native-networking")]
        native_friend: None,
        #[cfg(feature = "native-networking")]
        native_gathering: None,
        #[cfg(feature = "native-networking")]
        native_local_world: None,
        display_name: "cache-owner".to_owned(),
        client_random_id: protocol::ClientRandomId::new(-7).unwrap(),
        client_blob_cache: owner.cache(),
        player_skin: client_shell::player_skin::LocalPlayerSkin::generated_default("cache-owner"),
        login_settings: protocol::LoginSettings::default(),
        actor_artwork: None,
        ui_catalog: None,
        log_dir: std::path::PathBuf::from("logs"),
        record_session: false,
    };
    let hash = first
        .client_blob_cache
        .insert(b"verified-across-session")
        .expect("seed verified blob before replacement");
    let replacement = NetworkConfig {
        registry_base,
        session_generation: SessionGeneration::new(8),
        endpoint: protocol::SessionEndpoint::Core(std::path::PathBuf::from(
            "replacement-core.sock",
        )),
        #[cfg(feature = "native-networking")]
        native_account: None,
        #[cfg(feature = "native-networking")]
        native_friend: None,
        #[cfg(feature = "native-networking")]
        native_gathering: None,
        #[cfg(feature = "native-networking")]
        native_local_world: None,
        display_name: "cache-owner".to_owned(),
        client_random_id: protocol::ClientRandomId::new(-7).unwrap(),
        client_blob_cache: owner.cache(),
        player_skin: client_shell::player_skin::LocalPlayerSkin::generated_default("cache-owner"),
        login_settings: protocol::LoginSettings::default(),
        actor_artwork: None,
        ui_catalog: None,
        log_dir: std::path::PathBuf::from("logs"),
        record_session: false,
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
    session::configure_finish_system(&mut app);

    let mut schedules = app
        .world_mut()
        .remove_resource::<Schedules>()
        .expect("Schedules resource");
    let result = schedules
        .get_mut(Update)
        .expect("production Update schedule")
        .initialize(app.world_mut());
    app.world_mut().insert_resource(schedules);

    assert!(
        result.is_ok(),
        "production Update schedule: {:?}",
        result.err()
    );
}
