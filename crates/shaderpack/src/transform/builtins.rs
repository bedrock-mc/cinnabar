//! Declares legacy and implicit stage inputs, varyings and fragment outputs.

use super::{ProgramInterface, TransformError, declare, legacy};
use crate::programs::StageKind;

/// Vertex inputs Iris supplies even when the pack omits their declarations.
pub(super) const IMPLICIT_VERTEX_INPUTS: &[(&str, &str)] =
    &[("mc_chunkFade", "float"), ("dhMaterialId", "int")];

/// Adds the built-in stage interface and rejects fragment output location collisions.
pub(super) fn declarations(
    stage: StageKind,
    lg: &legacy::Legacy,
    c: &mut declare::Collected,
    iface: &mut ProgramInterface,
) -> Result<String, TransformError> {
    let mut builtin_io = String::new();
    if stage == StageKind::Vertex {
        // Inputs Iris supplies without the pack declaring them.
        for &(n, ty) in IMPLICIT_VERTEX_INPUTS {
            if !c.declared.contains(n) && lg.toks.iter().any(|t| t.ident() == Some(n)) {
                declare::vertex_input(&mut builtin_io, c, n, ty);
            }
        }
        for (g, n, ty) in [
            ("gl_Vertex", "iris_Vertex", "vec4"),
            ("gl_Color", "iris_Color", "vec4"),
            ("gl_Normal", "iris_Normal", "vec3"),
            ("gl_MultiTexCoord0", "iris_MultiTexCoord0", "vec4"),
            ("gl_MultiTexCoord1", "iris_MultiTexCoord1", "vec4"),
            ("gl_MultiTexCoord2", "iris_MultiTexCoord2", "vec4"),
            ("gl_MultiTexCoord3", "iris_MultiTexCoord3", "vec4"),
        ] {
            if lg.builtins.contains(g) {
                declare::vertex_input(&mut builtin_io, c, n, ty);
            }
        }
        declare::position_parts(&mut builtin_io, c);
    }
    let varying_dir = match stage {
        StageKind::Vertex => Some("out"),
        StageKind::Fragment => Some("in"),
        _ => None,
    };
    if let Some(dir) = varying_dir {
        let front = (stage == StageKind::Vertex && lg.builtins.contains("gl_FrontColor"))
            || (stage == StageKind::Fragment && lg.builtins.contains("gl_Color"));
        let mut varying = |name: &str, ty: &str| {
            let loc = iface.varying(stage, name, 1, None);
            builtin_io.push_str(&format!("layout(location = {loc}) {dir} {ty} {name};\n"));
        };
        if front {
            varying("iris_FrontColor", "vec4");
        }
        if lg.builtins.contains("gl_FogFragCoord") {
            varying("iris_FogFragCoord", "float");
        }
        for t in &lg.tex_coords {
            varying(&format!("iris_TexCoord{t}"), "vec4");
        }
    }
    if stage == StageKind::Fragment {
        for f in &lg.frag_data {
            if c.frag_locations.contains(f) {
                return Err(TransformError::new(
                    format!("gl_FragData[{f}] collides with a declared output"),
                    0,
                ));
            }
            let name = format!("iris_FragData{f}");
            builtin_io.push_str(&format!("layout(location = {f}) out vec4 {name};\n"));
            c.frag_outputs.push((*f, name));
        }
    }

    Ok(builtin_io)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn implicit_inputs_are_host_supplied_attributes() {
        for (name, _) in IMPLICIT_VERTEX_INPUTS {
            assert!(
                !matches!(
                    crate::reflection::VertexAttribute::from_glsl_name(name),
                    crate::reflection::VertexAttribute::Other(_)
                ),
                "{name}"
            );
        }
    }
}
