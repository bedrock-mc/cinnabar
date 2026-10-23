//! Compatibility regressions using small original GLSL programs.

use super::*;

#[test]
fn compatibility_missing_varyings_receive_zero_vertex_outputs() {
    let translation = success(
        VERTEX,
        "#version 450\nlayout(location=6) in vec2 missing;
        layout(location=8) flat in int index;
        out vec4 color; void main() { color = vec4(missing, float(index), 1.0); }",
    );
    let translated = program(&translation, "composite");
    let vertex = &translated.stages[0].result.as_ref().unwrap().module;
    let mut locations: Vec<_> = interface_locations(vertex, true)
        .into_iter()
        .map(|(_, location)| location)
        .collect();
    locations.sort_unstable();
    assert_eq!(locations, [6, 8]);
    let function = &vertex.entry_points[0].function;
    let naga::Statement::Return { value: Some(value) } = function.body.last().unwrap() else {
        panic!("vertex entry returns its outputs");
    };
    let naga::Expression::Compose { components, .. } = &function.expressions[*value] else {
        panic!("vertex result includes the supplied outputs");
    };
    assert!(
        components
            .iter()
            .rev()
            .take(2)
            .all(|value| matches!(function.expressions[*value], naga::Expression::ZeroValue(_)))
    );
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    )
    .validate(vertex)
    .unwrap();
}

#[test]
fn compatibility_empty_declarations_preserve_functions_structs_and_loops() {
    success(
        VERTEX,
        "#version 330\n; struct Sample { float value; }; ;
        float read(Sample s) { return s.value; }; ;
        out vec4 color;
        void main() { float value = 0.0; for (;;) { value = read(Sample(2.0)); break; } ; color = vec4(value); }; ;",
    );
}

#[test]
fn compatibility_negative_constant_sqrt_remains_a_runtime_operation() {
    let translation = success(
        VERTEX,
        "#version 330\nfloat invalid() { return sqrt(-1.0); }
        out vec4 color; void main() { color = vec4(invalid() + sqrt(4.0)); }",
    );
    let fragment = &program(&translation, "composite").stages[1]
        .result
        .as_ref()
        .unwrap()
        .module;
    assert!(
        fragment
            .functions
            .iter()
            .any(
                |(_, function)| function.expressions.iter().any(|(_, expression)| matches!(
                    expression,
                    naga::Expression::Math {
                        fun: naga::MathFunction::Sqrt,
                        ..
                    }
                ))
            )
    );
}
