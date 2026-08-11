use super::{ASSET_FILES, DistError, Options, Platform, input_files, stage};
#[cfg(unix)]
use super::{canonicalize_top_level_alias, reject_existing_symlinks};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT: AtomicU64 = AtomicU64::new(0);

fn temp_root() -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "cinnabar-dist-test-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&path).unwrap();
    path
}

fn fixture(platform: Platform) -> (PathBuf, Options) {
    let root = temp_root();
    let assets = root.join("synthetic-assets");
    fs::create_dir(&assets).unwrap();
    for name in ASSET_FILES {
        fs::write(assets.join(name), format!("synthetic {name}")).unwrap();
    }
    let client = root.join("client.bin");
    let core = root.join("core.bin");
    let physics = root.join("physics.bin");
    let notices = root.join("THIRD_PARTY_NOTICES.md");
    fs::write(&client, b"synthetic client").unwrap();
    fs::write(&core, b"synthetic core").unwrap();
    fs::write(&physics, b"synthetic physics").unwrap();
    fs::write(&notices, b"synthetic notices").unwrap();
    let options = Options {
        platform,
        client,
        core,
        assets,
        physics,
        notices,
        target_triple: "x86_64-synthetic-test".into(),
        git_commit: "0123456789abcdef0123456789abcdef01234567".into(),
        output: root.join("out"),
    };
    (root, options)
}

#[test]
fn platform_destination_table_matches_runtime_layouts() {
    let cases = [
        (Platform::Windows, "resources/assets/vanilla-v1001.mcbea"),
        (Platform::Linux, "share/cinnabar/assets/vanilla-v1001.mcbea"),
        (
            Platform::Macos,
            "Cinnabar.app/Contents/Resources/assets/vanilla-v1001.mcbea",
        ),
    ];
    for (platform, expected) in cases {
        let (root, options) = fixture(platform);
        assert!(
            input_files(&options)
                .iter()
                .any(|(_, destination)| destination == expected)
        );
        fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn synthetic_bundle_has_sorted_hash_manifest_and_local_scope() {
    let (root, options) = fixture(Platform::Linux);
    stage(&options).unwrap();
    let manifest = fs::read_to_string(options.output.join("bundle-manifest.json")).unwrap();
    assert!(manifest.contains("\"distribution_scope\": \"local-development-only\""));
    assert!(manifest.contains("\"sha256\""));
    assert!(manifest.contains("\"target_triple\": \"x86_64-synthetic-test\""));
    assert!(manifest.contains("\"git_commit\": \"0123456789abcdef0123456789abcdef01234567\""));
    assert!(manifest.contains("THIRD_PARTY_NOTICES.md"));
    let paths = manifest
        .lines()
        .filter(|line| line.trim_start().starts_with("\"path\""))
        .collect::<Vec<_>>();
    assert!(paths.windows(2).all(|pair| pair[0] <= pair[1]));
    fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn unix_bundle_binaries_are_executable() {
    use std::os::unix::fs::PermissionsExt;

    for (platform, client, core) in [
        (Platform::Linux, "bin/bedrock-client", "bin/bedrock-core"),
        (
            Platform::Macos,
            "Cinnabar.app/Contents/MacOS/bedrock-client",
            "Cinnabar.app/Contents/MacOS/bedrock-core",
        ),
    ] {
        let (root, options) = fixture(platform);
        stage(&options).unwrap();
        for path in [client, core] {
            let mode = fs::metadata(options.output.join(path))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o111, 0o111);
        }
        fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn refuses_existing_output_and_secret_named_inputs() {
    let (root, mut options) = fixture(Platform::Windows);
    fs::create_dir(&options.output).unwrap();
    assert!(matches!(stage(&options), Err(DistError::OutputExists(_))));
    fs::remove_dir(&options.output).unwrap();
    let secret = root.join("microsoft-token-client.bin");
    fs::write(&secret, b"not a credential, only a rejection witness").unwrap();
    options.client = secret;
    assert!(matches!(stage(&options), Err(DistError::SecretPath(_))));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn refuses_non_regular_inputs() {
    let (root, mut options) = fixture(Platform::Macos);
    options.core = root.join("directory-core");
    fs::create_dir(&options.core).unwrap();
    assert!(matches!(stage(&options), Err(DistError::NotRegular(_))));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn refuses_output_nested_beneath_asset_input() {
    let (root, mut options) = fixture(Platform::Linux);
    options.output = options.assets.join("nested-output");
    assert!(matches!(stage(&options), Err(DistError::UnsafePath(_))));
    fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn top_level_alias_resolution_preserves_nested_link_rejection() {
    let root = temp_root();
    let target = root.join("target");
    fs::create_dir(&target).unwrap();
    let nested = root.join("nested-alias");
    std::os::unix::fs::symlink(&target, &nested).unwrap();

    let raw = nested.join("input.bin");
    let canonical_root = canonicalize_top_level_alias(&root).unwrap();
    let canonical = canonicalize_top_level_alias(&raw).unwrap();
    assert_eq!(canonical, canonical_root.join("nested-alias/input.bin"));
    assert!(matches!(
        reject_existing_symlinks(&raw),
        Err(DistError::NotRegular(_))
    ));
    fs::remove_dir_all(root).unwrap();
}
