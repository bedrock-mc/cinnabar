//! The real compiler process owns failures, cancellation and time limits.

use shaderpack::{
    Environment,
    worker::{Request, Running, TIME_LIMIT},
};
use std::{path::Path, time::Duration};

/// Creates a self-contained fragment program and its worker request.
fn fixture(fragment: &str) -> (tempfile::TempDir, Request) {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("shaders")).unwrap();
    std::fs::write(
        root.path().join("shaders/final.vsh"),
        "#version 330\nvoid main() { gl_Position = vec4(0.0); }",
    )
    .unwrap();
    std::fs::write(root.path().join("shaders/final.fsh"), fragment).unwrap();
    let request = Request {
        pack: root.path().into(),
        options: None,
        dimension: "minecraft:overworld".into(),
        environment: Environment::default(),
        purpose: shaderpack::worker::Purpose::Render,
    };
    (root, request)
}

/// Starts the same bounded stdio entry point the shipping client dispatches.
fn start(request: &Request) -> Running {
    Running::start(Path::new(env!("CARGO_BIN_EXE_shaderpack-worker")), request).unwrap()
}

/// Waits for a child result without imposing wall-clock assertions on translation.
fn response(worker: &mut Running) -> Result<shaderpack::worker::Response, String> {
    let mut result = None;
    test_time::eventually("isolated shader compilation", || {
        result = worker.poll(Duration::ZERO, false);
        result.is_some()
    });
    result.unwrap()
}

/// Accepts only the render result a fixture requested.
fn finish(worker: &mut Running) -> Result<shaderpack::worker::Output, String> {
    response(worker).and_then(shaderpack::worker::Response::into_translation)
}

#[test]
fn compiled_modules_and_reflection_survive_the_process_boundary() {
    let (root, request) =
        fixture("#version 330\nout vec4 color; void main() { color = vec4(1.0); }");
    std::fs::write(
        root.path().join("shaders/shaders.properties"),
        "uniform.float.level=smooth(cameraPosition.y, 1, 1)\n",
    )
    .unwrap();
    let compiled = finish(&mut start(&request)).unwrap();
    assert_eq!(compiled.translation.programs.len(), 1);
    assert!(compiled.translation.programs[0].is_ok());
    assert!(
        compiled.translation.programs[0]
            .stages
            .iter()
            .all(|stage| stage.result.is_ok())
    );
    assert_eq!(compiled.pack.source().files().count(), 3);
    assert!(compiled.custom_uniforms.1.is_empty());
    let mut first = compiled.custom_uniforms.0.clone();
    let mut second = compiled.custom_uniforms.0;
    let at = |height| {
        move |name: &str| {
            (name == "cameraPosition").then_some(shaderpack::UniformValue::Vec3([0.0, height, 0.0]))
        }
    };
    first.evaluate(&at(0.0), 0.1);
    assert_eq!(
        first.evaluate(&at(1.0), 0.1),
        [("level".into(), shaderpack::UniformValue::Float(0.5))]
    );
    assert_eq!(
        second.evaluate(&at(9.0), 0.1),
        [("level".into(), shaderpack::UniformValue::Float(9.0))]
    );
    assert_eq!(
        first.evaluate(&at(0.0), 0.0),
        [("level".into(), shaderpack::UniformValue::Float(0.5))]
    );
}

#[test]
fn deeply_nested_glsl_cannot_crash_the_host_or_poison_the_next_load() {
    let fragment = format!(
        "#version 330\nout vec4 color; void main() {{ {} color=vec4(1.0); {} }}",
        "{".repeat(10_000),
        "}".repeat(10_000)
    );
    let (_root, request) = fixture(&fragment);
    let failed = finish(&mut start(&request));
    assert!(
        failed.is_err(),
        "the failing parser must stay inside the child"
    );
    let (_valid, request) =
        fixture("#version 330\nout vec4 color; void main() { color=vec4(1.0); }");
    assert!(finish(&mut start(&request)).unwrap().translation.programs[0].is_ok());
}

#[test]
fn cancellation_and_deadlines_stop_and_reap_the_owned_child() {
    let (_root, request) =
        fixture("#version 330\nout vec4 color; void main() { color=vec4(1.0); }");
    for (elapsed, cancelled, expected) in [
        (Duration::ZERO, true, "cancelled"),
        (TIME_LIMIT, false, "time limit"),
    ] {
        let mut worker = start(&request);
        let Some(Err(error)) = worker.poll(elapsed, cancelled) else {
            panic!("the worker must be stopped");
        };
        assert!(error.contains(expected));
    }
}

#[test]
fn options_reading_uses_the_same_bounded_process_without_compiling_glsl() {
    let (root, mut request) = fixture("not valid GLSL");
    std::fs::write(
        root.path().join("shaders/settings.glsl"),
        "//#define GLOW\n#ifdef GLOW\n#endif\n",
    )
    .unwrap();
    request.purpose = shaderpack::worker::Purpose::Options;
    let mut worker = start(&request);
    let mut result = None;
    test_time::eventually("isolated shader options", || {
        result = worker.poll(Duration::ZERO, false);
        result.is_some()
    });
    let session = result.unwrap().unwrap().into_options().unwrap();
    assert!(session.option("GLOW").is_some());
}

#[test]
fn loading_normalizes_options_without_writing_over_editor_saves() {
    let (root, mut request) =
        fixture("#version 330\nout vec4 color; void main() { color=vec4(1.0); }");
    std::fs::write(
        root.path().join("shaders/settings.glsl"),
        "#define QUALITY 2 // [1 2 3]\n",
    )
    .unwrap();
    let file = root.path().join("options.txt");
    request.options = Some(file.clone());
    for value in ["3", "1"] {
        let saved = format!("QUALITY={value}\nREMOVED=true\n");
        std::fs::write(&file, &saved).unwrap();
        let loaded = finish(&mut start(&request)).unwrap();
        assert_eq!(
            loaded.config.options,
            shaderpack::OptionValues::from([("QUALITY".into(), value.into())])
        );
        assert_eq!(std::fs::read_to_string(&file).unwrap(), saved);
    }
}

#[test]
fn render_defaults_survive_unreadable_saved_settings_but_editor_reports_the_error() {
    let (root, mut request) =
        fixture("#version 330\nout vec4 color; void main() { color=vec4(1.0); }");
    let directory = root.path().join("options.txt");
    std::fs::create_dir(&directory).unwrap();
    request.options = Some(directory);
    let compiled = finish(&mut start(&request)).unwrap();
    assert!(compiled.translation.programs[0].is_ok());
    assert!(compiled.config.options.is_empty());
    assert!(compiled.options_warning.is_some());
    request.purpose = shaderpack::worker::Purpose::Options;
    let Err(reason) = response(&mut start(&request)) else {
        panic!("the editor must report its read error");
    };
    assert!(reason.contains("regular file"));
}
