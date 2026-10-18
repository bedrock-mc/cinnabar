//! Installs the current connection fence before any gameplay observation is delivered.

use client_extensions::modding::{drawing_policy::remote_permissions, multi::ModHosts};
use mod_api::drawing::{DrawingPermissions, DrawingSession};

/// Uses trusted session state, never a loopback address, to select local authority.
pub(super) fn bind(
    hosts: &mut ModHosts,
    gameplay: &super::gameplay::GameplayContext<'_>,
    ui: &client_ui::ui_runtime::UiRuntime,
) -> bool {
    let mut changed = false;
    let fence = gameplay
        .world
        .as_ref()
        .and_then(|world| (!world.dimension_transfer.active()).then_some(world.stream.as_ref()?))
        .map(|stream| (ui.session_id(), stream.authority().current_dimension()));
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
        changed |= host.bind_drawing_session(DrawingSession {
            connection,
            dimension,
            local,
            permissions,
        });
    }
    changed
}

/// Publishes availability for only the block anchors in the current retained output.
pub(super) fn publish_frame(
    hosts: &ModHosts,
    gameplay: &super::gameplay::GameplayContext<'_>,
    ui: &client_ui::ui_runtime::UiRuntime,
    frame: &mut render_scene::mod_scene::retained::DrawingFrame,
) {
    *frame = Default::default();
    let Some(world) = gameplay
        .world
        .as_ref()
        .filter(|world| !world.dimension_transfer.active())
    else {
        return;
    };
    let Some(stream) = world.stream.as_ref() else {
        return;
    };
    frame.connection = ui.session_id();
    frame.dimension = stream.authority().current_dimension();
    for (output, _) in hosts.render_outputs() {
        for resources in &output.retained {
            publish_retained(resources, stream.authority().terrain(), frame);
        }
    }
}

/// Adds one retained snapshot's live owner and authoritative block anchors to the frame.
fn publish_retained(
    resources: &mod_render::retained::Resources,
    terrain: &world::ChunkStore,
    frame: &mut render_scene::mod_scene::retained::DrawingFrame,
) {
    if let Some(handle) = resources
        .geometry
        .keys()
        .chain(resources.materials.keys())
        .next()
    {
        frame.live_owners.insert((handle.owner, handle.generation));
    }
    for draw in resources.instances.values() {
        if let mod_render::retained::Anchor::Block {
            dimension,
            position,
        } = draw.anchor
        {
            let key = world::SubChunkKey {
                dimension,
                x: position[0].div_euclid(world::SUB_CHUNK_SIDE as i32),
                y: position[1].div_euclid(world::SUB_CHUNK_SIDE as i32),
                z: position[2].div_euclid(world::SUB_CHUNK_SIDE as i32),
            };
            if dimension == frame.dimension && terrain.is_sub_chunk_loaded(key) {
                frame.loaded_blocks.insert(position);
            }
        }
    }
}

#[cfg(test)]
mod tests;
