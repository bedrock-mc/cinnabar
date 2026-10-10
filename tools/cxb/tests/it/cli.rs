use std::{
    collections::BTreeSet,
    path::Path,
    process::{Command, Output},
};

use {
    ring::signature::KeyPair,
    server_experience::{
        bundle::VerifiedBundle,
        cache::{self, BundleCache},
        crypto,
        manifest::{PackageOffer, Permission, Scope},
        policy::MAX_EXPANDED_BYTES,
        wire::Direction,
    },
};

/// The M0 client part (spec § Sub-project 2) with a screen, in an Experience's `experience.toml`
/// beside the server keys and index, which are the runtime's.
const EXPERIENCE: &str = r#"
id = "benergistics"
version = "0.1.0"
api = "0.3"
data-schema = 1

[client]
permissions = ["ui", "messaging"]
templates = ["ui/terminal.json"]

[[client.channels]]
id = "benergistics.controller"
schema = 1
direction = "to_client"
fields = [{ type = "integer", min = 0, max = 4294967295 }]

[[client.channels]]
id = "benergistics.ack"
schema = 1
direction = "to_server"
fields = [{ type = "integer", min = 0, max = 4294967295 }]

[files]
"server.wasm" = "0000000000000000000000000000000000000000000000000000000000000000"
"#;

/// The screen `EXPERIENCE` lists, read beside it.
const TEMPLATE: (&str, &str) = (
    "ui/terminal.json",
    r#"{"namespace":"benergistics","terminal":{"type":"panel"}}"#,
);

/// The smallest core module: the builder must componentize it like `mod-host pack`.
const CORE_MODULE: &[u8] = b"\0asm\x01\0\0\0";

fn cxb(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_cinnabar-cxb"))
        .args(args)
        .output()
        .unwrap()
}

fn path(path: &Path) -> &str {
    path.to_str().unwrap()
}

/// Reads `key=value` lines the CLI prints for scripts.
fn printed<'a>(output: &'a Output, key: &str) -> &'a str {
    std::str::from_utf8(&output.stdout)
        .unwrap()
        .lines()
        .find_map(|line| line.strip_prefix(key)?.strip_prefix('='))
        .unwrap_or_else(|| panic!("no {key}= in {output:?}"))
}

fn publisher_key(seed: &Path) -> String {
    let seed = crypto::fixed_hex::<32>(std::fs::read_to_string(seed).unwrap().trim()).unwrap();
    let key = ring::signature::Ed25519KeyPair::from_seed_unchecked(&seed).unwrap();
    crypto::hex(key.public_key().as_ref())
}

/// Writes `text` as `dir/experience.toml` with the template beside it, and returns its path.
fn write_experience(dir: &Path, text: &str) -> std::path::PathBuf {
    let (template, bytes) = TEMPLATE;
    std::fs::create_dir_all(dir.join("ui")).unwrap();
    std::fs::write(dir.join(template), bytes).unwrap();
    let path = dir.join("experience.toml");
    std::fs::write(&path, text).unwrap();
    path
}

fn build(experience: &Path, component: &Path, seed: &Path, out: &Path) -> Output {
    cxb(&[
        "build",
        "--experience",
        path(experience),
        "--component",
        path(component),
        "--publisher-seed",
        path(seed),
        "--out",
        path(out),
    ])
}

/// Runs the client's own checks with an offer that pins this publisher and these bytes.
fn verify(bytes: &[u8], seed: &Path) -> anyhow::Result<VerifiedBundle> {
    let offer = PackageOffer {
        id: "benergistics".into(),
        publisher_key: publisher_key(seed),
        digest: crypto::digest(bytes),
        bytes: bytes.len() as u64,
        url: "https://cxb.example/benergistics.cxb".into(),
    };
    let scope = Scope {
        permissions: BTreeSet::from([Permission::Ui, Permission::Messaging]),
        origins: BTreeSet::new(),
        memory_bytes: 0,
        gpu_bytes: 0,
    };
    VerifiedBundle::read(bytes, &offer, &scope, MAX_EXPANDED_BYTES)
}

#[test]
fn keygen_writes_a_hex_seed_and_refuses_to_overwrite() {
    let dir = tempfile::tempdir().unwrap();
    let seed = dir.path().join("publisher.seed");
    let output = cxb(&["keygen", path(&seed)]);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(printed(&output, "public_key"), publisher_key(&seed));

    let text = std::fs::read_to_string(&seed).unwrap();
    let again = cxb(&["keygen", path(&seed)]);
    assert!(!again.status.success(), "{again:?}");
    assert_eq!(std::fs::read_to_string(&seed).unwrap(), text);
}

#[test]
fn build_output_passes_the_client_bundle_verifier() {
    let dir = tempfile::tempdir().unwrap();
    let seed = dir.path().join("publisher.seed");
    assert!(cxb(&["keygen", path(&seed)]).status.success());
    let core = dir.path().join("core.wasm");
    std::fs::write(&core, CORE_MODULE).unwrap();
    let experience = write_experience(dir.path(), EXPERIENCE);
    let out = dir.path().join("benergistics.cxb");
    let output = build(&experience, &core, &seed, &out);
    assert!(output.status.success(), "{output:?}");
    let bytes = std::fs::read(&out).unwrap();
    assert_eq!(printed(&output, "sha256"), crypto::digest(&bytes));
    assert_eq!(printed(&output, "bytes"), bytes.len().to_string());

    let verified = verify(&bytes, &seed).unwrap();
    assert_eq!(verified.manifest.package_version, "0.1.0");
    assert_eq!(
        verified.manifest.permissions,
        BTreeSet::from([Permission::Ui, Permission::Messaging])
    );
    assert_eq!(verified.manifest.channels[0].id, "benergistics.controller");
    assert_eq!(verified.manifest.channels[0].direction, Direction::ToClient);
    let (template, template_bytes) = TEMPLATE;
    assert_eq!(
        verified.manifest.templates,
        BTreeSet::from([template.to_owned()])
    );
    assert!(
        verified
            .manifest
            .files
            .iter()
            .any(|file| file.path == template
                && file.sha256 == crypto::digest(template_bytes.as_bytes()))
    );
    let component = verified.component().unwrap().to_vec();
    // Component preamble: version 0x0d, layer 1. A core module would be layer 0.
    assert_eq!(&component[..8], b"\0asm\x0d\0\x01\0");

    // An existing component is stored unchanged rather than encoded twice.
    let input = dir.path().join("component.wasm");
    std::fs::write(&input, &component).unwrap();
    let out = dir.path().join("again.cxb");
    let output = build(&experience, &input, &seed, &out);
    assert!(output.status.success(), "{output:?}");
    let verified = verify(&std::fs::read(&out).unwrap(), &seed).unwrap();
    assert_eq!(verified.component().unwrap(), component);
}

#[test]
fn build_rejects_a_manifest_the_client_would_refuse() {
    let dir = tempfile::tempdir().unwrap();
    let seed = dir.path().join("publisher.seed");
    assert!(cxb(&["keygen", path(&seed)]).status.success());
    let core = dir.path().join("core.wasm");
    std::fs::write(&core, CORE_MODULE).unwrap();
    // Channels must live in the package's own namespace.
    let experience = write_experience(
        dir.path(),
        &EXPERIENCE.replace("benergistics.controller", "other.controller"),
    );
    let out = dir.path().join("rejected.cxb");
    let output = build(&experience, &core, &seed, &out);
    assert!(!output.status.success(), "{output:?}");
    assert!(!out.exists());
}

/// A bundle needs `[client]`, and takes nothing from it that it does not sign.
#[test]
fn build_requires_a_client_table_of_known_keys() {
    let dir = tempfile::tempdir().unwrap();
    let seed = dir.path().join("publisher.seed");
    assert!(cxb(&["keygen", path(&seed)]).status.success());
    let core = dir.path().join("core.wasm");
    std::fs::write(&core, CORE_MODULE).unwrap();
    let (server_keys, _) = EXPERIENCE.split_once("[client]").unwrap();
    let misspelled = EXPERIENCE.replace("permissions", "permission");
    for (text, reason) in [
        (server_keys, "[client]"),
        (misspelled.as_str(), "`permission`"),
    ] {
        let experience = write_experience(dir.path(), text);
        let out = dir.path().join("rejected.cxb");
        let output = build(&experience, &core, &seed, &out);
        assert!(!output.status.success(), "{output:?}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains(reason), "{stderr}");
        assert!(!out.exists());
    }
}

#[test]
fn seed_cache_publishes_where_the_client_cache_reads() {
    let dir = tempfile::tempdir().unwrap();
    let bundle = dir.path().join("bundle.cxb");
    std::fs::write(&bundle, b"bundle bytes").unwrap();
    let user_data = dir.path().join("user-data");
    let output = cxb(&[
        "seed-cache",
        "--cxb",
        path(&bundle),
        "--user-data",
        path(&user_data),
    ]);
    assert!(output.status.success(), "{output:?}");
    let digest = crypto::digest(b"bundle bytes");
    assert_eq!(printed(&output, "sha256"), digest);
    let objects = cache::objects_dir(&user_data);
    assert_eq!(Path::new(printed(&output, "objects")), objects);
    let cache = BundleCache::open(&objects).unwrap();
    assert_eq!(cache.read(&digest).unwrap().unwrap(), b"bundle bytes");
}

#[test]
fn build_assets_are_indexed_under_their_relative_paths() {
    let dir = tempfile::tempdir().unwrap();
    let seed = dir.path().join("publisher.seed");
    assert!(cxb(&["keygen", path(&seed)]).status.success());
    let core = dir.path().join("core.wasm");
    std::fs::write(&core, CORE_MODULE).unwrap();
    let experience = write_experience(dir.path(), EXPERIENCE);
    let assets = dir.path().join("assets");
    std::fs::create_dir_all(assets.join("media")).unwrap();
    std::fs::write(assets.join("media/clip.json"), b"{}").unwrap();
    std::fs::write(assets.join("poster.png"), b"png").unwrap();
    let out = dir.path().join("assets.cxb");
    let mut args = vec![
        "build",
        "--experience",
        path(&experience),
        "--component",
        path(&core),
        "--publisher-seed",
        path(&seed),
        "--out",
        path(&out),
        "--assets",
        path(&assets),
    ];
    let output = cxb(&args);
    assert!(output.status.success(), "{output:?}");
    let verified = verify(&std::fs::read(&out).unwrap(), &seed).unwrap();
    assert_eq!(verified.file("media/clip.json").unwrap(), b"{}");
    assert_eq!(verified.file("poster.png").unwrap(), b"png");

    // An asset that would shadow the component is refused.
    std::fs::write(assets.join("component.wasm"), b"x").unwrap();
    let rejected = dir.path().join("rejected.cxb");
    args[8] = path(&rejected);
    assert!(!cxb(&args).status.success());
    assert!(!rejected.exists());
}
