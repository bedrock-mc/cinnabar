//! Personal-mod rendering: sandboxed post passes before the HUD and world primitives in the
//! transparent phase. Nothing is queued or drawn while no mod renders. Pass pipelines are
//! owned per pass revision, so replaced or reloaded passes release them.

use render_scene::ModRenderScene;

mod passes;
#[cfg(test)]
pub(crate) use passes::PassGpu;

mod primitives;
#[cfg(test)]
mod tests;

#[cfg(test)]
use bevy::prelude::{Entity, GlobalTransform, Mat4, Msaa, Mut, UVec4, Vec3, World, default};
use bevy::{
    prelude::{App, Plugin, Resource},
    render::{RenderApp, extract_resource::ExtractResourcePlugin},
};
pub use passes::ModPassLabel;

#[derive(Clone, Copy, Debug, Default)]
pub struct ModRenderPlugin;

impl Plugin for ModRenderPlugin {
    fn build(&self, app: &mut App) {
        install(app);
    }

    fn finish(&self, app: &mut App) {
        install(app);
    }
}

#[derive(Resource)]
struct Installed;

fn install(app: &mut App) {
    app.init_resource::<ModRenderScene>();
    crate::pipeline_warmup::register_pending::<passes::PassGpu>(app);
    let Some(render_app) = app.get_sub_app(RenderApp) else {
        return;
    };
    if render_app.world().contains_resource::<Installed>() {
        passes::install_graph(app.sub_app_mut(RenderApp).world_mut());
        return;
    }
    app.add_plugins(ExtractResourcePlugin::<ModRenderScene>::default());
    primitives::install(app);
    app.sub_app_mut(RenderApp).insert_resource(Installed);
    passes::install(app.sub_app_mut(RenderApp));
}

use mod_render::{block_highlights, position_box};
