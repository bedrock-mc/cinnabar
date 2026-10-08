use bevy::render::render_resource::RenderPipelineDescriptor;

pub(crate) const SHADER_DEF: &str = "ALPHA_TO_COVERAGE";

/// Cutout coverage replaces the fragment alpha test only on multisampled color passes.
pub(crate) fn apply(descriptor: &mut RenderPipelineDescriptor, cutout: bool) {
    let enabled = cutout && descriptor.multisample.count > 1;
    descriptor.multisample.alpha_to_coverage_enabled = enabled;
    if enabled {
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
