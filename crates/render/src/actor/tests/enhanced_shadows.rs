//! Animated actor caster and receiver pipeline contracts.

use bevy::render::render_resource::{CompareFunction, TextureFormat};

fn shader(definitions: &[&str]) -> String {
    let shader = crate::shader_safety::from_actor_wgsl(
        include_str!("../../actor.wgsl"),
        "actor.wgsl",
        crate::actor::ACTOR_GPU_INSTANCE_WORDS,
        render_model::ACTOR_RIG_VERTEX_WORDS,
    );
    let bevy::shader::Source::Wgsl(source) = shader.source else {
        panic!("actor source is WGSL");
    };
    crate::shader_source::composed(&source, definitions)
}

#[test]
fn actor_caster_writes_conventional_depth_without_color_or_backface_culling() {
    let descriptor = crate::actor_render::actor_shadow_pipeline_descriptor(
        crate::enhanced::enhanced_caster_layout(),
        TextureFormat::Depth32Float,
    );
    assert_eq!(descriptor.layout.len(), 3);
    assert!(descriptor.fragment.as_ref().unwrap().targets.is_empty());
    assert_eq!(descriptor.multisample.count, 1);
    assert!(descriptor.primitive.cull_mode.is_none());
    let depth = descriptor.depth_stencil.as_ref().unwrap();
    assert!(depth.depth_write_enabled);
    assert_eq!(depth.depth_compare, CompareFunction::LessEqual);
    assert_eq!(depth.format, TextureFormat::Depth32Float);

    let module = naga::front::wgsl::parse_str(&shader(&["ENHANCED_SHADOW"])).unwrap();
    let fragment = module
        .entry_points
        .iter()
        .find(|entry| {
            entry.name
                == descriptor
                    .fragment
                    .as_ref()
                    .unwrap()
                    .entry_point
                    .as_deref()
                    .unwrap()
        })
        .expect("caster fragment entry point exists");
    assert_eq!(fragment.stage, naga::ShaderStage::Fragment);
    assert!(fragment.function.result.is_none());
}

#[test]
fn actor_caster_projects_interpolated_bones_and_samples_real_skin_coverage() {
    let source = shader(&["ENHANCED_SHADOW"]);
    let module = naga::front::wgsl::parse_str(&source).unwrap();
    let info = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    )
    .validate(&module)
    .unwrap();
    let (vertex_index, _) = module
        .entry_points
        .iter()
        .enumerate()
        .find(|(_, entry)| entry.stage == naga::ShaderStage::Vertex)
        .unwrap();
    let vertex = info.get_entry_point(vertex_index);
    for (group, binding) in [(0, 1), (0, 2), (0, 3), (0, 4), (0, 5), (2, 0)] {
        assert!(
            module.global_variables.iter().any(|(handle, global)| {
                global
                    .binding
                    .as_ref()
                    .is_some_and(|slot| slot.group == group && slot.binding == binding)
                    && !vertex[handle].is_empty()
            }),
            "caster vertex must use group {group} binding {binding}"
        );
    }
    // Real artwork alpha and material class drive coverage in both passes.
    for binding in [6, 7, 8] {
        assert!(crate::shader_test_support::fragment_reads_binding(
            &source, 0, binding
        ));
    }
    crate::shader_test_support::assert_binding_visibility(
        &source,
        0,
        &crate::actor_render::actor_bind_group_layout(),
    );
    crate::shader_test_support::assert_binding_visibility(
        &source,
        2,
        &crate::enhanced::enhanced_caster_layout(),
    );
}

#[test]
fn enhanced_actor_receiver_reads_sun_shadows_and_early_depth_occlusion() {
    let source = shader(&["ENHANCED"]);
    for binding in [1, 2, 5, 9] {
        assert!(
            crate::shader_test_support::fragment_reads_binding(&source, 2, binding),
            "receiver fragment must use group 2 binding {binding}"
        );
    }
    crate::shader_test_support::assert_binding_visibility(
        &source,
        2,
        &crate::enhanced::enhanced_view_layout(),
    );
    let vanilla = shader(&[]);
    let module = naga::front::wgsl::parse_str(&vanilla).unwrap();
    assert!(
        module
            .global_variables
            .iter()
            .all(|(_, global)| { global.binding.as_ref().is_none_or(|slot| slot.group != 2) })
    );
}
