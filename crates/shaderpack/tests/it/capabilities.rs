//! Device capability checks on translated modules from every supported stage.

use crate::reflection;
use naga::valid::{Capabilities, TypeError, ValidationError, ValidationFlags, Validator};
use shaderpack::{PackSource, ShaderPack, StageKind, TranslateConfig};

/// Translates render and compute programs and checks each module's cube-array requirement.
fn check_cube_array_requirement(declaration: &str, value: &str, required: bool) {
    let translation = ShaderPack::new(PackSource::from_files([
        (
            "composite.vsh",
            format!("#version 450\n{declaration}\nvoid main() {{ gl_Position = {value}; }}"),
        ),
        (
            "composite.fsh",
            format!(
                "#version 450\n{declaration}\nout vec4 color; void main() {{ color = {value}; }}"
            ),
        ),
        (
            "begin.csh",
            format!(
                "#version 450\n{declaration}\nlayout(local_size_x=1) in;
                layout(std430, binding=0) buffer Result {{ vec4 value; }} result;
                void main() {{ result.value = {value}; }}"
            ),
        ),
    ]))
    .translate(&TranslateConfig::default());
    let mut stages = Vec::new();
    for program in &translation.programs {
        reflection(program);
        for stage in &program.stages {
            let compiled = stage.result.as_ref().unwrap();
            stages.push(stage.stage);
            let capabilities = Capabilities::all() - Capabilities::CUBE_ARRAY_TEXTURES;
            let result =
                Validator::new(ValidationFlags::all(), capabilities).validate(&compiled.module);
            if required {
                let error = result.expect_err("live cube arrays require device support");
                assert!(
                    matches!(
                        error.as_inner(),
                        ValidationError::Type {
                            source: TypeError::MissingCapability(capability),
                            ..
                        } if *capability == Capabilities::CUBE_ARRAY_TEXTURES
                    ),
                    "{:?}: {error:?}",
                    stage.stage
                );
                Validator::new(ValidationFlags::all(), Capabilities::all())
                    .validate(&compiled.module)
                    .expect("cube-array support permits the module");
            } else {
                result.unwrap_or_else(|error| panic!("{:?}: {error:?}", stage.stage));
            }
        }
    }
    stages.sort();
    assert_eq!(
        stages,
        [StageKind::Vertex, StageKind::Fragment, StageKind::Compute]
    );
}

#[test]
fn unused_cube_array_aliases_do_not_require_device_support() {
    check_cube_array_requirement("", "vec4(1.0)", false);
}

#[test]
fn live_cube_arrays_still_require_device_support() {
    check_cube_array_requirement(
        "uniform samplerCubeArray cube;",
        "textureLod(cube, vec4(1.0), 0.0)",
        true,
    );
}
