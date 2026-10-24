use std::{cell::Cell, fs, process::Command, sync::OnceLock};

use super::{Project, go};
use crate::{
    Channel, Direction, Field, Value,
    client::{self, ClientPart},
    message::Registry,
};

static REGISTRY: OnceLock<Registry> = OnceLock::new();
thread_local! { static INITIALIZED: Cell<usize> = const { Cell::new(0) }; }

struct RetriedClient;
impl ClientPart for RetriedClient {
    /// Requires the registry generated for the Go fixture before authored callbacks start.
    fn registry() -> Option<&'static Registry> {
        REGISTRY.get()
    }
    /// Counts the authored initialization released by the retried confirmation.
    fn init() {
        INITIALIZED.set(INITIALIZED.get() + 1);
    }
}

#[test]
fn refused_go_confirmation_retries_and_resumes_client_callbacks() {
    let project = Project::parse("#![project(id = \"retry\", permissions(messaging))]").unwrap();
    let root = tempfile::tempdir().unwrap();
    let module = go::fixture_module(&project.id);
    fs::write(root.path().join("go.mod"), module).unwrap();
    fs::write(
        root.path().join("messages.go"),
        go::generate(&project).unwrap(),
    )
    .unwrap();
    fs::write(
        root.path().join("registry_test.go"),
        include_str!("registry_test.go.txt"),
    )
    .unwrap();
    for command in ["vet", "test"] {
        let output = Command::new("go")
            .args([command, "-mod=mod", "./..."])
            .env("GOWORK", "off")
            .current_dir(root.path())
            .output()
            .expect("Go is required for generated registry conformance");
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let record: Vec<Value> =
        serde_json::from_slice(&fs::read(root.path().join("confirmation.json")).unwrap()).unwrap();
    let fingerprint: &'static str = Box::leak(project.fingerprint().into_boxed_str());
    let fields: &'static [Field] = Box::leak(
        vec![Field::Text {
            max_bytes: u16::try_from(fingerprint.len()).unwrap(),
        }]
        .into_boxed_slice(),
    );
    let registry = Registry {
        fingerprint,
        channel: Channel {
            id: "retry.registry",
            schema: 1,
            direction: Direction::ToClient,
            fields,
        },
        request: Channel {
            id: "retry.registry_request",
            schema: 1,
            direction: Direction::ToServer,
            fields,
        },
    };
    assert!(REGISTRY.set(registry).is_ok());
    client::initialize_with::<RetriedClient>(|_| Ok(()));
    assert!(!client::active::<RetriedClient>());
    assert_eq!(INITIALIZED.get(), 0);
    client::dispatch::<RetriedClient>(REGISTRY.get().unwrap().channel.id.into(), record.clone());
    assert!(
        client::active::<RetriedClient>(),
        "the retried confirmation must resume authored callbacks"
    );
    assert_eq!(INITIALIZED.get(), 1);
    client::dispatch::<RetriedClient>(REGISTRY.get().unwrap().channel.id.into(), record);
    assert_eq!(
        INITIALIZED.get(),
        1,
        "confirmation must initialize only once"
    );
}
