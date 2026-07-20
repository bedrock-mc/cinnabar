use crate::chunk::*;

pub(in crate::chunk) fn install_model_commands(render_app: &mut SubApp) {
    render_app
        .add_render_command::<Opaque3d, DrawModelCommands>()
        .add_render_command::<Opaque3d, DrawModelIndirectCommands>()
        .add_render_command::<Transparent3d, DrawTransparentModelCommands>();
}
