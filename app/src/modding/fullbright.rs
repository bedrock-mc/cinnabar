use crate::{app::ClientFrameSet, runtime::world::ClientWorld};
use bevy::prelude::*;
use render::WorldFullbright;

pub(super) fn configure(app: &mut App) {
    app.add_systems(
        Update,
        publish
            .after(ClientFrameSet::NetworkSend)
            .before(ClientFrameSet::UiPreparation),
    );
}

fn publish(
    runtime: Option<Res<super::ModRuntime>>,
    world: Option<Res<ClientWorld>>,
    mut lighting: ResMut<WorldFullbright>,
) {
    let enabled = world
        .as_deref()
        .is_some_and(|world| world.stream.is_some() && world.fatal_error.is_none())
        && runtime.as_deref().is_some_and(|runtime| {
            !runtime.suspended
                && (0..runtime.host_count()).any(|index| runtime.host(index).fullbright())
        });
    if lighting.0 != enabled {
        lighting.0 = enabled;
    }
}
