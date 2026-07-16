use crate::chunk::*;

pub(in crate::chunk) fn install_liquid_commands(render_app: &mut SubApp) {
    render_app
        .add_render_command::<Opaque3d, DrawDepthLiquidCommands>()
        .add_render_command::<Opaque3d, DrawDepthLiquidIndirectCommands>()
        .add_render_command::<Transparent3d, DrawTransparentLiquidCommands>()
        .add_render_command::<Transparent3d, DrawTransparentLiquidIndirectCommands>();
}
