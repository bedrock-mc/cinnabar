//! Renderer setup shared by direct and launcher sessions.

use bevy::render::{
    RenderPlugin,
    settings::{RenderCreation, WgpuSettings},
};

pub(super) fn render_plugin() -> RenderPlugin {
    let mut settings = WgpuSettings::default();
    settings.limits.max_storage_buffers_per_shader_stage = settings
        .limits
        .max_storage_buffers_per_shader_stage
        .max(render::required_vertex_storage_buffers());
    if let Some(backends) =
        super::preferred_render_backends(std::env::var_os("WGPU_BACKEND").as_deref())
    {
        settings.backends = Some(backends);
    }
    #[cfg(all(feature = "enhanced", target_os = "windows"))]
    render::configure_enhanced_shader_compiler(&mut settings);
    RenderPlugin {
        render_creation: RenderCreation::Automatic(settings),
        ..Default::default()
    }
}
