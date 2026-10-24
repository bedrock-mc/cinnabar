use super::drawing_context;
use std::{fs, path::Path, process::Command};

use experience_runtime::{
    callback, load,
    protocol::{Call, Info, Op, Outcome, Request},
};
use mod_host::{helper::Event, server::BundleHost};
use server_experience::{
    bundle::{VerifiedBundle, offline},
    manifest::{PackageOffer, Scope},
    policy::{
        DEVELOPER_ENV, INITIAL_BUNDLE_GENERATION, MAX_EXPANDED_BYTES, MAX_GUEST_MEMORY,
        MAX_MESSAGE_BYTES,
    },
    runtime::{Capabilities, Command as ClientCommand, Principal},
    wire::Scalar,
};

#[test]
fn cosmic_builds_and_runs_both_halves() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/mods/cosmic");
    let output = Command::new(env!("CARGO_BIN_EXE_cinnabar-cxb"))
        .args(["mod", "build", "--project"])
        .arg(&root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let temporary = tempfile::tempdir().unwrap();
    let package = temporary.path().join("cosmic");
    cinnabar_cxb::project::unpack(&root.join("dist/cosmic.cmod"), &package).unwrap();
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "cosmic::cosmic_runtime_child", "--nocapture"])
        .env(DEVELOPER_ENV, "1")
        .env("CINNABAR_TEST_COSMIC_PACKAGE", &package)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn cosmic_runtime_child() {
    let Some(package) = std::env::var_os("CINNABAR_TEST_COSMIC_PACKAGE") else {
        return;
    };
    let package = Path::new(&package);
    let bytes = fs::read(package.join("client/cosmic.cxb")).unwrap();
    let metadata = offline::verify(&bytes).unwrap();
    let scope = Scope {
        permissions: metadata.manifest.permissions.clone(),
        origins: Default::default(),
        memory_bytes: MAX_GUEST_MEMORY,
        gpu_bytes: 0,
        personal_mods: Default::default(),
    };
    let offer = PackageOffer {
        id: metadata.manifest.id.clone(),
        publisher_key: metadata.manifest.publisher_key.clone(),
        digest: metadata.sha256,
        bytes: metadata.bytes,
        url: String::new(),
    };
    let bundle = VerifiedBundle::read(&bytes, &offer, &scope, MAX_EXPANDED_BYTES).unwrap();
    let capabilities = Capabilities {
        scope,
        assets: bundle
            .manifest
            .files
            .iter()
            .map(|file| file.path.clone())
            .collect(),
        templates: bundle.manifest.templates.clone(),
        channels: bundle.manifest.channels.clone(),
        actions: bundle.manifest.actions.clone(),
        max_message_bytes: MAX_MESSAGE_BYTES as u32,
    };
    let owner = Principal {
        session: "cosmic-test".into(),
        bundle: bundle.manifest.id.clone(),
        generation: INITIAL_BUNDLE_GENERATION,
    };
    let mut client = BundleHost::developer_in_process(
        bundle.component().unwrap(),
        owner,
        capabilities,
        0,
        drawing_context(0),
    )
    .unwrap();
    let (engine, _ticker) = load::engine().unwrap();
    let server = load::load(&engine, &package.join("server")).unwrap();
    assert_eq!(server.blocks.len(), 1);
    assert_eq!(server.blocks[0].id, "cosmic:moon_rock");
    assert_eq!(server.items.len(), 1);
    assert_eq!(server.items[0].id, "cosmic:moon_key");
    assert_eq!(
        fs::read(&server.items[0].icon).unwrap(),
        fs::read(package.join("server/assets/moon.png")).unwrap()
    );

    let mut transaction = client.take_transaction();
    // Feed the generated registry request, then the authored ping, through the real server guest.
    for expected in ["cosmic.registry_request", "cosmic.ping"] {
        let [
            ClientCommand::Send {
                channel,
                schema,
                record,
            },
        ] = transaction.commands.as_slice()
        else {
            panic!("expected one client message: {:?}", transaction.commands);
        };
        assert_eq!(channel, expected);
        let player = "00000000-0000-0000-0000-000000000001".to_owned();
        let request = Request::Callback {
            seq: 1,
            info: Info {
                context: None,
                simulation: None,
                world_id: "cosmic-test".into(),
                dimension_id: "overworld".into(),
                tick: 1,
                event_sequence: 1,
            },
            actor: Some(player.clone()),
            world_min_y: -64,
            world_max_y: 319,
            data_budget: 0,
            snapshot: Vec::new(),
            network: None,
            inventory: None,
            call: Call::ClientMessage {
                player: player.clone(),
                channel: channel.clone(),
                schema: *schema,
                payload: serde_json::from_value(serde_json::to_value(record).unwrap()).unwrap(),
                focus: None,
            },
        };
        let outcome = callback::run(&engine, &server, &request);
        let Outcome::Committed { ops } = outcome else {
            panic!("server callback: {outcome:?}");
        };
        let [
            Op::SendClient {
                player: recipient,
                channel,
                schema,
                payload,
                ..
            },
        ] = ops.as_slice()
        else {
            panic!("expected one reply: {ops:?}");
        };
        assert_eq!(recipient, &player);
        assert_eq!(*schema, 1);
        if expected == "cosmic.ping" {
            assert_eq!(channel, "cosmic.pong");
            assert_eq!(
                serde_json::to_value(payload).unwrap(),
                serde_json::to_value([Scalar::Integer(1)]).unwrap()
            );
        }
        transaction = client
            .dispatch(
                &Event::Message {
                    channel: channel.clone(),
                    record: serde_json::to_vec(payload).unwrap(),
                },
                0,
                &drawing_context(0),
            )
            .unwrap()
            .into_transaction();
    }
    assert!(
        matches!(transaction.commands.as_slice(), [ClientCommand::Widget { id, text }] if id == "cosmic.moon" && text == "☾ Cosmic reply 1")
    );
}
