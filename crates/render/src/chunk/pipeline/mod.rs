pub(in crate::chunk) mod commands;
pub(in crate::chunk) mod layouts;
pub(in crate::chunk) mod liquid;
pub(in crate::chunk) mod model;
pub(in crate::chunk) mod opaque;

use crate::chunk::*;

pub(in crate::chunk) fn install_chunk_commands(render_app: &mut SubApp) {
    opaque::install_opaque_commands(render_app);
    model::install_model_commands(render_app);
    liquid::install_liquid_commands(render_app);
}
