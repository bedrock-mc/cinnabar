use super::*;

const GRADE: &str = "fn effect(uv: vec2<f32>) -> vec3<f32> {
    let c = scene(uv);
    return mix(c, vec3<f32>(luminance(c)), param(0u));
}";

#[test]
fn accepts_a_colour_effect_with_prelude_helpers() {
    let module = compose(GRADE, false).unwrap();
    assert!(module.contains(FRAGMENT_ENTRY));
    compose(
        "fn effect(uv: vec2<f32>) -> vec3<f32> { return blur(uv, 2.0) + bloom(uv, 4.0, 0.8); }",
        false,
    )
    .unwrap();
}

#[test]
fn depth_helpers_exist_only_when_requested() {
    let source = "fn effect(uv: vec2<f32>) -> vec3<f32> { return vec3<f32>(depth(uv)); }";
    assert!(compose(source, false).is_err());
    compose(source, true).unwrap();
}

#[test]
fn rejects_resources_storage_and_entry_points() {
    let cases = [
        "@group(0) @binding(9) var<storage, read_write> out: array<u32>;
         fn effect(uv: vec2<f32>) -> vec3<f32> { out[0] = 1u; return scene(uv); }",
        "var<private> state: f32;
         fn effect(uv: vec2<f32>) -> vec3<f32> { return scene(uv) * state; }",
        "@group(0) @binding(4) var other: texture_2d<f32>;
         fn effect(uv: vec2<f32>) -> vec3<f32> { return scene(uv); }",
        "fn effect(uv: vec2<f32>) -> vec3<f32> { return scene(uv); }
         @compute @workgroup_size(1) fn escape() {}",
        "override strength: f32 = 1.0;
         fn effect(uv: vec2<f32>) -> vec3<f32> { return scene(uv) * strength; }",
    ];
    for case in cases {
        assert!(compose(case, false).is_err(), "{case}");
    }
}

#[test]
fn rejects_loops_and_comment_escapes() {
    let looped = "fn effect(uv: vec2<f32>) -> vec3<f32> {
        var c = vec3<f32>(0.0);
        for (var i = 0; i < 4; i++) { c += scene(uv); }
        return c;
    }";
    assert!(compose(looped, false).unwrap_err().contains("loop"));
    let unbounded = "fn effect(uv: vec2<f32>) -> vec3<f32> { loop { } }";
    assert!(compose(unbounded, false).is_err());
    let swallow = "fn effect(uv: vec2<f32>) -> vec3<f32> { return scene(uv); } /*";
    assert!(compose(swallow, false).is_err());
}

#[test]
fn texture_reads_are_bounded_across_call_sites() {
    // Each blur reads nine times; four calls exceed the per-pixel budget.
    let heavy = "fn twice(uv: vec2<f32>) -> vec3<f32> { return blur(uv, 1.0) + blur(uv, 2.0); }
        fn effect(uv: vec2<f32>) -> vec3<f32> { return twice(uv) + twice(uv * 0.5); }";
    let error = compose(heavy, false).unwrap_err();
    assert!(error.contains("reads textures 36 times"), "{error}");
    let fits = "fn effect(uv: vec2<f32>) -> vec3<f32> { return blur(uv, 1.0) + blur(uv, 2.0); }";
    compose(fits, false).unwrap();
}

#[test]
fn expression_count_is_bounded_across_call_sites() {
    let mut source = String::from("fn f(v: f32) -> f32 { var x = v;\n");
    for _ in 0..40 {
        source.push_str("x = sin(x) * 1.5 + 0.25;\n");
    }
    source.push_str("return x; }\nfn effect(uv: vec2<f32>) -> vec3<f32> { var x = uv.x;\n");
    for _ in 0..40 {
        source.push_str("x = f(x);\n");
    }
    source.push_str("return vec3<f32>(x); }");
    let error = compose(&source, false).unwrap_err();
    assert!(error.contains("expressions per pixel"), "{error}");
}

#[test]
fn oversized_and_malformed_sources_report_guest_lines() {
    assert!(
        compose(&" ".repeat(mod_api::MAX_SHADER_BYTES + 1), false)
            .unwrap_err()
            .contains("byte limit")
    );
    let error = compose(
        "fn effect(uv: vec2<f32>) -> vec3<f32> {\n  return nope;\n}",
        false,
    )
    .unwrap_err();
    assert!(error.starts_with("shader line 2:"), "{error}");
    assert!(compose("fn effect(uv: vec2<f32>) -> f32 { return 1.0; }", false).is_err());
}

#[test]
fn uniform_size_matches_the_prelude_struct() {
    assert_eq!(FRAME_UNIFORM_BYTES, 240);
}

#[test]
fn long_statements_are_rejected_before_parsing() {
    let mut chain = String::from("fn effect(uv: vec2<f32>) -> vec3<f32> { return vec3<f32>(uv.x");
    for _ in 0..mod_api::MAX_STATEMENT_TOKENS {
        chain.push_str("+a");
    }
    chain.push_str("); }");
    let error = compose(&chain, false).unwrap_err();
    assert!(error.contains("tokens"), "{error}");
}

#[test]
fn deepest_admitted_statement_validates_from_a_small_stack() {
    // Operator chains nest one level per operator; validation must not exhaust any caller's stack.
    let mut body = String::from("fn effect(uv: vec2<f32>) -> vec3<f32> { let a = uv.x; let b = a");
    while body.matches('+').count() * 2 + 20 < mod_api::MAX_STATEMENT_TOKENS {
        body.push_str("+a");
    }
    body.push_str("; return vec3<f32>(b); }");
    let result = std::thread::Builder::new()
        .stack_size(64 * 1024)
        .spawn(move || compose(&body, false).map(|_| ()))
        .unwrap()
        .join()
        .unwrap();
    result.unwrap();
}

#[test]
fn oversized_values_are_rejected() {
    for case in [
        "fn effect(uv: vec2<f32>) -> vec3<f32> { var big: array<vec4<f32>, 4096>; return big[0].xyz; }",
        "fn effect(uv: vec2<f32>) -> vec3<f32> { return array<vec4<f32>, 4096>()[u32(uv.x)].xyz; }",
    ] {
        let error = compose(case, false).unwrap_err();
        assert!(error.contains("bytes"), "{error}");
    }
}

#[test]
fn comments_do_not_count_towards_statement_tokens() {
    let comment = "+a ".repeat(mod_api::MAX_STATEMENT_TOKENS);
    let source = format!(
        "fn effect(uv: vec2<f32>) -> vec3<f32> {{ // {comment}\n /* {comment} /* nested */ */ return scene(uv); }}"
    );
    compose(&source, false).unwrap();
}
