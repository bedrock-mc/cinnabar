use bevy::render::render_resource::RenderPipelineDescriptor;

pub(crate) const SHADER_DEF: &str = "ALPHA_TO_COVERAGE";

/// Coverage varyings and fragment alpha tests share the multisampled cutout specialization.
pub(crate) fn apply(descriptor: &mut RenderPipelineDescriptor, cutout: bool) {
    let enabled = cutout && descriptor.multisample.count > 1;
    descriptor.multisample.alpha_to_coverage_enabled = enabled;
    if enabled {
        descriptor.vertex.shader_defs.push(SHADER_DEF.into());
        descriptor
            .fragment
            .as_mut()
            .expect("cutout color fragment")
            .shader_defs
            .push(SHADER_DEF.into());
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod actor_uv_boundary_tests;
