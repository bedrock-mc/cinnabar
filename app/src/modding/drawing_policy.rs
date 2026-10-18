//! Installs the current connection fence before any gameplay observation is delivered.

use client_extensions::modding::{drawing_policy::remote_permissions, multi::ModHosts};
use mod_api::drawing::{DrawingPermissions, DrawingSession};

/// Uses trusted session state, never a loopback address, to select local authority.
pub(super) fn bind(
    hosts: &mut ModHosts,
    gameplay: &super::gameplay::GameplayContext<'_>,
    ui: &client_ui::ui_runtime::UiRuntime,
) {
    let fence = gameplay
        .world
        .as_ref()
        .and_then(|world| (!world.dimension_transfer.active()).then_some(world.stream.as_ref()?))
        .map(|stream| {
            let authority = stream.authority();
            (authority.actor_session_id(), authority.current_dimension())
        });
    let grant = match &ui.experiences.session.state {
        server_experience::session::State::Granted(grant) => Some(grant),
        _ => None,
    };
    for index in 0..hosts.host_count() {
        let host = hosts.host_mut(index);
        let (connection, dimension) = fence.unwrap_or_default();
        let local = gameplay
            .session
            .as_ref()
            .is_some_and(|session| session.is_local_world());
        let permissions = if connection == 0 {
            DrawingPermissions::default()
        } else if local {
            DrawingPermissions::ALL
        } else {
            remote_permissions(
                grant,
                &host.source_digest(),
                client_ui::experience_session::unix_seconds(),
            )
        };
        host.bind_drawing_session(DrawingSession {
            connection,
            dimension,
            local,
            permissions,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::{ecs::system::SystemState, prelude::World};
    use chunk_pipeline::WorldStream;
    use client_ui::ui_runtime::UiRuntime;
    use mod_host::{ModGrants, ModHost};
    use protocol::WorldBootstrap;

    /// A real world authority allocates its own actor incarnation independently of UI sessions.
    fn stream() -> WorldStream {
        WorldStream::new(WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 1,
            local_player_unique_id: 1,
            player_position: [0.; 3],
            world_spawn_position: [0; 3],
            air_network_id: 0,
            block_network_ids_are_hashes: false,
        })
    }

    #[test]
    fn drawing_fence_tracks_actor_incarnations_when_ui_generation_differs() {
        let bytes = br#"(component
            (core module $m (func (export "init")) (func (export "frame")))
            (core instance $i (instantiate $m))
            (func (export "init") (canon lift (core func $i "init")))
            (func (export "frame") (canon lift (core func $i "frame"))))"#;
        let directory = tempfile::tempdir().unwrap();
        let host = ModHost::load_snapshot_with_grants(
            &directory.path().join("selection.wasm"),
            bytes,
            ModGrants::default(),
        )
        .unwrap();
        let mut hosts = ModHosts::new(host, vec![]);
        let stream = stream();
        let actor_session = stream.authority().actor_session_id();
        let ui = UiRuntime::new(actor_session + 1);
        let mut world = World::new();
        world.insert_resource(crate::runtime::world::ClientWorld {
            stream: Some(stream),
            ..Default::default()
        });
        let mut state = SystemState::<super::super::gameplay::GameplayContext>::new(&mut world);
        bind(&mut hosts, &state.get_mut(&mut world).unwrap(), &ui);
        assert_eq!(
            hosts.host(0).drawing_session().unwrap().connection,
            actor_session
        );

        let replacement = self::stream();
        let replacement_session = replacement.authority().actor_session_id();
        world
            .resource_mut::<crate::runtime::world::ClientWorld>()
            .stream = Some(replacement);
        bind(&mut hosts, &state.get_mut(&mut world).unwrap(), &ui);
        assert_eq!(
            hosts.host(0).drawing_session().unwrap().connection,
            replacement_session
        );
        assert_ne!(actor_session, replacement_session);
        world
            .resource_mut::<crate::runtime::world::ClientWorld>()
            .stream = None;
        bind(&mut hosts, &state.get_mut(&mut world).unwrap(), &ui);
        assert_eq!(hosts.host(0).drawing_session().unwrap().connection, 0);
    }
}
