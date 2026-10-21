//! Regression coverage for GLSL parsing and module validation.

use super::*;

/// Compiles a fragment fixture without an include-expanded source map.
fn fragment(glsl: &str) -> Result<CompiledStage, Vec<Diagnostic>> {
    compile(
        StageKind::Fragment,
        &OutputMap {
            glsl,
            origins: &[],
            lines: &LineMap::default(),
        },
    )
}

#[test]
fn forward_declared_functions_validate_with_nested_calls() {
    let glsl = "#version 450
layout(location = 0) out vec4 color;
float first(float value);
float second(float value);
float caller(float value) { return first(value); }
float first(float value) { return second(value) + 1.0; }
float second(float value) { return value * 2.0; }
void main() { color = vec4(caller(3.0)); }
";
    let compiled = fragment(glsl).expect("forward declarations and nested calls validate");
    for (caller, callee) in [("caller", "first"), ("first", "second"), ("main", "caller")] {
        let function = compiled
            .module
            .functions
            .iter()
            .find(|(_, function)| function.name.as_deref() == Some(caller))
            .expect("caller survives compilation")
            .1;
        let targets: Vec<_> = function
            .expressions
            .iter()
            .filter_map(|(_, expression)| match expression {
                naga::Expression::CallResult(target) => {
                    compiled.module.functions[*target].name.as_deref()
                }
                _ => None,
            })
            .collect();
        assert_eq!(targets, [callee], "{caller} retains its call target");
    }
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    )
    .validate(&compiled.module)
    .expect("returned module remains valid");
}

#[test]
fn void_calls_in_nested_control_flow_and_overloads_validate() {
    fragment(
        "#version 450
layout(location = 0) out vec4 color;
int counter = 0;
void first();
void second();
void second(int value);
void first() {
    { second(); }
    if (counter == 0) { second(); } else { second(1); }
    switch (counter) {
        case 0: second(); break;
        default: second(2); break;
    }
    for (int i = 0; i < 1; second()) { second(3); break; }
}
void second() { counter += 1; }
void second(int value) { counter += value; }
void main() { first(); color = vec4(float(counter)); }
",
    )
    .expect("void calls and overloads validate in every nested block");
}

#[test]
fn recursive_function_dependencies_are_rejected() {
    for functions in [
        "void first(); void first() { first(); }",
        "void first(); void second(); void first() { second(); } void second() { first(); }",
    ] {
        let glsl = format!("#version 450\n{functions}\nvoid main() {{ first(); }}");
        let errors = fragment(&glsl).expect_err("recursive calls must fail");
        assert!(errors.iter().any(|error| {
            error.phase == Phase::Validate && error.message.contains("recursive function calls")
        }));
    }
}
