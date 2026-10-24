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
        "#version 330\nin vec2 carry; out vec4 color; void main() { vec2 carry=vec2(1.0); color=vec4(carry,0.0,1.0); }",
        "#version 330\nin vec2 carry; out vec4 color; void main() { struct Data { vec2 carry; }; Data d=Data(vec2(1.0)); color=vec4(d.carry,0.0,1.0); }",
    ] {
        let translated = translate(
            "#version 330\nvoid main() { carry=vec2(0.25); gl_Position=vec4(0.0); }",
            fragment,
        );
        assert!(!program(&translated, "composite").is_ok());
    }
}

#[test]
fn compatibility_vertex_locals_and_parameters_do_not_create_outputs() {
    for (helper, body) in [
        ("", "vec2 uv=vec2(1.0); sourceUv=uv;"),
        (
            "vec2 readUv(vec2 uv) { return uv; }",
            "sourceUv=readUv(vec2(1.0));",
        ),
        ("", "{ vec2 uv=vec2(1.0); sourceUv=uv; }"),
        (
            "",
            "for (int uv=0; uv<2; uv++) { sourceUv=vec2(float(uv)); }",
        ),
    ] {
        let translated = success(
            &format!(
                "#version 450\nlayout(location=0) out vec2 sourceUv; {helper} void main() {{ {body} gl_Position=vec4(0.0); }}"
            ),
            "#version 450\nlayout(location=0) in vec2 uv; out vec4 color; void main() { color=vec4(uv,0.0,1.0); }",
        );
        let module = &program(&translated, "composite").stages[0]
            .result
            .as_ref()
            .unwrap()
            .module;
        assert_eq!(interface_locations(module, true), [("sourceUv", 0)]);
    }
}

#[test]
fn compatibility_vertex_block_globals_do_not_create_outputs() {
    for (declaration, value) in [
        ("uniform Data { vec2 uv; };", "uv"),
        ("uniform Data { vec2 value; } uv;", "uv.value"),
        ("struct Data { vec2 value; } uv;", "uv.value"),
    ] {
        let translated = success(
            &format!(
                "#version 450\n{declaration} layout(location=0) out vec2 sourceUv; void main() {{ sourceUv={value}; gl_Position=vec4(0.0); }}"
            ),
            "#version 450\nlayout(location=0) in vec2 uv; out vec4 color; void main() { color=vec4(uv,0.0,1.0); }",
        );
        let module = &program(&translated, "composite").stages[0]
            .result
            .as_ref()
            .unwrap()
            .module;
        assert_eq!(interface_locations(module, true), [("sourceUv", 0)]);
    }
}

#[test]
fn compatibility_vertex_references_after_local_scope_still_create_outputs() {
    success(
        "#version 330\nvoid main() { { vec2 carry=vec2(1.0); gl_Position=vec4(carry,0.0,1.0); } carry=vec2(0.25,0.75); }",
        "#version 330\nin vec2 carry; out vec4 color; void main() { color=vec4(carry,0.0,1.0); }",
    );
}

#[test]
fn compatibility_implicit_vertex_attributes_do_not_create_outputs() {
    success(
        "#version 330\nlayout(location=0) out vec2 sourceUv; void main() { sourceUv=vaUV0; gl_Position=vec4(0.0); }",
        "#version 330\nlayout(location=0) in vec2 vaUV0; out vec4 color; void main() { color=vec4(vaUV0,0.0,1.0); }",
    );
}

#[test]
fn compatibility_local_struct_fields_do_not_create_outputs() {
    for name in ["uv", "Data"] {
        success(
            "#version 450\nlayout(location=0) out vec2 sourceUv; void main() { struct Data { vec2 uv; }; Data d=Data(vec2(1.0)); sourceUv=d.uv; gl_Position=vec4(0.0); }",
            &format!(
                "#version 450\nlayout(location=0) in vec2 {name}; out vec4 color; void main() {{ color=vec4({name},0.0,1.0); }}"
            ),
        );
    }
}

#[test]
fn compatibility_initializer_callees_do_not_create_outputs() {
    success(
        "#version 450\nvec2 uv() { return vec2(1.0); } vec2 value=uv(); layout(location=0) out vec2 sourceUv; void main() { sourceUv=value; gl_Position=vec4(0.0); }",
        "#version 450\nlayout(location=0) in vec2 uv; out vec4 color; void main() { color=vec4(uv,0.0,1.0); }",
    );
}

#[test]
fn compatibility_repaired_outputs_initialize_at_user_main_entry() {
    let translated = success(
        "#version 330\nfloat initialize() { carry=vec2(0.25); return 1.0; } float value=initialize(); void main() { gl_Position=vec4(value); }",
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
    let initialize = module
        .functions
        .iter()
        .find(|(_, f)| f.name.as_deref() == Some("initialize"))
        .unwrap()
        .0;
    let (main_handle, main) = module
        .functions
        .iter()
        .find(|(_, f)| f.name.as_deref() == Some("iris_user_main"))
        .unwrap();
    let first_effect = main.body.iter().find(|statement| {
        matches!(
            statement,
            naga::Statement::Store { .. } | naga::Statement::Call { .. }
        )
    });
    assert!(
        matches!(first_effect, Some(naga::Statement::Store { pointer, .. }) if matches!(main.expressions[*pointer], naga::Expression::GlobalVariable(g) if g == carry)),
        "the repaired output is initialized at user main entry"
    );
    assert!(
        module.functions.iter().any(|(_, function)| {
            let initialize = function.body.iter().position(
                |s| matches!(s, naga::Statement::Call { function, .. } if *function == initialize),
            );
            let main = function.body.iter().position(
                |s| matches!(s, naga::Statement::Call { function, .. } if *function == main_handle),
            );
            matches!((initialize, main), (Some(initialize), Some(main)) if initialize < main)
        }),
        "global initializers still run before user main entry"
    );
}

#[test]
fn compatibility_binding_implicit_inputs_remain_attributes() {
    for (name, ty, qualifier) in [
        ("mc_chunkFade", "float", ""),
        ("dhMaterialId", "int", "flat"),
    ] {
        let translated = success(
            &format!(
                "#version 450\nlayout(location=0) {qualifier} out {ty} sourceValue; void main() {{ sourceValue={name}; gl_Position=vec4(0.0); }}"
            ),
            &format!(
                "#version 450\nlayout(location=0) {qualifier} in {ty} {name}; out vec4 color; void main() {{ color=vec4(float({name})); }}"
            ),
        );
        let module = &program(&translated, "composite").stages[0]
            .result
            .as_ref()
            .unwrap()
            .module;
        assert_eq!(interface_locations(module, true), [("sourceValue", 0)]);
        assert!(
            interface_locations(module, false)
                .iter()
                .any(|(input, _)| *input == name)
        );
    }
}

#[test]
fn compatibility_binding_unbraced_locals_do_not_create_outputs() {
    for body in [
        "if (true) vec2 uv=vec2(1.0);",
        "if (false) {} else vec2 uv=vec2(1.0);",
        "while (false) vec2 uv=vec2(1.0);",
        "for (int i=0; i<1; i++) vec2 uv=vec2(1.0);",
        "do vec2 uv=vec2(1.0); while (false);",
    ] {
        success(
            &format!(
                "#version 450\nlayout(location=0) out vec2 sourceUv; void main() {{ {body} sourceUv=vec2(1.0); gl_Position=vec4(0.0); }}"
            ),
            "#version 450\nlayout(location=0) in vec2 uv; out vec4 color; void main() { color=vec4(uv,0.0,1.0); }",
        );
    }
}

#[test]
fn compatibility_bare_condition_inputs_repair_missing_vertex_variables() {
    success(
        "#version 450\nvoid main() { carry=1; gl_Position=vec4(0.0); }",
        "#version 450\nflat in int carry; out vec4 color; void main() { switch(carry) { case 1: color=vec4(1.0); break; default: color=vec4(0.0); } }",
    );
}

#[test]
fn compatibility_repair_preserves_other_bound_vertex_names() {
    success(
        "#version 450\nlayout(location=0) out vec2 sourceUv; void main() { vec2 uv=vec2(1.0); sourceUv=uv; carry=1; gl_Position=vec4(0.0); }",
        "#version 450\nlayout(location=0) in vec2 uv; layout(location=1) flat in int carry; out vec4 color; void main() { color=vec4(uv,float(carry),1.0); }",
    );
}

#[test]
fn compatibility_missing_variables_use_resolved_fragment_reads() {
    for (vertex, fragment) in [
        (
            "#version 330\nvoid main() { carry=vec2(0.25); gain=0.75; gl_Position=vec4(0.0); }",
            "#version 330\nin vec2 carry; in float gain; out vec4 color; void main() { color=vec4(carry,gain,1.0); }",
        ),
        (
            "#version 120\nvoid main() { texture=vec2(0.25); gl_Position=vec4(0.0); }",
            "#version 120\nvarying vec2 texture; void main() { gl_FragColor=vec4(texture,0.0,1.0); }",
        ),
        (
            "#version 330\nvoid main() { carry=mat2(1.0); gl_Position=vec4(0.0); }",
            "#version 330\nin mat2 carry; out vec4 color; void main() { color=vec4(carry[0],carry[1]); }",
        ),
        (
            "#version 330\nvoid main() { carry=vec2(0.25); gl_Position=vec4(0.0); }",
            "#version 330\nin vec2 carry; vec2 readUnused() { return carry; } out vec4 color; void main() { color=vec4(1.0); }",
        ),
    ] {
        success(vertex, fragment);
    }
}
