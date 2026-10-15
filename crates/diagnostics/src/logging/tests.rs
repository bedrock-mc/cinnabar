use super::*;
use crate::{
    console::{SessionFiles, flush_before_exit, initialize_log},
    session_log::Retention,
};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

const FIXTURE: &str = "CINNABAR_LOGGING_FIXTURE";

fn targets(category: Category, level: &str) -> Vec<String> {
    category
        .targets()
        .iter()
        .map(|target| format!("{target}={level}"))
        .collect()
}

/// The repository root, two levels above this crate.
fn repository() -> PathBuf {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .and_then(Path::parent)
        .expect("crates/diagnostics sits two levels below the repository root")
        .to_path_buf()
}

#[test]
fn category_directives_expand_to_every_target_of_the_category() {
    assert_eq!(
        category::expand("net=debug").unwrap(),
        targets(Category::Net, "debug")
    );
    assert_eq!(
        category::expand(" ui ").unwrap(),
        targets(Category::Ui, "trace")
    );
    let mut mixed = vec!["wgpu=warn".to_owned()];
    mixed.extend(targets(Category::Render, "info"));
    assert_eq!(category::expand("wgpu=warn, render=INFO").unwrap(), mixed);
    assert_eq!(category::expand("").unwrap(), Vec::<String>::new());
}

#[test]
fn invalid_directives_are_reported_by_name() {
    let invalid = category::expand("net=debug,net=loud").unwrap_err();
    assert_eq!(invalid.directive, "net=loud");
    assert!(category::expand("bogus=[").is_err());
    let (kept, rejected) = category::expand_lossy("core=warn,net=loud,wgpu=error");
    let mut expected = targets(Category::Core, "warn");
    expected.push("wgpu=error".to_owned());
    assert_eq!(kept, expected);
    assert_eq!(rejected.len(), 1);
    assert_eq!(rejected[0].directive, "net=loud");
}

#[test]
fn category_levels_resolve_from_layered_directives() {
    assert_eq!(
        category::category_level(Category::Net, ["info", "protocol=debug"]),
        LevelFilter::DEBUG
    );
    assert_eq!(
        category::category_level(Category::Render, ["warn", "wgpu=error"]),
        LevelFilter::WARN
    );
    // Later directives for the same target override earlier ones.
    assert_eq!(
        category::category_level(Category::Core, ["error", "bridge=trace", "bridge=info"]),
        LevelFilter::INFO
    );
    // A bare target enables every level; span directives do not set the level outside spans.
    assert_eq!(
        category::category_level(Category::Audio, ["warn", "client_audio"]),
        LevelFilter::TRACE
    );
    assert_eq!(
        category::category_level(Category::Session, ["warn", "[join]=trace"]),
        LevelFilter::WARN
    );
    assert_eq!(
        category::category_level(Category::Input, std::iter::empty()),
        LevelFilter::OFF
    );
    let expanded = category::expand("packs=debug").unwrap();
    assert_eq!(
        category::category_level(
            Category::Packs,
            std::iter::once("warn").chain(expanded.iter().map(String::as_str))
        ),
        LevelFilter::DEBUG
    );
}

#[test]
fn category_names_round_trip() {
    for category in Category::ALL {
        assert_eq!(Category::from_name(category.name()), Some(category));
    }
    assert_eq!(Category::from_name("wgpu"), None);
}

/// The registry is the only place a target joins a category, so a crate that starts logging
/// must be added to it before its events can be selected by category.
#[test]
fn every_crate_that_depends_on_tracing_belongs_to_a_category() {
    let root = repository();
    let mut manifests = vec![root.join("app").join("Cargo.toml")];
    for entry in fs::read_dir(root.join("crates")).unwrap() {
        let manifest = entry.unwrap().path().join("Cargo.toml");
        if manifest.is_file() {
            manifests.push(manifest);
        }
    }
    let mut unmapped = Vec::new();
    for path in manifests {
        let manifest: toml::Value = toml::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        let tables = std::iter::once(manifest.get("dependencies")).chain(
            manifest
                .get("target")
                .and_then(toml::Value::as_table)
                .into_iter()
                .flat_map(|targets| targets.values().map(|target| target.get("dependencies"))),
        );
        let logs = tables
            .flatten()
            .any(|dependencies| dependencies.get("tracing").is_some());
        if !logs {
            continue;
        }
        let target = manifest
            .get("lib")
            .and_then(|lib| lib.get("name"))
            .or_else(|| {
                manifest
                    .get("package")
                    .and_then(|package| package.get("name"))
            })
            .and_then(toml::Value::as_str)
            .unwrap()
            .replace('-', "_");
        let scoped = format!("{target}::");
        let mapped = Category::ALL.iter().any(|category| {
            category
                .targets()
                .iter()
                .any(|prefix| target.starts_with(prefix) || prefix.starts_with(&scoped))
        });
        if !mapped {
            unmapped.push(target);
        }
    }
    assert!(
        unmapped.is_empty(),
        "add these crates' targets to a logging::Category: {unmapped:?}"
    );
}

#[test]
fn live_testing_guide_documents_every_category() {
    let guide = fs::read_to_string(repository().join("docs/agents/live-testing.md")).unwrap();
    assert!(guide.contains(markers::LOG) && guide.contains(markers::LOG_JSON));
    for category in Category::ALL {
        assert!(
            guide.contains(&format!("| `{}` |", category.name())),
            "docs/agents/live-testing.md does not document the `{}` category",
            category.name()
        );
    }
}

/// A process that installs logging with no caller layers, as default and setup builds do,
/// before its session log opens, as client startup does.
#[test]
fn logging_child() {
    let Some(dir) = std::env::var_os(FIXTURE) else {
        return;
    };
    let dir = PathBuf::from(dir);
    assert_eq!(filter_generation(), 0);
    install(Options {
        base_filter: "info,wgpu_core=warn".to_owned(),
        omit_frame_marks: true,
        layers: Vec::new(),
        flight_recorder: true,
    })
    .unwrap();
    assert_eq!(filter_generation(), 1);
    let staging = dir.join("staging");
    std::fs::create_dir_all(&staging).unwrap();
    flight::attach(&staging).unwrap();
    assert_eq!(
        install(Options::default()),
        Err(InstallError::AlreadyInstalled)
    );
    assert_eq!(filter_generation(), 1);
    tracing::warn!(target: "cinnabar::lifecycle", "warning before the log opens");
    initialize_log(&SessionFiles {
        dir: &dir,
        stem: "client",
        header: "HEADER",
        json: json_enabled(),
        retention: Retention::DEFAULT,
    })
    .unwrap();
    tracing_log::log::warn!(target: "wgpu_core::device", "log crate warning");
    tracing_log::log::info!(target: "wgpu_core::device", "filtered log record");
    assert_eq!(category_filter(Category::Net), LevelFilter::DEBUG);
    assert_eq!(category_filter(Category::Core), LevelFilter::INFO);
    tracing::debug!(target: "launcher_host::update", "hidden debug");
    set_filter("core=debug").unwrap();
    assert_eq!(filter_generation(), 2);
    assert_eq!(category_filter(Category::Core), LevelFilter::DEBUG);
    assert_eq!(category_filter(Category::Net), LevelFilter::INFO);
    tracing::debug!(target: "launcher_host::update", "visible debug");
    assert!(matches!(
        set_filter("net=loud"),
        Err(FilterError::Invalid(InvalidDirective { directive, .. })) if directive == "net=loud"
    ));
    assert_eq!(filter().unwrap().spec, "core=debug");
    assert_eq!(filter_generation(), 2, "a rejected spec is no change");
    tracing::info!(tracy.frame_mark = true, "frame mark");
    std::thread::Builder::new()
        .name("log-fixture".to_owned())
        .spawn(|| {
            let span = tracing::info_span!("join", server = "local");
            let _entered = span.enter();
            assert!(span_trace().is_some_and(|trace| trace.contains("join")));
            tracing::error!(target: "client_session::join", chunks = 3, "join failed");
        })
        .unwrap()
        .join()
        .unwrap();
    assert!(flush_before_exit());
}

/// With no caller layers, events from before the session log opens, `log`-crate records and
/// runtime filter changes all reach the session file in the documented format.
#[test]
fn installed_logging_writes_the_session_file() {
    let dir = tempfile::tempdir().unwrap();
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "logging::tests::logging_child", "--nocapture"])
        .env(FIXTURE, dir.path())
        .env(markers::LOG, "net=debug,net=loud")
        .env(markers::LOG_JSON, "1")
        .env_remove("RUST_LOG")
        .stdout(Stdio::null())
        .output()
        .unwrap();
    assert!(output.status.success(), "fixture failed: {output:?}");
    let text = fs::read_to_string(dir.path().join("client.log")).unwrap();
    assert!(text.starts_with("HEADER\n"), "{text}");
    let early = text.find("warning before the log opens").expect(&text);
    assert!(text[..early].contains("cinnabar::lifecycle"), "{text}");
    assert!(text.contains(&format!("ignored {} directive `net=loud`", markers::LOG)));
    assert!(text.contains("log crate warning") && text.contains("wgpu_core::device"));
    for hidden in ["filtered log record", "hidden debug", "frame mark"] {
        assert!(
            !text.contains(hidden),
            "`{hidden}` passed the filter: {text}"
        );
    }
    assert!(text.contains("visible debug"), "{text}");
    // The recorder keeps the debug line the session file left out, under its own filter.
    let ring = fs::read(
        dir.path()
            .join("staging")
            .join(crash_report::staging::FLIGHT_RING_FILE),
    )
    .unwrap();
    let flight = crash_report::ring::flight::read(&ring).unwrap().to_log();
    for recorded in [
        "warning before the log opens",
        "log crate warning",
        "hidden debug",
        "visible debug",
        "join failed chunks=3",
    ] {
        assert!(
            flight.contains(recorded),
            "`{recorded}` not recorded: {flight}"
        );
    }
    for left_out in ["filtered log record", "frame mark"] {
        assert!(
            !flight.contains(left_out),
            "`{left_out}` recorded: {flight}"
        );
    }
    let failure = text
        .lines()
        .find(|line| line.contains("join failed"))
        .expect(&text);
    for part in [
        "ERROR",
        "log-fixture",
        "join{server=\"local\"}",
        "client_session::join",
        "chunks=3",
    ] {
        assert!(failure.contains(part), "`{part}` missing from {failure}");
    }
    let json = fs::read_to_string(dir.path().join("client.jsonl")).unwrap();
    let mut records = json
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap());
    assert_eq!(records.next().unwrap()["session_start"], "HEADER");
    let failure = records
        .find(|record| record["message"] == "join failed")
        .expect(&json);
    assert_eq!(failure["level"], "ERROR");
    assert_eq!(failure["thread"], "log-fixture");
    assert_eq!(failure["spans"][0]["fields"]["server"], "local");
}

/// Counts the events a caller layer sees.
static CALLER_EVENTS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

struct CountingLayer;

impl Layer<Registry> for CountingLayer {
    fn on_event(
        &self,
        _event: &tracing::Event<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, Registry>,
    ) {
        CALLER_EVENTS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
}

/// Installs logging with or without one caller layer, per `CINNABAR_LOGGING_LAYERS`.
#[test]
fn caller_layers_child() {
    let Some(dir) = std::env::var_os(FIXTURE) else {
        return;
    };
    let with_layer = std::env::var_os("CINNABAR_LOGGING_LAYERS").is_some_and(|value| value == "1");
    let layers: Vec<CallerLayer> = if with_layer {
        vec![Box::new(CountingLayer)]
    } else {
        Vec::new()
    };
    install(Options {
        base_filter: "info".to_owned(),
        omit_frame_marks: false,
        layers,
        flight_recorder: false,
    })
    .unwrap();
    initialize_log(&SessionFiles {
        dir: Path::new(&dir),
        stem: "client",
        header: "HEADER",
        json: false,
        retention: Retention::DEFAULT,
    })
    .unwrap();
    tracing::info!(target: "cinnabar::lifecycle", "event through the subscriber");
    assert_eq!(
        CALLER_EVENTS.load(std::sync::atomic::Ordering::Relaxed),
        usize::from(with_layer)
    );
    assert!(flush_before_exit());
}

/// An empty caller layer list must not disable callsites: a `tracing` event reaches the file
/// with no caller layers, and with one, which also sees the event.
#[test]
fn a_tracing_event_reaches_the_file_with_and_without_caller_layers() {
    for layers in ["0", "1"] {
        let dir = tempfile::tempdir().unwrap();
        let output = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "logging::tests::caller_layers_child",
                "--nocapture",
            ])
            .env(FIXTURE, dir.path())
            .env("CINNABAR_LOGGING_LAYERS", layers)
            .env_remove(markers::LOG)
            .env_remove("RUST_LOG")
            .stdout(Stdio::null())
            .output()
            .unwrap();
        assert!(output.status.success(), "layers={layers}: {output:?}");
        let text = fs::read_to_string(dir.path().join("client.log")).unwrap();
        assert!(
            text.contains("event through the subscriber"),
            "layers={layers}: {text}"
        );
    }
}
