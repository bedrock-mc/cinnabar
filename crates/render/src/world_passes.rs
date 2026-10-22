//! Render-world systems that prepare or queue the world behind the menu panorama.

use bevy::{
    app::SubApp,
    prelude::{IntoScheduleConfigs, Resource, SystemSet},
    render::Render,
};

/// Systems that do nothing while the panorama or an opaque pack screen replaces the world.
///
/// The extracted panorama does not change while the render schedule runs, so one set
/// condition replaces a per-system check of the same state.
#[derive(SystemSet, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct WorldPassSystems;

#[derive(Resource)]
struct Installed;

/// Gates [`WorldPassSystems`] on [`render_scene::panorama::world_passes_enabled`] once per
/// render app, whichever plugin adds the first member.
pub(crate) fn install(app: &mut SubApp) {
    if app.world().contains_resource::<Installed>() {
        return;
    }
    app.insert_resource(Installed).configure_sets(
        Render,
        WorldPassSystems.run_if(render_scene::panorama::world_passes_enabled),
    );
}
