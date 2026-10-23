use std::{fs, path::Path, process::Command};

use cinnabar_cxb::project;
use server_experience::{bundle::offline, policy::DEVELOPER_ENV};

#[test]
fn one_project_builds_a_runtime_accepted_bundle_and_asset_renames_fail() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("space");
    let sdk = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates/experience-sdk");
    let output = Command::new(env!("CARGO_BIN_EXE_cinnabar-cxb"))
        .args(["mod", "new", "space", "--sdk"])
        .arg(&sdk)
        .current_dir(temporary.path())
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert!(project::create(&root, "space", &sdk).is_err());
    let client = root.join("src/client.rs");
    let source = fs::read_to_string(&client).unwrap().replace(
        "let _welcome = crate::shared::assets::ASSET_WELCOME_TXT;",
        "let _welcome = crate::shared::assets::ASSET_WELCOME_TXT;\nexperience_sdk::client::send_message(&crate::shared::Travel { world: crate::shared::HOME.clone(), request: 42 }).unwrap();",
    );
    fs::write(&client, source).unwrap();
    let shared = root.join("src/shared.rs");
    fs::write(&shared, format!("{}\n#[message(id = \"space.notice\", schema = 1, to_client)]\npub struct Notice {{ pub trap: bool }}\n", fs::read_to_string(&shared).unwrap())).unwrap();
    fs::write(&client, format!("{}\nimpl Client {{\n    /// Exercises a recoverable callback failure in the generated component.\n    fn receive(record: Vec<experience_sdk::Value>) {{\n        use experience_sdk::message::Message;\n        if crate::shared::Notice::decode(record).unwrap().trap {{ panic!(\"recoverable callback failure\"); }}\n    }}\n}}\n", fs::read_to_string(&client).unwrap().replace("impl ClientPart for Client {", "impl ClientPart for Client {\n    /// Processes the declared test notification.\n    fn dispatch(_channel: String, record: Vec<experience_sdk::Value>) { Self::receive(record); }\n    /// Proves callbacks run again after a fresh confirmation.\n    fn epoch() { experience_sdk::client::send_message(&crate::shared::Travel { world: crate::shared::HOME.clone(), request: 43 }).unwrap(); }"))).unwrap();
    let private = "SERVER_ONLY_SENTINEL_9b1d38";
    fs::write(root.join("assets/server/secret.txt"), private).unwrap();
    let server_source = root.join("src/server.rs");
    fs::write(&server_source, fs::read_to_string(&server_source).unwrap().replace("impl Experience for Server {}", r#"impl Experience for Server {
    /// Exercises a server-only asset in a callable server entry point.
    fn on_interact(_ctx: &experience_sdk::server::Callback, _player: String, _pos: experience_sdk::server::BlockPos, _face: experience_sdk::server::Face) -> Result<(), experience_sdk::server::GuestError> {
        experience_sdk::server::log(experience_sdk::server::LogLevel::Info, include_str!("../assets/server/secret.txt"));
        Ok(())
    }
}"#)).unwrap();
    fs::write(
        root.join("assets/block.png"),
        include_bytes!("../../../../examples/experiences/terminal-client/assets/terminal.png"),
    )
    .unwrap();
    fs::write(&shared, format!("{}\nuse experience_sdk::message::{{BlockId, ItemId}};\n#[block(name = \"Declared cube\", texture = assets::ASSET_BLOCK_PNG, hardness = 2.0)]\npub const CUBE: BlockId = id!(\"space:cube\");\n#[item(name = \"Declared item\", icon = assets::ASSET_BLOCK_PNG, max_stack = 16)]\npub const TOKEN: ItemId = id!(\"space:token\");\n", fs::read_to_string(&shared).unwrap())).unwrap();
    let archive = project::build(&root, false).unwrap();
    assert_eq!(archive.extension().unwrap(), "cmod");
    let archive_before = fs::read(&archive).unwrap();
    let package = root.join("unpacked");
    assert_eq!(project::unpack(&archive, &package).unwrap(), "space");
    let cxb = package.join("client/space.cxb");
    let bytes = fs::read(&cxb).unwrap();
    let verified = offline::verify(&bytes).unwrap();
    assert_eq!(verified.manifest.id, "space");
    assert!(
        verified
            .manifest
            .files
            .iter()
            .all(|file| !file.path.starts_with("server/"))
    );
    let mut client_archive = zip::ZipArchive::new(std::io::Cursor::new(&bytes)).unwrap();
    use std::io::Read;
    let mut component = Vec::new();
    client_archive
        .by_name(cinnabar_cxb::bundle::COMPONENT_PATH)
        .unwrap()
        .read_to_end(&mut component)
        .unwrap();
    assert!(
        !component
            .windows(private.len())
            .any(|window| window == private.as_bytes())
    );
    let server = fs::read(
        package
            .join("server")
            .join(experience_sdk::artifact::SERVER_WASM),
    )
    .unwrap();
    assert!(
        server
            .windows(private.len())
            .any(|window| window == private.as_bytes())
    );
    let mut pack =
        zip::ZipArchive::new(fs::File::open(package.join("resource.mcpack")).unwrap()).unwrap();
    let atlas: serde_json::Value =
        serde_json::from_reader(pack.by_name("textures/terrain_texture.json").unwrap()).unwrap();
    let texture = atlas["texture_data"]["space.cube.all"]["textures"]
        .as_str()
        .unwrap();
    assert!(pack.by_name(&format!("{texture}.png")).is_ok());
    let world = root.join("world");
    assert_eq!(
        project::install(&archive, &world).unwrap(),
        world.join("mods/space")
    );
    assert!(
        project::install(&archive, &world).is_err(),
        "replacement requires an explicit update"
    );
    let output = Command::new("go")
        .args(["test", "-run", "^TestInstalledModPackage$", "."])
        .env("GOWORK", "off")
        .env("CINNABAR_TEST_CMOD_WORLD", &world)
        .env(
            "CINNABAR_TEST_REGISTRY_FINGERPRINT",
            experience_sdk::project::Project::parse(&fs::read_to_string(&shared).unwrap())
                .unwrap()
                .fingerprint(),
        )
        .env("CINNABAR_CXB_BIN", env!("CARGO_BIN_EXE_cinnabar-cxb"))
        .env("CINNABAR_EXPERIENCE_RUNTIME_BIN", runtime_tool())
        .current_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join("../localserver"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(package.join("messages/messages.go").is_file());
    assert!(package.join("registry.json").is_file());
    let output = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "project::runtime_acceptance_child",
            "--nocapture",
        ])
        .env(DEVELOPER_ENV, "1")
        .env("CINNABAR_TEST_MOD_PACKAGE", &package)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    // Compile the generated Rust codec on the host, and check it against its Go counterpart.
    fs::write(root.join("src/conformance.rs"), RUST_CONFORMANCE).unwrap();
    fs::write(
        root.join("src/lib.rs"),
        format!(
            "{}\n#[cfg(test)] mod conformance;\n",
            fs::read_to_string(root.join("src/lib.rs")).unwrap()
        ),
    )
    .unwrap();
    let output = Command::new("cargo")
        .args(["test", "--lib", "--offline"])
        .current_dir(&root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    fs::write(package.join("messages/messages_test.go"), GO_CONFORMANCE).unwrap();
    let output = Command::new("go")
        .args(["test", "-mod=mod", "./..."])
        .env("GOWORK", "off")
        .current_dir(package.join("messages"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let client_source = fs::read_to_string(&client).unwrap();
    fs::write(
        &client,
        client_source.replace("ASSET_WELCOME_TXT", "ASSET_SERVER_SECRET_TXT"),
    )
    .unwrap();
    assert!(
        project::build(&root, false).is_err(),
        "client code cannot import server-only asset constants"
    );
    fs::write(&client, client_source).unwrap();
    fs::rename(
        root.join("assets/welcome.txt"),
        root.join("assets/renamed.txt"),
    )
    .unwrap();
    assert!(
        project::build(&root, false).is_err(),
        "stale asset references must fail Rust compilation"
    );
    assert_eq!(
        fs::read(archive).unwrap(),
        archive_before,
        "a failed build keeps the last successful package"
    );
}

#[test]
fn runtime_acceptance_child() {
    let Some(package) = std::env::var_os("CINNABAR_TEST_MOD_PACKAGE") else {
        return;
    };
    use mod_host::{helper::Event, server::BundleHost};
    use server_experience::{
        bundle::VerifiedBundle,
        manifest::{PackageOffer, Scope},
        policy::*,
        runtime::{Capabilities, Command, Principal},
        wire::Scalar,
    };
    let package = Path::new(&package);
    let bytes = fs::read(package.join("client/space.cxb")).unwrap();
    let metadata = offline::verify(&bytes).unwrap();
    let offer = PackageOffer {
        id: metadata.manifest.id.clone(),
        publisher_key: metadata.manifest.publisher_key.clone(),
        digest: metadata.sha256,
        bytes: metadata.bytes,
        url: String::new(),
    };
    let scope = Scope {
        permissions: metadata.manifest.permissions.clone(),
        origins: Default::default(),
        memory_bytes: MAX_GUEST_MEMORY,
        gpu_bytes: 0,
        personal_mods: Default::default(),
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
    assert!(
        !capabilities
            .scope
            .permissions
            .contains(&server_experience::manifest::Permission::Input),
        "messaging-only parts must recover without input callbacks"
    );
    let owner = Principal {
        session: "test-session".into(),
        bundle: bundle.manifest.id.clone(),
        generation: INITIAL_BUNDLE_GENERATION,
    };
    let mut host =
        BundleHost::developer_in_process(bundle.component().unwrap(), owner, capabilities, 0)
            .unwrap();
    assert!(
        matches!(host.take_transaction().commands.as_slice(), [Command::Send { channel, schema: 1, .. }] if channel == "space.registry_request"),
        "only the generated registry request runs before confirmation"
    );
    assert!(
        matches!(host.dispatch(&Event::Epoch, 1).unwrap().commands.as_slice(), [Command::Send { channel, .. }] if channel == "space.registry_request")
    );
    assert!(
        matches!(host.dispatch(&Event::Epoch, 2).unwrap().commands.as_slice(), [Command::Send { channel, .. }] if channel == "space.registry_request"),
        "a new world must re-arm pending registry negotiation"
    );
    let schema: serde_json::Value =
        serde_json::from_slice(&fs::read(package.join("registry.json")).unwrap()).unwrap();
    let project_source =
        fs::read_to_string(package.parent().unwrap().join("src/shared.rs")).unwrap();
    let project = experience_sdk::project::Project::parse(&project_source).unwrap();
    assert_eq!(schema, project.document());
    let confirmation = Event::Message {
        channel: "space.registry".into(),
        record: serde_json::to_vec(&vec![Scalar::Text(project.fingerprint())]).unwrap(),
    };
    let output = host.dispatch(&confirmation, 2).unwrap();
    assert!(
        matches!(output.commands.as_slice(), [Command::Send { channel, schema: 1, record }] if channel == "space.travel" && record == &[Scalar::Text("space:home".into()), Scalar::Integer(42)])
    );
    assert!(
        host.dispatch(&confirmation, 2).unwrap().commands.is_empty(),
        "duplicate confirmation never initializes twice"
    );
    let trap = Event::Message {
        channel: "space.notice".into(),
        record: serde_json::to_vec(&vec![Scalar::Bool(true)]).unwrap(),
    };
    assert!(host.dispatch(&trap, 2).is_err());
    let recovering = host.take_transaction();
    assert!(
        matches!(recovering.commands.as_slice(), [Command::Send { channel, .. }] if channel == "space.registry_request"),
        "guest recovery must publish a fresh registry request without another callback"
    );
    let restarted = host.dispatch(&confirmation, 2).unwrap();
    assert!(
        matches!(restarted.commands.as_slice(), [Command::Send { channel, .. }] if channel == "space.travel")
    );
    let active = host.dispatch(&Event::Epoch, 3).unwrap();
    assert!(
        matches!(active.commands.as_slice(), [Command::Send { record, .. }] if record == &[Scalar::Text("space:home".into()), Scalar::Integer(43)])
    );
    let mismatch = Event::Message {
        channel: "space.registry".into(),
        record: serde_json::to_vec(&vec![Scalar::Text("different".into())]).unwrap(),
    };
    assert!(host.dispatch(&mismatch, 3).is_err());
    let refused = host.dispatch(&Event::Epoch, 4).unwrap();
    assert!(refused.commands.iter().all(|command| matches!(command, Command::Send { channel, .. } if channel == "space.registry_request")), "a mismatch must not restore authored callbacks");
}

const RUST_CONFORMANCE: &str = r#"
use experience_sdk::{message::Message, Value};
use crate::shared::{Travel, HOME};
#[test]
fn generated_rust_record_matches_go() {
    let request = Travel { world: HOME.clone(), request: u32::MAX };
    let expected = vec![Value::Text("space:home".into()), Value::Integer(i64::from(u32::MAX))];
    assert_eq!(request.encode().unwrap(), expected);
    assert_eq!(Travel::decode(expected).unwrap(), request);
    assert!(Travel::decode(vec![Value::Text("invalid".into()), Value::Integer(1)]).is_err());
    assert!(Travel::decode(vec![Value::Text("space:home".into()), Value::Integer(-1)]).is_err());
}
"#;

const GO_CONFORMANCE: &str = r#"package messages
import "testing"
// TestGeneratedRustRecord checks the exact record also exercised by the generated Rust codec.
func TestGeneratedRustRecord(t *testing.T) {
    raw := []byte(`[{"type":"text","value":"space:home"},{"type":"integer","value":4294967295}]`)
    var travel Travel
    if err := travel.DecodeRecord(raw); err != nil { t.Fatal(err) }
    if travel.World != Home || travel.Request != 4294967295 { t.Fatal(travel) }
    encoded, err := travel.EncodeRecord(); if err != nil { t.Fatal(err) }
    if string(encoded) != string(raw) { t.Fatalf("%s", encoded) }
}
"#;

/// Locates the helper built by mod packaging through Cargo's artifact metadata.
fn runtime_tool() -> std::path::PathBuf {
    let output = Command::new("cargo")
        .args([
            "build",
            "--locked",
            "-p",
            "experience-runtime",
            "--message-format=json",
        ])
        .current_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
        .stdout
        .split(|byte| *byte == b'\n')
        .filter_map(|line| serde_json::from_slice::<serde_json::Value>(line).ok())
        .find_map(|event| {
            (event["target"]["name"] == "experience-runtime")
                .then(|| event["executable"].as_str().map(std::path::PathBuf::from))
                .flatten()
        })
        .unwrap()
}
