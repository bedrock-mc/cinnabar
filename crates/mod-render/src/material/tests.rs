use super::*;

const SOLID: &str = "fn shade(input: MaterialInput) -> Surface { return Surface(input.color.rgb, input.color.a, vec3<f32>(0.0)); }";

#[test]
fn accepts_builtin_and_procedural_surface_inputs() {
    compose(BUILTIN).unwrap();
    compose(SOLID).unwrap();
    compose("fn shade(input: MaterialInput) -> Surface { return Surface(vec3<f32>(input.uv, phase()), input.distance, input.local_position * param(input, 0u)); }").unwrap();
}

#[test]
fn rejects_resources_entry_points_depth_and_unbounded_work() {
    for source in [
        format!("@group(1) @binding(0) var tex: texture_2d<f32>; {SOLID}"),
        format!("var<private> state: f32; {SOLID}"),
        format!("@vertex fn guest() -> @builtin(position) vec4<f32> {{ return vec4<f32>(0.0); }} {SOLID}"),
        format!("@fragment fn guest() -> @builtin(frag_depth) f32 {{ return 0.0; }} {SOLID}"),
        "fn shade(input: MaterialInput) -> Surface { loop {} return Surface(vec3<f32>(0.0), 1.0, vec3<f32>(0.0)); }".into(),
        "fn shade(input: MaterialInput) -> vec4<f32> { return input.position; }".into(),
        format!("@group(1) @binding(0) var<storage, read_write> values: array<f32>; {SOLID}"),
    ] {
        assert!(compose(&source).is_err(), "accepted {source}");
    }
}

#[test]
fn expanded_call_trees_and_source_size_are_bounded() {
    let mut source = "fn f0(x: f32) -> f32 { return x*x; }".to_owned();
    for index in 1..14 {
        let previous = index - 1;
        source.push_str(&format!(
            "fn f{index}(x: f32) -> f32 {{ return f{previous}(x)+f{previous}(x); }}"
        ));
    }
    source.push_str("fn shade(input: MaterialInput) -> Surface { return Surface(vec3<f32>(f13(input.uv.x)), 1.0, vec3<f32>(0.0)); }");
    assert!(compose(&source).unwrap_err().contains("expressions"));
    assert!(compose(&" ".repeat(mod_api::MAX_MATERIAL_SHADER_BYTES + 1)).is_err());
}

#[test]
fn explicit_phase_is_bounded_and_continuous_at_wrap() {
    let wrap = mod_api::materials::ANIMATION_WRAP_SECONDS;
    assert_eq!(phase(0.), 0.);
    assert_eq!(phase(wrap * 1.25), 0.25);
    assert_eq!(phase(wrap), 0.);
    assert!(phase(wrap - 1e-10) < 1.);
    assert_eq!(phase(f64::NAN), 0.);
}
