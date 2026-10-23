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

#[test]
fn compatibility_missing_vertex_declarations_preserve_assigned_values() {
    let translated = success(
        "#version 330\nvoid main() { carry=vec2(0.25,0.75); gl_Position=vec4(0.0); }",
        "#version 330\nin vec2 carry; out vec4 color; void main() { color=vec4(carry,0.0,1.0); }",
    );
    let module = &program(&translated, "composite").stages[0]
        .result
        .as_ref()
        .unwrap()
        .module;
    let carry = module
        .global_variables
        .iter()
        .find(|(_, g)| g.name.as_deref() == Some("carry"))
        .unwrap()
        .0;
    let main = module
        .functions
        .iter()
        .find(|(_, f)| f.name.as_deref() == Some("iris_user_main"))
        .unwrap()
        .1;
    let values: Vec<_> = main.body.iter().filter_map(|s| match s {
        naga::Statement::Store { pointer, value }
            if matches!(main.expressions[*pointer], naga::Expression::GlobalVariable(g) if g == carry) =>
        {
            Some(*value)
        }
        _ => None,
    }).collect();
    let value = *values.last().expect("the vertex keeps its assignment");
    let naga::Expression::Compose { components, .. } = &main.expressions[value] else {
        panic!("assigned vector survives");
    };
    let values: Vec<_> = components
        .iter()
        .map(|v| match main.expressions[*v] {
            naga::Expression::Literal(naga::Literal::F32(value)) => value,
            ref other => panic!("unexpected assigned component: {other:?}"),
        })
        .collect();
    assert_eq!(values, [0.25, 0.75]);
}

#[test]
fn compatibility_missing_vertex_declarations_copy_locations_and_qualifiers() {
    let translated = success(
        "#version 450\nvoid main() { carry=3; gl_Position=vec4(0.0); }",
        "#version 450\nlayout(location=6) flat in int carry; out vec4 color; void main() { color=vec4(float(carry)); }",
    );
    let module = &program(&translated, "composite").stages[0]
        .result
        .as_ref()
        .unwrap()
        .module;
    assert_eq!(interface_locations(module, true), [("carry", 6)]);
}

#[test]
fn compatibility_unused_fragment_inputs_do_not_declare_unknown_vertex_names() {
    for fragment in [
        "#version 330\nin vec2 carry; out vec4 color; void main() { color=vec4(1.0); }",
        "#version 330\nin vec2 carry; struct Data { vec2 carry; }; out vec4 color; void main() { Data d=Data(vec2(1.0)); color=vec4(d.carry,0.0,1.0); }",
    ] {
        let translated = translate(
            "#version 330\nvoid main() { carry=vec2(0.25); gl_Position=vec4(0.0); }",
            fragment,
        );
        assert!(!program(&translated, "composite").is_ok());
    }
}
