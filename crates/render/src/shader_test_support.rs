//! Naga checks that tie bind group layouts to what shader stages actually read.

/// Whether the shader's fragment entry point reads the global at `group`/`binding`.
pub(crate) fn fragment_reads_binding(source: &str, group: u32, binding: u32) -> bool {
    let module = naga::front::wgsl::parse_str(source).expect("shader parses");
    let info = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    )
    .validate(&module)
    .expect("shader validates");
    let (index, _) = module
        .entry_points
        .iter()
        .enumerate()
        .find(|(_, entry)| entry.stage == naga::ShaderStage::Fragment)
        .expect("fragment entry point");
    let entry = info.get_entry_point(index);
    module.global_variables.iter().any(|(handle, global)| {
        global
            .binding
            .as_ref()
            .is_some_and(|slot| slot.group == group && slot.binding == binding)
            && !entry[handle].is_empty()
    })
}

/// Checks each used binding is available to the shader stage that actually accesses it.
pub(crate) fn assert_binding_visibility(
    source: &str,
    group: u32,
    layout: &bevy::render::render_resource::BindGroupLayoutDescriptor,
) {
    use bevy::render::render_resource::ShaderStages;
    let module = naga::front::wgsl::parse_str(source).expect("shader parses");
    let info = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    )
    .validate(&module)
    .expect("shader validates");
    for (index, entry) in module.entry_points.iter().enumerate() {
        let stage = match entry.stage {
            naga::ShaderStage::Vertex => ShaderStages::VERTEX,
            naga::ShaderStage::Fragment => ShaderStages::FRAGMENT,
            naga::ShaderStage::Compute => ShaderStages::COMPUTE,
            _ => continue,
        };
        for (handle, global) in module.global_variables.iter() {
            let Some(binding) = &global.binding else {
                continue;
            };
            if binding.group != group || info.get_entry_point(index)[handle].is_empty() {
                continue;
            }
            let declared = layout
                .entries
                .iter()
                .find(|slot| slot.binding == binding.binding)
                .expect("every used shader resource has a layout entry");
            assert!(
                declared.visibility.contains(stage),
                "group {group} binding {} is unavailable to {:?}",
                binding.binding,
                entry.stage
            );
        }
    }
}
