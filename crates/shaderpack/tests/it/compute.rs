//! Compute programs: dispatch shape and storage images.

use crate::{open_fixture, program, reflection, uniform_names};
use shaderpack::{
    ComputeDispatch, IMAGE_GROUP, ProgramGroup, ProgramKind, StageKind, StageMask,
    TextureDimension, TranslateConfig,
};

const COMPUTE: StageMask = StageMask {
    vertex: false,
    fragment: false,
    compute: true,
};

#[test]
fn fixed_work_groups_and_a_properties_image_format() {
    let Some(pack) = open_fixture("compute") else {
        return;
    };
    let t = pack.translate(&TranslateConfig::default());
    assert_eq!(t.properties.images.len(), 1);
    assert_eq!(t.properties.images[0].name, "scratchImg");

    let begin = program(&t, "begin");
    assert_eq!(begin.program.kind, ProgramKind::Compute);
    assert_eq!(begin.program.group, ProgramGroup::Begin);
    assert_eq!(
        begin.program.stages,
        [(StageKind::Compute, "begin.csh".to_string())]
    );
    let r = reflection(begin);
    assert_eq!(r.kind, ProgramKind::Compute);
    assert_eq!(r.entry_points.len(), 1);
    assert_eq!(r.entry_points[0].0, StageKind::Compute);
    assert_eq!(
        r.compute,
        Some(ComputeDispatch {
            workgroup_size: [8, 8, 1],
            work_groups: Some([4, 2, 1]),
            work_groups_render: None,
        })
    );
    assert!(r.vertex_inputs.is_empty());
    assert!(r.color_outputs.is_empty());
    assert_eq!(uniform_names(r), ["frameTimeCounter"]);
    assert_eq!(r.uniforms.as_ref().unwrap().stages, COMPUTE);

    assert_eq!(r.storage_images.len(), 1);
    let image = &r.storage_images[0];
    assert_eq!(image.name, "scratchImg");
    assert_eq!((image.group, image.binding), (IMAGE_GROUP, 0));
    assert_eq!(image.dimension, TextureDimension::D2);
    // No layout qualifier in the shader: the format is `image.scratchImg`'s RGBA16F.
    assert_eq!(image.format, naga::StorageFormat::Rgba16Float);
    assert_eq!(image.stages, COMPUTE);
}

#[test]
fn render_relative_dispatch_and_explicit_image_formats() {
    let Some(pack) = open_fixture("compute") else {
        return;
    };
    let t = pack.translate(&TranslateConfig::default());
    let pass = program(&t, "composite_a");
    assert_eq!(pass.program.kind, ProgramKind::Compute);
    assert_eq!(pass.program.group, ProgramGroup::Composite);
    let r = reflection(pass);
    assert_eq!(
        r.compute,
        Some(ComputeDispatch {
            workgroup_size: [16, 1, 1],
            work_groups: None,
            work_groups_render: Some([0.5, 0.5]),
        })
    );
    assert!(r.uniforms.is_none());
    let images: Vec<(&str, u32, u32, naga::StorageFormat, bool, bool)> = r
        .storage_images
        .iter()
        .map(|i| {
            (
                i.name.as_str(),
                i.group,
                i.binding,
                i.format,
                i.read,
                i.write,
            )
        })
        .collect();
    assert_eq!(
        images,
        [
            (
                "scratchImg",
                IMAGE_GROUP,
                0,
                naga::StorageFormat::Rgba16Float,
                true,
                false
            ),
            (
                "lumaImg",
                IMAGE_GROUP,
                1,
                naga::StorageFormat::R32Float,
                false,
                true
            ),
        ]
    );
}
