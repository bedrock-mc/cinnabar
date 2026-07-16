use crate::chunk::*;

pub(in crate::chunk) fn install_opaque_commands(render_app: &mut SubApp) {
    render_app
        .add_render_command::<Opaque3d, DrawChunkCommands>()
        .add_render_command::<Opaque3d, DrawChunkIndirectCommands>();
}
