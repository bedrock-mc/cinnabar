use std::{
    collections::BTreeSet,
    path::Path,
    process::{Command, Output},
};

use server_experience::{
    bundle::VerifiedBundle,
    cache::{self, BundleCache},
    crypto::{self, KeyPair},
    manifest::{PackageOffer, Permission, Scope},
    policy::MAX_EXPANDED_BYTES,
    wire::Direction,
};

const MANIFEST_TOML: &str = r#"
id = "benergistics"
package_version = "0.1.0"
permissions = ["ui", "messaging"]

[[channels]]
id = "benergistics.controller"
schema = 1
direction = "to_client"
fields = [{ type = "integer", min = 0, max = 4294967295 }]

[[channels]]
id = "benergistics.ack"
schema = 1
direction = "to_server"
fields = [{ type = "integer", min = 0, max = 4294967295 }]
"#;

const MANIFEST_JSON: &str = r#"{"id":"benergistics","package_version":"0.1.0","permissions":["ui","messaging"],"channels":[{"id":"benergistics.controller","schema":1,"direction":"to_client","fields":[{"type":"integer","min":0,"max":4294967295}]}]}"#;

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
    let key = crypto::Ed25519KeyPair::from_seed_unchecked(&seed).unwrap();
    crypto::hex(key.public_key().as_ref())
}

fn build(manifest: &Path, component: &Path, seed: &Path, out: &Path) -> Output {
    cxb(&[
        "build",
        "--manifest",
        path(manifest),
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
    let mut component = Vec::new();
    for (name, text) in [
        ("manifest.toml", MANIFEST_TOML),
        ("manifest.json", MANIFEST_JSON),
    ] {
        let manifest = dir.path().join(name);
        std::fs::write(&manifest, text).unwrap();
        let out = dir.path().join(format!("{name}.cxb"));
        let output = build(&manifest, &core, &seed, &out);
        assert!(output.status.success(), "{output:?}");
        let bytes = std::fs::read(&out).unwrap();
        assert_eq!(printed(&output, "sha256"), crypto::digest(&bytes));
        assert_eq!(printed(&output, "bytes"), bytes.len().to_string());

        let verified = verify(&bytes, &seed).unwrap();
        assert_eq!(
            verified.manifest.permissions,
            BTreeSet::from([Permission::Ui, Permission::Messaging])
        );
        assert_eq!(verified.manifest.channels[0].id, "benergistics.controller");
        assert_eq!(verified.manifest.channels[0].direction, Direction::ToClient);
        component = verified.component().unwrap().to_vec();
        // Component preamble: version 0x0d, layer 1. A core module would be layer 0.
        assert_eq!(&component[..8], b"\0asm\x0d\0\x01\0");
    }

    // An existing component is stored unchanged rather than encoded twice.
    let input = dir.path().join("component.wasm");
    std::fs::write(&input, &component).unwrap();
    let out = dir.path().join("again.cxb");
    let output = build(&dir.path().join("manifest.toml"), &input, &seed, &out);
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
    let manifest = dir.path().join("manifest.toml");
    // Channels must live in the package's own namespace.
    std::fs::write(
        &manifest,
        MANIFEST_TOML.replace("benergistics.controller", "other.controller"),
    )
    .unwrap();
    let out = dir.path().join("rejected.cxb");
    let output = build(&manifest, &core, &seed, &out);
    assert!(!output.status.success(), "{output:?}");
    assert!(!out.exists());
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
    let manifest = dir.path().join("manifest.toml");
    std::fs::write(&manifest, MANIFEST_TOML).unwrap();
    let assets = dir.path().join("assets");
    std::fs::create_dir_all(assets.join("media")).unwrap();
    std::fs::write(assets.join("media/clip.json"), b"{}").unwrap();
    std::fs::write(assets.join("poster.png"), b"png").unwrap();
    let out = dir.path().join("assets.cxb");
    let mut args = vec![
        "build",
        "--manifest",
        path(&manifest),
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
