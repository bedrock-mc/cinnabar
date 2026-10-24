//! Array bounds preserve typed constant values and lexical binding lifetimes.

use super::*;

/// Returns the translated fragment module for an original GLSL fixture.
fn fragment(translation: &PackTranslation) -> &naga::Module {
    &program(translation, "composite").stages[1]
        .result
        .as_ref()
        .unwrap()
        .module
}

/// Reads the declared length of a named local array in the compiled shader.
fn array_length(module: &naga::Module, name: &str) -> u32 {
    let variable = module
        .functions
        .iter()
        .flat_map(|(_, function)| function.local_variables.iter())
        .find(|(_, variable)| variable.name.as_deref() == Some(name))
        .expect("fixture array remains a local variable")
        .1;
    let naga::TypeInner::Array {
        size: naga::ArraySize::Constant(size),
        ..
    } = module.types[variable.ty].inner
    else {
        panic!("fixture array has a constant length");
    };
    size.get()
}

#[test]
fn compatibility_array_sizes_do_not_repeat_runtime_constant_initializers() {
    let translation = success(
        VERTEX,
        "#version 450\nint seed = 0; int next() { seed++; return seed; }
        out vec4 color; void main() {
            const int a = next(); const int b = a + a;
            const int n = 2; float values[n]; values[0] = float(b);
            color = vec4(values[0]);
        }",
    );
    let module = fragment(&translation);
    let next = module
        .functions
        .iter()
        .find(|(_, function)| function.name.as_deref() == Some("next"))
        .unwrap()
        .0;
    let calls = module
        .functions
        .iter()
        .flat_map(|(_, function)| function.body.iter())
        .filter(|statement| matches!(statement, naga::Statement::Call { function, .. } if *function == next))
        .count();
    assert_eq!(calls, 1);

    let unresolved = translate(
        VERTEX,
        "#version 450\nuniform int count; out vec4 color; void main() {
            const int n = count; float values[n]; color = vec4(values.length());
        }",
    );
    assert!(!program(&unresolved, "composite").is_ok());
}

#[test]
fn compatibility_array_sizes_keep_implicit_float_conversions() {
    let translation = success(
        VERTEX,
        "#version 450\nout vec4 color; void main() {
            const float n = 2; const float ratio = n / 4;
            float values[int(ratio * 8)]; color = vec4(values.length());
        }",
    );
    assert_eq!(array_length(fragment(&translation), "values"), 4);
}

#[test]
fn compatibility_array_sizes_restore_bindings_after_complete_loops() {
    for body in [
        "{}",
        "if (false) {} else {}",
        "do {} while (false);",
        "for (int i = 0; i < 1; i++) {}",
    ] {
        let translation = success(
            VERTEX,
            &format!(
                "#version 450\nconst int n = 3; out vec4 color;
                void main() {{ for (const int n = 2; false;) {body}
                float values[n]; color = vec4(values.length()); }}"
            ),
        );
        assert_eq!(array_length(fragment(&translation), "values"), 3);
    }
}

#[test]
fn compatibility_array_sizes_follow_nested_declarations_and_initializers() {
    let translation = success(
        VERTEX,
        "#version 450\nout vec4 color; void main() {
            const int n = 2, m = n + 1;
            { const int n = n + 2; float inner[n]; color = vec4(inner.length()); }
            float outer[n], other[m]; color += vec4(outer.length() + other.length());
        }",
    );
    let module = fragment(&translation);
    assert_eq!(array_length(module, "inner"), 4);
    assert_eq!(array_length(module, "outer"), 2);
    assert_eq!(array_length(module, "other"), 3);
}

#[test]
fn compatibility_array_sizes_do_not_fold_shadowing_runtime_bindings() {
    for declarations in [
        "void f(int n) { const int m = n; float values[m]; }",
        "void f() { int n = 4; float values[n]; }",
        "void f() { const int n = int(gl_FragCoord.x); float values[n]; }",
        "struct Data { int n; }; void f() { Data data; float values[data.n]; }",
    ] {
        let translation = translate(
            VERTEX,
            &format!(
                "#version 450\nconst int n = 2; {declarations}
                out vec4 color; void main() {{ color = vec4(1.0); }}"
            ),
        );
        assert!(
            !program(&translation, "composite").is_ok(),
            "{declarations}"
        );
    }
}
