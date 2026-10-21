//! Original programs pin translation boundaries, logical values and resource contracts.

use crate::{program, reflection};
use shaderpack::{PackSource, PackTranslation, Phase, ShaderPack, TranslateConfig};

const VERTEX: &str = "#version 330\nvoid main() { gl_Position = vec4(0.0); }";
const FRAGMENT: &str = "#version 330\nout vec4 color; void main() { color = vec4(1.0); }";

/// Translates an original in-memory render program without external fixtures.
fn translate(vertex: &str, fragment: &str) -> PackTranslation {
    ShaderPack::new(PackSource::from_files([
        ("composite.vsh", vertex),
        ("composite.fsh", fragment),
    ]))
    .translate(&TranslateConfig::default())
}

/// Requires a usable program and returns its complete translation for module inspection.
fn success(vertex: &str, fragment: &str) -> PackTranslation {
    let translation = translate(vertex, fragment);
    reflection(program(&translation, "composite"));
    translation
}

#[test]
fn conflicting_resolved_uniform_types_are_link_errors() {
    let translation = translate(
        "#version 330\nconst int N = 2; uniform float weights[N]; void main() { gl_Position = vec4(weights[0]); }",
        "#version 330\nconst int N = 3; uniform float weights[N]; out vec4 color; void main() { color = vec4(weights[0]); }",
    );
    let failed = program(&translation, "composite");
    assert!(!failed.is_ok());
    assert!(failed.diagnostics.iter().any(|d| d.phase == Phase::Link));
}

#[test]
fn boolean_scalar_and_vector_uniform_arguments_keep_logical_types() {
    let translation = success(
        VERTEX,
        "#version 450
        uniform bool enabled; uniform bvec3 mask; uniform bvec2 pairs[2];
        bool choose(bool flag, bvec3 value, bvec2 pair) {
            return flag && all(value) && any(pair);
        }
        out vec4 color;
        void main() { color = vec4(choose(enabled, mask, pairs[1]) ? 1.0 : 0.0); }",
    );
    let block = reflection(program(&translation, "composite"))
        .uniforms
        .as_ref()
        .unwrap();
    assert_eq!(
        block
            .members
            .iter()
            .map(|m| m.name.as_str())
            .collect::<Vec<_>>(),
        ["enabled", "mask", "pairs"]
    );
    assert_eq!(
        block.members[0].ty,
        shaderpack::ValueType::Scalar(shaderpack::ScalarType::Int)
    );
    assert_eq!(
        block.members[1].ty,
        shaderpack::ValueType::Vector {
            size: 3,
            scalar: shaderpack::ScalarType::Int
        }
    );
    assert_eq!(block.members[2].array.unwrap().0, 2);
}

#[test]
fn increment_operands_keep_their_original_bindings() {
    success(
        VERTEX,
        "#version 330
        const float a = 1.0;
        out vec4 color;
        void main() { float b = 2.0; float c = a + ++b; float d = a - --b; color = vec4(c, d, b, 1.0); }",
    );
}

#[test]
fn boolean_array_indices_rewrite_nested_uniform_references() {
    success(
        VERTEX,
        "#version 330
        uniform bool flags[2][2]; uniform bool selected;
        out vec4 color;
        void main() { color = vec4(flags[selected ? 0 : 1][flags[0][0] ? 1 : 0] ? 1.0 : 0.0); }",
    );
}

#[test]
fn local_initializers_read_outer_bindings_before_shadowing() {
    success(
        VERTEX,
        "#version 330
        uniform bool enabled;
        out vec4 color;
        void main() {
            float result = 0.0;
            { int enabled = enabled ? 1 : 0; result = float(enabled); }
            { struct State { float value; }; State enabled = State(0.5); result += enabled.value; }
            color = vec4(result + (enabled ? 1.0 : 0.0));
        }",
    );
}

#[test]
fn shared_uniform_structs_keep_stage_local_nominal_types() {
    success(
        "#version 330\nuniform float weight; void main() { gl_Position = vec4(weight); }",
        "#version 330
        struct A { vec4 tint; }; struct B { vec4 tint; };
        uniform A first; uniform B second;
        vec4 readA(A state) { return state.tint; }
        vec4 readB(B state) { return state.tint; }
        out vec4 color;
        void main() { color = readA(first) + readB(second); }",
    );
}

#[test]
fn same_size_resource_blocks_require_matching_member_types() {
    for storage in ["uniform", "buffer"] {
        let layout = if storage == "uniform" {
            "std140"
        } else {
            "std430"
        };
        for (vertex_members, fragment_members, vertex_value, fragment_value) in [
            ("vec4 tint;", "ivec4 tint;", "data.tint", "vec4(data.tint)"),
            (
                "vec4 tint[2];",
                "ivec4 tint[2];",
                "data.tint[1]",
                "vec4(data.tint[1])",
            ),
            (
                "State state;",
                "Other state;",
                "data.state.tint",
                "vec4(data.state.tint)",
            ),
            (
                "vec4 tint; float value; vec2 pair;",
                "vec4 tint; vec2 value; float pair;",
                "data.tint + vec4(data.value + data.pair.x)",
                "data.tint + vec4(data.value.x + data.pair)",
            ),
        ] {
            let vertex = format!(
                "#version 450\nstruct State {{ vec4 tint; }};
                layout({layout}, binding=2) {storage} Data {{ {vertex_members} }} data;
                void main() {{ gl_Position = {vertex_value}; }}"
            );
            let fragment = format!(
                "#version 450\nstruct Other {{ ivec4 tint; }};
                layout({layout}, binding=2) {storage} Data {{ {fragment_members} }} data;
                out vec4 color; void main() {{ color = {fragment_value}; }}"
            );
            let translation = ShaderPack::new(PackSource::from_files([
                ("composite.vsh", vertex.as_str()),
                ("composite.fsh", fragment.as_str()),
                ("final.vsh", VERTEX),
                ("final.fsh", FRAGMENT),
            ]))
            .translate(&TranslateConfig::default());
            let failed = program(&translation, "composite");
            assert!(
                !failed.is_ok(),
                "accepted incompatible {storage}: {vertex_members} / {fragment_members}"
            );
            assert!(failed.stages.iter().all(|stage| stage.result.is_ok()));
            assert!(failed.diagnostics.iter().any(|d| d.phase == Phase::Link));
            reflection(program(&translation, "final"));
        }
    }
}

#[test]
fn compatible_resource_blocks_allow_stage_local_member_names() {
    for storage in ["uniform", "buffer"] {
        let layout = if storage == "uniform" {
            "std140"
        } else {
            "std430"
        };
        let translation = success(
            &format!(
                "#version 450
            struct First {{ vec4 tint; }};
            layout({layout}, binding=2) {storage} Data {{ First values[2]; }} data;
            void main() {{ gl_Position = data.values[1].tint; }}"
            ),
            &format!(
                "#version 450
            struct Second {{ vec4 renamed; }};
            layout({layout}, binding=2) {storage} Data {{ Second items[2]; }} data;
            out vec4 color; void main() {{ color = data.items[1].renamed; }}"
            ),
        );
        let r = reflection(program(&translation, "composite"));
        let buffers = if storage == "uniform" {
            &r.uniform_buffers
        } else {
            &r.storage_buffers
        };
        assert_eq!(buffers.len(), 1);
        assert!(buffers[0].stages.vertex && buffers[0].stages.fragment);
    }
}

#[test]
fn unused_resource_layout_differences_do_not_reject_live_stages() {
    success(
        "#version 450
        layout(std430, binding=2) readonly buffer Data { vec4 tint; } data;
        void main() { gl_Position = data.tint; }",
        "#version 450
        layout(std430, binding=2) readonly buffer Data { ivec4 tint; } data;
        out vec4 color; void main() { color = vec4(1.0); }",
    );
}

#[test]
fn resource_helpers_preserve_value_overloads_and_nested_resource_calls() {
    success(
        VERTEX,
        "#version 330
        uniform sampler2D source;
        vec4 lookup(float x) { return vec4(x); }
        vec4 lookup(sampler2D s, vec2 uv) { return texture(s, uv); }
        vec4 nested(sampler2D s, vec2 uv) { return lookup(s, uv); }
        out vec4 color;
        void main() { color = lookup(0.5) + nested(source, vec2(0.5)); }",
    );
}

#[test]
fn specialized_resources_cannot_be_captured_by_helper_locals() {
    success(
        VERTEX,
        "#version 330
        uniform sampler2D source;
        vec4 read(sampler2D s, vec2 uv) { return texture(s, uv); }
        vec4 helper(sampler2D s, vec2 uv) {
            float source = 1.0;
            vec4 result = read(s, uv) * source;
            { float s = 2.0; result *= s; }
            return result + texture(s, uv);
        }
        out vec4 color;
        void main() { color = helper(source, vec2(0.5)); }",
    );
}

#[test]
fn malformed_delimiters_are_isolated_to_their_program() {
    for broken in [
        "#version 120\nvoid main() { gl_Position = texture2D(",
        "#version 330\nvoid main() { float a[] = float[](",
        "#version 330\nlayout(",
        "#version 330\nvoid main() { gl_Position = vec4(0.0]; }",
    ] {
        let translation = ShaderPack::new(PackSource::from_files([
            ("composite.vsh", broken),
            ("composite.fsh", FRAGMENT),
            ("final.vsh", VERTEX),
            ("final.fsh", FRAGMENT),
        ]))
        .translate(&TranslateConfig::default());
        let failed = program(&translation, "composite");
        assert!(!failed.is_ok(), "accepted malformed source: {broken}");
        assert!(failed.all_diagnostics().any(|d| d.location.is_some()));
        reflection(program(&translation, "final"));
    }
}
