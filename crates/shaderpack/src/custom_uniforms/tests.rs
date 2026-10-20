use std::collections::HashMap;
use std::f32::consts::PI;

use super::{CustomUniformError, CustomUniforms, UniformValue};
use crate::{CustomUniform, CustomUniformKind, Expr};

/// Builds definitions from `kind.type.name = expression` lines, numbered from 1.
fn definitions(lines: &[&str]) -> Vec<CustomUniform> {
    lines
        .iter()
        .zip(1..)
        .map(|(line, number)| {
            let (key, expr) = line.split_once('=').expect("definition has `=`");
            let mut parts = key.trim().splitn(3, '.');
            let kind = match parts.next() {
                Some("uniform") => CustomUniformKind::Uniform,
                Some("variable") => CustomUniformKind::Variable,
                other => panic!("bad kind {other:?}"),
            };
            let ty = parts.next().expect("type").to_owned();
            let name = parts.next().expect("name").to_owned();
            CustomUniform {
                kind,
                ty,
                name,
                expr: Expr::parse_custom(expr).expect("test expression parses"),
                line: number,
            }
        })
        .collect()
}

/// Compiles test declarations and returns their diagnostics.
fn compile(lines: &[&str]) -> (CustomUniforms, Vec<CustomUniformError>) {
    CustomUniforms::new(&definitions(lines))
}

/// Compiles `lines`, asserting every definition compiles.
fn compile_ok(lines: &[&str]) -> CustomUniforms {
    let (uniforms, errors) = compile(lines);
    assert!(errors.is_empty(), "unexpected errors: {errors:?}");
    uniforms
}

/// A built-in lookup answering from `pairs`.
fn builtins(pairs: &[(&str, UniformValue)]) -> impl Fn(&str) -> Option<UniformValue> + use<> {
    let map: HashMap<String, UniformValue> = pairs
        .iter()
        .map(|(name, value)| ((*name).to_owned(), *value))
        .collect();
    move |name: &str| map.get(name).copied()
}

/// Answers no built-in inputs for constant-expression tests.
fn no_builtins(_: &str) -> Option<UniformValue> {
    None
}

/// Returns the named result or fails the test.
fn get(results: &[(String, UniformValue)], name: &str) -> UniformValue {
    results
        .iter()
        .find(|(result, _)| result == name)
        .map(|(_, value)| *value)
        .unwrap_or_else(|| panic!("no result `{name}` in {results:?}"))
}

/// Returns a named float result or fails the test.
fn float(results: &[(String, UniformValue)], name: &str) -> f32 {
    match get(results, name) {
        UniformValue::Float(value) => value,
        other => panic!("`{name}` is {other:?}, expected a float"),
    }
}

/// Checks float results within the evaluator test tolerance.
fn assert_close(actual: f32, expected: f32, what: &str) {
    assert!(
        (actual - expected).abs() <= 1e-4,
        "{what}: expected {expected}, got {actual}"
    );
}

#[test]
fn scalar_results_convert_to_their_declared_types() {
    let mut uniforms = compile_ok(&[
        "uniform.float.sum = 1 + 2 * 3 - 8 / 4",
        "uniform.int.half = 7 / 2",
        "uniform.int.negative_half = -7 / 2",
        "uniform.int.huge = 1e12",
        "uniform.bool.fraction = 0.25",
        "uniform.bool.zero = 0",
        "uniform.float.truth = true",
        "uniform.float.remainder = -7 % 3",
        "uniform.bool.logic = !(2 > 3) && (1 != 1 || 2 <= 2)",
    ]);
    let results = uniforms.evaluate(&no_builtins, 0.0);
    assert_eq!(get(results, "sum"), UniformValue::Float(5.0));
    assert_eq!(get(results, "half"), UniformValue::Int(3));
    assert_eq!(get(results, "negative_half"), UniformValue::Int(-3));
    assert_eq!(get(results, "huge"), UniformValue::Int(i32::MAX));
    assert_eq!(get(results, "fraction"), UniformValue::Bool(true));
    assert_eq!(get(results, "zero"), UniformValue::Bool(false));
    assert_eq!(get(results, "truth"), UniformValue::Float(1.0));
    assert_eq!(get(results, "remainder"), UniformValue::Float(-1.0));
    assert_eq!(get(results, "logic"), UniformValue::Bool(true));
}

#[test]
fn builtin_components_matrix_elements_and_unknown_names_resolve() {
    let mut uniforms = compile_ok(&[
        "uniform.float.height = cameraPosition.y",
        "uniform.float.green = cameraPosition.g + cameraPosition.t + cameraPosition.1",
        "uniform.float.element = gbufferModelView.3.1 * 2",
        "uniform.float.unknown = notAUniform + notAUniform.x + 1",
        "uniform.int.water = isEyeInWater",
        "uniform.float.past_end = cameraPosition.w + 5",
    ]);
    assert_eq!(
        uniforms.inputs().collect::<Vec<_>>(),
        [
            "cameraPosition",
            "gbufferModelView.3.1",
            "notAUniform",
            "isEyeInWater"
        ]
    );
    let lookup = builtins(&[
        ("cameraPosition", UniformValue::Vec3([1.0, 64.5, -3.0])),
        ("gbufferModelView.3.1", UniformValue::Float(-0.25)),
        ("isEyeInWater", UniformValue::Int(2)),
    ]);
    let results = uniforms.evaluate(&lookup, 0.0);
    assert_eq!(float(results, "height"), 64.5);
    assert_eq!(float(results, "green"), 64.5 * 3.0);
    assert_eq!(float(results, "element"), -0.5);
    assert_eq!(float(results, "unknown"), 1.0);
    assert_eq!(get(results, "water"), UniformValue::Int(2));
    // `.w` of a vec3 is a run-time type error, so the uniform falls back to zero.
    assert_eq!(float(results, "past_end"), 0.0);
}

#[test]
fn vectors_operate_per_component_with_scalar_broadcast() {
    let mut uniforms = compile_ok(&[
        "variable.vec3.base = vec3(1, 2, 3) * 2 + 1",
        "uniform.vec3.scaled = base",
        "uniform.float.middle = base.y + base.b",
        "uniform.vec2.product = vec2(1, 2) * vec2(3, 4)",
        "uniform.vec2.absolute = abs(vec2(-1.5, 2))",
        "uniform.vec3.clamped = clamp(vec3(-1, 0.5, 2), 0, 1)",
        "uniform.vec2.smallest = min(vec2(1, 5), vec2(3, 2), vec2(4, 4))",
        "uniform.vec4.mixed = mix(vec4(0, 0, 0, 0), vec4(4, 8, 12, 16), 0.25)",
        "uniform.bool.same = vec2(1, 2) == vec2(1, 2) && vec2(1, 2) != vec2(2, 1)",
        "uniform.vec3.shifted = sunDirection + 1",
    ]);
    let lookup = builtins(&[("sunDirection", UniformValue::Vec3([0.0, -1.0, 0.5]))]);
    let results = uniforms.evaluate(&lookup, 0.0);
    assert_eq!(get(results, "scaled"), UniformValue::Vec3([3.0, 5.0, 7.0]));
    assert_eq!(float(results, "middle"), 12.0);
    assert_eq!(get(results, "product"), UniformValue::Vec2([3.0, 8.0]));
    assert_eq!(get(results, "absolute"), UniformValue::Vec2([1.5, 2.0]));
    assert_eq!(get(results, "clamped"), UniformValue::Vec3([0.0, 0.5, 1.0]));
    assert_eq!(get(results, "smallest"), UniformValue::Vec2([1.0, 2.0]));
    assert_eq!(
        get(results, "mixed"),
        UniformValue::Vec4([1.0, 2.0, 3.0, 4.0])
    );
    assert_eq!(get(results, "same"), UniformValue::Bool(true));
    assert_eq!(get(results, "shifted"), UniformValue::Vec3([1.0, 0.0, 1.5]));
}

#[test]
fn if_chains_pick_the_first_true_branch() {
    let mut uniforms = compile_ok(&[
        "uniform.float.brightness = if(phase == 0, 1.0, phase == 1, 0.75, phase <= 3, 0.5, 0.1)",
        "uniform.vec2.pick = if(phase > 2, vec2(1, 0), vec2(0, 1))",
    ]);
    for (phase, brightness, pick) in [
        (0, 1.0, [0.0, 1.0]),
        (1, 0.75, [0.0, 1.0]),
        (3, 0.5, [1.0, 0.0]),
        (6, 0.1, [1.0, 0.0]),
    ] {
        let lookup = builtins(&[("phase", UniformValue::Int(phase))]);
        let results = uniforms.evaluate(&lookup, 0.0);
        assert_eq!(float(results, "brightness"), brightness, "phase {phase}");
        assert_eq!(
            get(results, "pick"),
            UniformValue::Vec2(pick),
            "phase {phase}"
        );
    }
}

#[test]
fn functions_match_optifine_and_iris_definitions() {
    let cases: &[(&str, f32)] = &[
        ("sin(pi / 2)", 1.0),
        ("cos(0)", 1.0),
        ("tan(0)", 0.0),
        ("asin(1)", PI / 2.0),
        ("acos(1)", 0.0),
        ("atan(1)", PI / 4.0),
        ("atan2(1, -1)", 3.0 * PI / 4.0),
        ("atan(1, -1)", 3.0 * PI / 4.0),
        ("torad(180)", PI),
        ("radians(90)", PI / 2.0),
        ("todeg(pi)", 180.0),
        ("degrees(pi / 4)", 45.0),
        ("exp(0)", 1.0),
        ("exp2(3)", 8.0),
        ("exp10(2)", 100.0),
        ("log(1)", 0.0),
        ("log(2, 8)", 3.0),
        ("log2(32)", 5.0),
        ("log10(1000)", 3.0),
        ("pow(2, 10)", 1024.0),
        ("sqrt(16)", 4.0),
        ("abs(-2.5)", 2.5),
        ("signum(-3)", -1.0),
        ("sign(4)", 1.0),
        ("sign(0)", 0.0),
        ("floor(-1.5)", -2.0),
        ("ceil(1.2)", 2.0),
        ("frac(-1.25)", 0.75),
        ("round(2.5)", 3.0),
        ("round(-2.5)", -2.0),
        ("rint(2.5)", 2.0),
        ("rint(3.5)", 4.0),
        ("fmod(-1, 3)", 2.0),
        ("fmod(7, 3)", 1.0),
        ("min(4, 1, 3)", 1.0),
        ("max(4, 9, 3)", 9.0),
        ("clamp(5, 0, 1)", 1.0),
        ("clamp(-5, 0, 1)", 0.0),
        ("mix(0, 10, 0.25)", 2.5),
        ("edge(0.5, 0.4)", 0.0),
        ("edge(0.5, 0.5)", 1.0),
        ("if(between(5, 1, 10), 1, 0)", 1.0),
        ("if(between(11, 1, 10), 1, 0)", 0.0),
        ("if(equals(1.0, 1.05, 0.1), 1, 0)", 1.0),
        ("if(equals(1.0, 1.5, 0.1), 1, 0)", 0.0),
        ("if(in(3, 1, 2, 3), 1, 0)", 1.0),
        ("if(in(4, 1, 2, 3), 1, 0)", 0.0),
    ];
    let lines: Vec<String> = cases
        .iter()
        .enumerate()
        .map(|(index, (expr, _))| format!("uniform.float.case{index} = {expr}"))
        .collect();
    let lines: Vec<&str> = lines.iter().map(String::as_str).collect();
    let mut uniforms = compile_ok(&lines);
    let results = uniforms.evaluate(&no_builtins, 0.0);
    for (index, (expr, expected)) in cases.iter().enumerate() {
        assert_close(float(results, &format!("case{index}")), *expected, expr);
    }
}

#[test]
fn smooth_converges_on_a_step_over_its_fade_time() {
    let mut uniforms = compile_ok(&["uniform.float.level = smooth(target, 1.0, 1.0)"]);
    let at = |value: f32| builtins(&[("target", UniformValue::Float(value))]);
    // The first evaluation starts at the current value.
    assert_eq!(float(uniforms.evaluate(&at(0.0), 0.1), "level"), 0.0);
    let mut previous = 0.0;
    let mut history = Vec::new();
    for _ in 0..10 {
        let level = float(uniforms.evaluate(&at(1.0), 0.1), "level");
        assert!(level > previous && level < 1.0, "{level} after {previous}");
        previous = level;
        history.push(level);
    }
    assert!((history[0] - 0.5).abs() < 1e-6, "first step {}", history[0]);
    assert!(
        (history[4] - 0.96875).abs() < 1e-6,
        "half way {}",
        history[4]
    );
    assert!(previous > 0.98, "after the fade time {previous}");
    // No elapsed time holds the value.
    assert_eq!(float(uniforms.evaluate(&at(0.0), 0.0), "level"), previous);
}

#[test]
fn restarted_smoothing_starts_again_from_the_current_value() {
    // Photon's `biome_temperature`: a 1000 s fade that must start from the world's biome.
    let mut uniforms = compile_ok(&["uniform.float.level = smooth(target, 1000, 1000)"]);
    let at = |value: f32| builtins(&[("target", UniformValue::Float(value))]);
    assert_eq!(float(uniforms.evaluate(&at(-1.0), 0.1), "level"), -1.0);
    let held = float(uniforms.evaluate(&at(0.6), 0.1), "level");
    assert!(held < -0.99, "a long fade barely moves: {held}");
    uniforms.restart_smoothing();
    assert_eq!(float(uniforms.evaluate(&at(0.6), 0.1), "level"), 0.6);
}

#[test]
fn smooth_uses_separate_rise_and_fall_times() {
    let mut uniforms = compile_ok(&[
        "uniform.float.instant_fall = smooth(target, 2.0, 0.0)",
        "uniform.float.default_fade = smooth(target)",
    ]);
    let at = |value: f32| builtins(&[("target", UniformValue::Float(value))]);
    uniforms.evaluate(&at(0.0), 0.05);
    let results = uniforms.evaluate(&at(1.0), 0.05);
    let rising = float(results, "instant_fall");
    assert!(rising > 0.0 && rising < 0.2, "rising {rising}");
    let default_rise = float(results, "default_fade");
    assert!(
        default_rise > rising,
        "a 1 s fade rises faster than a 2 s one"
    );
    let results = uniforms.evaluate(&at(0.0), 0.05);
    assert_eq!(float(results, "instant_fall"), 0.0);
    assert!(float(results, "default_fade") > 0.0);
}

#[test]
fn smooth_filters_vectors_per_component() {
    let mut uniforms = compile_ok(&["uniform.vec2.pair = smooth(vec2(target, -target), 1, 1)"]);
    let at = |value: f32| builtins(&[("target", UniformValue::Float(value))]);
    uniforms.evaluate(&at(0.0), 0.1);
    let UniformValue::Vec2([x, y]) = get(uniforms.evaluate(&at(2.0), 0.1), "pair") else {
        panic!("pair is not a vec2");
    };
    assert!(x > 0.0 && x < 2.0, "x {x}");
    assert_close(y, -x, "components move symmetrically");
}

#[test]
fn variables_feed_uniforms_without_being_reported() {
    let mut uniforms = compile_ok(&[
        "variable.int.whole = 2.75",
        "variable.bool.flag = 3",
        "uniform.float.doubled = whole * 2",
        "uniform.float.flagged = if(flag, 10, 20)",
        "variable.vec3.direction = vec3(0, height, 0)",
        "uniform.float.up = direction.y",
    ]);
    let lookup = builtins(&[("height", UniformValue::Float(4.0))]);
    let results = uniforms.evaluate(&lookup, 0.0);
    let names: Vec<&str> = results.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(names, ["doubled", "flagged", "up"]);
    assert_eq!(float(results, "doubled"), 4.0);
    assert_eq!(float(results, "flagged"), 10.0);
    assert_eq!(float(results, "up"), 4.0);
}

#[test]
fn invalid_definitions_are_dropped_with_located_errors() {
    let (mut uniforms, errors) = compile(&[
        "uniform.float.early = mystery + 1",
        "variable.float.later = 2",
        "uniform.float.selfish = selfish + 1",
        "uniform.float.mystery = frobnicate(1)",
        "uniform.float.short = clamp(1, 2)",
        "uniform.mat4.matrix = 1",
        "uniform.float.dependent = early * 2",
        "uniform.float.later = 3",
        "uniform.float.flat = vec3(1, 2, 3)",
        "uniform.vec3.mixed = vec2(1, 2) + vec3(1, 2, 3)",
        "uniform.float.scalar_part = later.x",
        "variable.vec2.pair = vec2(1, 2)",
        "uniform.float.past_end = pair.z",
        "uniform.bool.vector_order = pair < 1",
        "uniform.float.odd_if = if(true, 1)",
        "uniform.float.branches = if(true, 1, vec2(1, 1))",
        "uniform.float.macro = if(defined FOO, 1, 0)",
        "uniform.float.fine = later + pair.y + onlyFine",
        "uniform.float.dropped_input = onlyDropped + frobnicate(2)",
    ]);
    let failed: Vec<(u32, &str)> = errors
        .iter()
        .map(|error| (error.line, error.name.as_str()))
        .collect();
    assert_eq!(
        failed,
        [
            (1, "early"),
            (3, "selfish"),
            (4, "mystery"),
            (5, "short"),
            (6, "matrix"),
            (7, "dependent"),
            (8, "later"),
            (9, "flat"),
            (10, "mixed"),
            (11, "scalar_part"),
            (13, "past_end"),
            (14, "vector_order"),
            (15, "odd_if"),
            (16, "branches"),
            (17, "macro"),
            (19, "dropped_input"),
        ]
    );
    assert!(
        errors[0].message.contains("failed to compile"),
        "{}",
        errors[0].message
    );
    assert_eq!(uniforms.inputs().collect::<Vec<_>>(), ["onlyFine"]);
    let lookup = builtins(&[("onlyFine", UniformValue::Float(0.5))]);
    let results = uniforms.evaluate(&lookup, 0.0);
    assert_eq!(results.len(), 1);
    assert_eq!(float(results, "fine"), 4.5);
}

#[test]
fn builtins_of_unexpected_type_yield_zero() {
    let mut uniforms = compile_ok(&[
        "uniform.float.scalar = sunPosition",
        "uniform.vec2.wrong_length = sunPosition",
        "uniform.vec3.right_length = sunPosition",
        "uniform.float.not_a_matrix = gbufferProjection.0.0",
    ]);
    let lookup = builtins(&[
        ("sunPosition", UniformValue::Vec3([1.0, 2.0, 3.0])),
        ("gbufferProjection.0.0", UniformValue::Vec2([1.0, 1.0])),
    ]);
    let results = uniforms.evaluate(&lookup, 0.0);
    assert_eq!(get(results, "scalar"), UniformValue::Float(0.0));
    assert_eq!(get(results, "wrong_length"), UniformValue::Vec2([0.0; 2]));
    assert_eq!(
        get(results, "right_length"),
        UniformValue::Vec3([1.0, 2.0, 3.0])
    );
    assert_eq!(get(results, "not_a_matrix"), UniformValue::Float(0.0));
}

#[test]
fn random_is_reproducible_and_within_range() {
    let lines = [
        "uniform.float.unit = random()",
        "uniform.float.ranged = random(2, 4)",
        "uniform.int.dice = randomInt(1, 7)",
        "uniform.int.any = randomInt()",
    ];
    let mut first = compile_ok(&lines);
    let mut second = compile_ok(&lines);
    let mut seen_unit = Vec::new();
    for _ in 0..50 {
        let a = first.evaluate(&no_builtins, 0.016).to_vec();
        let b = second.evaluate(&no_builtins, 0.016).to_vec();
        assert_eq!(a, b);
        let unit = float(&a, "unit");
        assert!((0.0..1.0).contains(&unit), "unit {unit}");
        let ranged = float(&a, "ranged");
        assert!((2.0..4.0).contains(&ranged), "ranged {ranged}");
        let UniformValue::Int(dice) = get(&a, "dice") else {
            panic!("dice is not an int");
        };
        assert!((1..7).contains(&dice), "dice {dice}");
        seen_unit.push(unit);
    }
    seen_unit.dedup();
    assert!(seen_unit.len() > 1, "random() changes between evaluations");
}

#[test]
fn hostile_frame_times_and_values_never_panic() {
    let mut uniforms = compile_ok(&[
        "uniform.float.held = smooth(target, 1, 1)",
        "uniform.int.nan = 0 / 0",
        "uniform.float.infinite = 1 / 0",
        "uniform.int.rounded = round(1 / 0)",
    ]);
    let at = |value: f32| builtins(&[("target", UniformValue::Float(value))]);
    uniforms.evaluate(&at(1.0), 0.1);
    for frame_time in [f32::NAN, -1.0, f32::INFINITY] {
        let results = uniforms.evaluate(&at(5.0), frame_time);
        assert_eq!(float(results, "held"), 1.0, "frame time {frame_time}");
    }
    let results = uniforms.evaluate(&at(5.0), 0.1);
    assert_eq!(get(results, "nan"), UniformValue::Int(0));
    assert_eq!(float(results, "infinite"), f32::INFINITY);
    assert_eq!(get(results, "rounded"), UniformValue::Int(i32::MAX));
}

#[test]
fn acyclic_forward_references_evaluate_in_dependency_order() {
    let mut uniforms = compile_ok(&[
        "uniform.float.out = later.y + scalar",
        "uniform.float.next = out * 2",
        "variable.vec2.later = vec2(scalar, 2)",
        "variable.float.scalar = input + 1",
    ]);
    for input in [1.0, 4.0] {
        let lookup = builtins(&[("input", UniformValue::Float(input))]);
        let results = uniforms.evaluate(&lookup, 0.0);
        assert_eq!(
            results
                .iter()
                .map(|(name, _)| name.as_str())
                .collect::<Vec<_>>(),
            ["out", "next"]
        );
        assert_eq!(float(results, "out"), input + 3.0);
        assert_eq!(float(results, "next"), (input + 3.0) * 2.0);
    }
}

#[test]
fn dependency_cycles_and_failed_forward_dependencies_are_diagnosed() {
    let (mut uniforms, errors) = compile(&[
        "uniform.float.a = b + 1",
        "variable.float.b = a + 1",
        "uniform.float.dependent = a",
        "uniform.float.broken = later",
        "variable.float.later = unsupported(1)",
        "uniform.float.valid = 5",
    ]);
    assert_eq!(errors.len(), 5);
    assert!(
        errors.iter().any(|error| error.message.contains("cycl")),
        "{errors:?}"
    );
    assert_eq!(
        uniforms.evaluate(&no_builtins, 0.0),
        [("valid".to_owned(), UniformValue::Float(5.0))]
    );
}

#[test]
fn intermediate_float_operations_round_at_every_step() {
    let mut uniforms = compile_ok(&[
        "uniform.float.cancel = (16777216.0 + 1.0) - 16777216.0",
        "uniform.bool.equal = 0.1 + 0.2 == 0.3",
        "uniform.vec2.vector = (vec2(16777216, 0.1) + vec2(1, 0.2)) - vec2(16777216, 0.3)",
        "uniform.float.function = pow(2.0, 24.0) + 1.0 - 16777216.0",
        "uniform.bool.literal = 16777217.0 == 16777216.0",
        "uniform.bool.epsilon = equals(0.1 + 0.2, 0.3, 0.0)",
    ]);
    let results = uniforms.evaluate(&no_builtins, 0.0);
    assert_eq!(float(results, "cancel"), 0.0);
    assert_eq!(get(results, "equal"), UniformValue::Bool(true));
    assert_eq!(get(results, "vector"), UniformValue::Vec2([0.0, 0.0]));
    assert_eq!(float(results, "function"), 0.0);
    assert_eq!(get(results, "literal"), UniformValue::Bool(true));
    assert_eq!(get(results, "epsilon"), UniformValue::Bool(true));
}

#[test]
fn random_int_accepts_a_single_exclusive_bound() {
    let mut uniforms = compile_ok(&[
        "uniform.int.choice = randomInt(8)",
        "uniform.int.large = randomInt(16777219, 16777220)",
    ]);
    for _ in 0..32 {
        let results = uniforms.evaluate(&no_builtins, 0.0);
        let UniformValue::Int(choice) = get(results, "choice") else {
            panic!("expected int")
        };
        assert!((0..8).contains(&choice));
        assert_eq!(get(results, "large"), UniformValue::Int(16777219));
    }
    assert_eq!(compile(&["uniform.float.bad = random(8)"]).1.len(), 1);
}

#[test]
fn smooth_uses_exponential_half_life_without_a_snap_threshold() {
    let mut uniforms = compile_ok(&["uniform.float.level = smooth(target, 1.0, 2.0)"]);
    let at = |value| builtins(&[("target", UniformValue::Float(value))]);
    uniforms.evaluate(&at(0.0), 0.1);
    assert_close(
        float(uniforms.evaluate(&at(1.0), 0.1), "level"),
        0.5,
        "one half life",
    );
    assert_close(
        float(uniforms.evaluate(&at(0.0), 0.2), "level"),
        0.25,
        "fall half life",
    );
    uniforms.restart_smoothing();
    uniforms.evaluate(&at(0.0), 0.1);
    assert_close(
        float(uniforms.evaluate(&at(1.0), 1.0), "level"),
        1.0 - 1.0 / 1024.0,
        "long frame",
    );
    uniforms.restart_smoothing();
    uniforms.evaluate(&at(0.0), 0.1);
    assert_eq!(
        float(uniforms.evaluate(&at(0.0000001), 0.1), "level"),
        0.00000005
    );
}

#[test]
fn smooth_call_sites_are_independent_and_use_only_the_current_frame() {
    let mut uniforms = compile_ok(&[
        "uniform.float.first = smooth(7, target, 1, 1)",
        "uniform.float.second = smooth(7, target * 3, 1, 1)",
        "uniform.float.gated = if(open, smooth(target, 1, 1), -1)",
    ]);
    let at = |target, open| {
        builtins(&[
            ("target", UniformValue::Float(target)),
            ("open", UniformValue::Bool(open)),
        ])
    };
    uniforms.evaluate(&at(0.0, true), 0.1);
    let results = uniforms.evaluate(&at(1.0, true), 0.1);
    assert_eq!(float(results, "first"), 0.5);
    assert_eq!(float(results, "second"), 1.5);
    for _ in 0..20 {
        uniforms.evaluate(&at(1.0, false), 0.1);
    }
    assert_eq!(float(uniforms.evaluate(&at(1.0, true), 0.1), "gated"), 0.75);
}

#[test]
fn integer_inputs_and_random_bounds_keep_all_signed_bits() {
    let mut uniforms = compile_ok(&[
        "uniform.int.direct = large",
        "uniform.int.difference = large - smaller",
        "uniform.bool.ordered = smaller < large",
        "uniform.bool.distinct = smaller != large",
        "uniform.int.bounded = randomInt(smaller, large)",
        "uniform.int.invalid = randomInt(5, 5)",
    ]);
    let at = builtins(&[
        ("large", UniformValue::Int(16777217)),
        ("smaller", UniformValue::Int(16777216)),
    ]);
    let results = uniforms.evaluate(&at, 0.0);
    assert_eq!(get(results, "direct"), UniformValue::Int(16777217));
    assert_eq!(get(results, "difference"), UniformValue::Int(1));
    assert_eq!(get(results, "ordered"), UniformValue::Bool(true));
    assert_eq!(get(results, "distinct"), UniformValue::Bool(true));
    assert_eq!(get(results, "bounded"), UniformValue::Int(16777216));
    assert_eq!(get(results, "invalid"), UniformValue::Int(0));
}

#[test]
fn smooth_only_literal_arguments_select_the_compatibility_id_overload() {
    let mut uniforms = compile_ok(&[
        "uniform.float.negative = smooth(-7, fade, fade)",
        "uniform.float.expression = smooth(2 + 3, fade, fade)",
        "uniform.float.named = smooth(pi, fade, fade)",
        "uniform.float.literal = smooth(7, target, 1, 1)",
    ]);
    let at = |target| {
        builtins(&[
            ("target", UniformValue::Float(target)),
            ("fade", UniformValue::Float(1.0)),
        ])
    };
    let results = uniforms.evaluate(&at(0.0), 0.1);
    assert_eq!(float(results, "negative"), -7.0);
    assert_eq!(float(results, "expression"), 5.0);
    assert_eq!(float(results, "named"), PI);
    assert_eq!(float(results, "literal"), 0.0);
    assert_eq!(float(uniforms.evaluate(&at(1.0), 0.1), "literal"), 0.5);
    assert_eq!(
        compile(&["uniform.float.invalid = smooth(2 + 3, target, 1, 1)"])
            .1
            .len(),
        1
    );
}

#[test]
fn numeric_functions_keep_integer_precision_and_propagate_nan() {
    let mut uniforms = compile_ok(&[
        "uniform.int.minimum = min(large, larger)",
        "uniform.int.absolute = abs(negative)",
        "uniform.float.nan = min(0.0 / 0.0, 1.0)",
    ]);
    let at = builtins(&[
        ("large", UniformValue::Int(16777217)),
        ("larger", UniformValue::Int(16777218)),
        ("negative", UniformValue::Int(-16777217)),
    ]);
    let results = uniforms.evaluate(&at, 0.0);
    assert_eq!(get(results, "minimum"), UniformValue::Int(16777217));
    assert_eq!(get(results, "absolute"), UniformValue::Int(16777217));
    assert!(float(results, "nan").is_nan());
}

#[test]
fn math_functions_round_their_library_result_to_float() {
    let mut uniforms = compile_ok(&["uniform.float.angle = degrees(1.0)"]);
    assert_eq!(
        float(uniforms.evaluate(&no_builtins, 0.0), "angle"),
        57.29578
    );
}

#[test]
fn smooth_zero_decay_uses_the_target_even_without_elapsed_time() {
    let mut uniforms = compile_ok(&["uniform.float.level = smooth(target, fade, fade)"]);
    let at = |target| {
        builtins(&[
            ("target", UniformValue::Float(target)),
            ("fade", UniformValue::Float(f32::INFINITY)),
        ])
    };
    for elapsed in [0.0, 0.1] {
        uniforms.restart_smoothing();
        uniforms.evaluate(&at(0.0), 0.1);
        assert_eq!(float(uniforms.evaluate(&at(1.0), elapsed), "level"), 1.0);
    }
}

#[test]
fn integer_literals_remain_distinct_from_float_literals() {
    let mut uniforms = compile_ok(&[
        "uniform.int.literal = 16777217",
        "uniform.int.sum = 16777216 + 1",
        "uniform.int.overflow = 2147483647 + 1",
        "uniform.bool.distinct = 16777217 != 16777216",
        "uniform.bool.float_equal = 16777217.0 == 16777216.0",
        "uniform.float.division = 7 / 2",
    ]);
    let results = uniforms.evaluate(&no_builtins, 0.0);
    assert_eq!(get(results, "literal"), UniformValue::Int(16777217));
    assert_eq!(get(results, "sum"), UniformValue::Int(16777217));
    assert_eq!(get(results, "overflow"), UniformValue::Int(i32::MIN));
    assert_eq!(get(results, "distinct"), UniformValue::Bool(true));
    assert_eq!(get(results, "float_equal"), UniformValue::Bool(true));
    assert_eq!(float(results, "division"), 3.5);
}

#[test]
fn integer_function_overloads_keep_integer_precision() {
    let mut uniforms = compile_ok(&[
        "uniform.int.clamped = clamp(large, low, high)",
        "uniform.int.edge_value = edge(large, low)",
        "uniform.bool.inside = between(large, low, low)",
        "uniform.int.modulo = fmod(large, ten)",
    ]);
    let at = builtins(&[
        ("large", UniformValue::Int(16777217)),
        ("low", UniformValue::Int(16777216)),
        ("high", UniformValue::Int(16777218)),
        ("ten", UniformValue::Int(10)),
    ]);
    let results = uniforms.evaluate(&at, 0.0);
    assert_eq!(get(results, "clamped"), UniformValue::Int(16777217));
    assert_eq!(get(results, "edge_value"), UniformValue::Int(0));
    assert_eq!(get(results, "inside"), UniformValue::Bool(false));
    assert_eq!(get(results, "modulo"), UniformValue::Int(7));
}

#[test]
fn float_modulo_uses_remainders_without_cancellation() {
    let mut uniforms = compile_ok(&["uniform.float.modulo = fmod(1e20, 3.0)"]);
    assert_eq!(float(uniforms.evaluate(&no_builtins, 0.0), "modulo"), 2.0);
}
