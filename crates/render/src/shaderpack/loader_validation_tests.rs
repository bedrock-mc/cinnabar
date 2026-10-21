//! Regressions for rejecting unsupported output destinations before mask construction.

use super::*;

#[test]
fn unsupported_outputs_fail_before_partial_write_masks() {
    for index in [MAX_COLOR_BUFFERS - 1, MAX_COLOR_BUFFERS, 32] {
        for (fragment, properties) in [
            ("if (gl_FragCoord.x < 0.0) discard;", ""),
            ("", "scale.composite=0.5\n"),
        ] {
            let fragment = format!(
                "#version 330 compatibility\n/* RENDERTARGETS:{index} */\n\
                 void main() {{ {fragment} gl_FragData[0] = vec4(1.0); }}"
            );
            let pack = ShaderPack::new(shaderpack::PackSource::from_files([
                (
                    "composite.vsh",
                    "#version 330 compatibility\nvoid main() { gl_Position = gl_Vertex; }",
                ),
                ("composite.fsh", fragment.as_str()),
                ("shaders.properties", properties),
            ]));
            let translation = pack.translate(&Default::default());
            let reflection = translation.programs[0]
                .reflection
                .as_ref()
                .expect("translated output fixture");
            assert_eq!(reflection.color_outputs[0].buffer, Some(index));
            assert_eq!(
                runnable_programs(&translation).is_ok(),
                index < MAX_COLOR_BUFFERS
            );
        }
    }
}
