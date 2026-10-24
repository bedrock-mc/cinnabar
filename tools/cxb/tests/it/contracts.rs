use std::{collections::BTreeSet, path::PathBuf, process::Command};

use serde::Deserialize;
use server_experience::{
    manifest::{Manifest, Offer},
    negotiation::{Accept, Grant, Hello, VerifiedOffer, Wire},
    runtime::Capabilities,
    session::Control,
    wire::{Envelope, Field, Ingress, Scalar},
};

#[derive(Deserialize)]
struct Corpus {
    documents: Vec<DocumentCase>,
    ingress: Vec<TransitionCase>,
}
#[derive(Deserialize)]
struct DocumentCase {
    name: String,
    document: String,
    json: String,
    accepted: bool,
}
#[derive(Deserialize)]
struct TransitionCase {
    name: String,
    steps: Vec<Step>,
}
#[derive(Deserialize)]
struct Step {
    sequence: u64,
    schema: Option<u16>,
    world_epoch: Option<u64>,
    connection: Option<String>,
    generation: Option<u64>,
    accepted: bool,
    delivered: bool,
}

/// Resolves the repository containing both bindings and their fixtures.
fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Reads the common malformed-input and ingress-transition corpus.
fn corpus() -> Corpus {
    serde_json::from_slice(
        &std::fs::read(root().join("crates/server-experience/schema/conformance.json")).unwrap(),
    )
    .unwrap()
}

/// Decodes an existing fixture independently of the generator being checked.
fn fixture<T: serde::de::DeserializeOwned>(name: &str) -> T {
    serde_json::from_slice(
        &std::fs::read(
            root()
                .join("tools/localserver/cinnabarext/testdata")
                .join(name),
        )
        .unwrap(),
    )
    .unwrap()
}

#[test]
fn schema_bindings_are_current() {
    let output = Command::new("python3")
        .arg(root().join("crates/server-experience/schema/generate.py"))
        .arg("--check")
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .output()
        .expect("Python and the Rust and Go formatters are required for schema regeneration");
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn shared_document_cases_match_the_rust_boundary() {
    for case in corpus().documents {
        let accepted = match case.document.as_str() {
            "hello" => serde_json::from_str::<Hello>(&case.json).is_ok(),
            "scalar" => serde_json::from_str::<Scalar>(&case.json).is_ok(),
            "field" => serde_json::from_str::<Field>(&case.json).is_ok(),
            "control" => serde_json::from_str::<Control>(&case.json).is_ok(),
            other => panic!("unknown document {other}"),
        };
        assert_eq!(accepted, case.accepted, "{}", case.name);
    }
}

#[test]
fn shared_ingress_transitions_match_the_client_boundary() {
    for case in corpus().ingress {
        let offer: Offer = fixture("offer_payload.json");
        let accept: Accept = fixture("accept_payload.json");
        let manifest: Manifest = fixture("manifest_payload.json");
        let grant = Grant {
            offer: VerifiedOffer {
                offer: offer.clone(),
                digest: accept.offer_digest,
            },
            session: accept.session,
            connection: accept.hello.connection,
            subclient: accept.hello.subclient,
            expires_unix: accept.expires_unix,
            wire: Wire::v1(),
        };
        let capabilities = Capabilities {
            scope: offer.scope,
            assets: BTreeSet::new(),
            templates: manifest.templates,
            channels: manifest.channels,
            actions: manifest.actions,
            max_message_bytes: grant.wire.limits.max_message_bytes,
        };
        let base: Envelope = fixture("envelope_to_client.json");
        let epoch = base.world_epoch;
        let mut ingress = Ingress::new(0);
        for step in case.steps {
            let mut envelope = base.clone();
            envelope.sequence = step.sequence;
            envelope.schema = step.schema.unwrap_or(envelope.schema);
            envelope.world_epoch = step.world_epoch.unwrap_or(envelope.world_epoch);
            envelope.generation = step.generation.unwrap_or(envelope.generation);
            if let Some(connection) = step.connection {
                envelope.connection = connection;
            }
            let accepted = ingress
                .receive(
                    &serde_json::to_vec(&envelope).unwrap(),
                    0,
                    1,
                    &grant,
                    |id| (id == envelope.bundle).then_some(&capabilities),
                )
                .is_ok();
            assert_eq!(
                accepted, step.accepted,
                "{} sequence {}",
                case.name, step.sequence
            );
            assert_eq!(
                ingress.pop(1, epoch).is_some(),
                step.delivered,
                "{} sequence {}",
                case.name,
                step.sequence
            );
        }
    }
}
