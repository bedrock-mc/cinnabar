use crate::chunk::*;

pub(in crate::chunk) fn install_model_commands(render_app: &mut SubApp) {
    use crate::chunk::transparent::mixed::{DrawMixedTerrainCommands, MixedTerrainRuntime};
    render_app
        .init_resource::<MixedTerrainRuntime>()
        .add_render_command::<Opaque3d, DrawModelCommands>()
        .add_render_command::<Opaque3d, DrawModelIndirectCommands>()
        .add_render_command::<Transparent3d, DrawTransparentModelCommands>()
        .add_render_command::<Transparent3d, DrawMixedTerrainCommands>();
}
