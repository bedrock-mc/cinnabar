use std::io::{Cursor, Write};

use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

use super::*;

/// Builds a real signed archive with one deliberately tiny portable payload.
fn archive(path: &str, content: &[u8], extra: bool) -> (Vec<u8>, manifest::Offer) {
    archive_files(&[(path, content)], extra, CompressionMethod::Stored)
}

/// Signs all assets independently so forged ZIP sizes cannot change the trusted index.
fn archive_files(
    assets: &[(&str, &[u8])],
    extra: bool,
    method: CompressionMethod,
) -> (Vec<u8>, manifest::Offer) {
    archive_files_with_permissions(assets, extra, method, BTreeSet::new(), None)
}

/// Produces signed revisions with the declared permissions covered by the manifest signature.
fn archive_files_with_permissions(
    assets: &[(&str, &[u8])],
    extra: bool,
    method: CompressionMethod,
    permissions: BTreeSet<manifest::Permission>,
    component: Option<&str>,
) -> (Vec<u8>, manifest::Offer) {
    let key = Ed25519KeyPair::from_seed_unchecked(&[7; 32]).unwrap();
    let mut deployment = offer(&key);
    deployment.scope.permissions = permissions.clone();
    let manifest = manifest::Manifest {
        version: policy::WIRE_VERSION,
        api: policy::API_VERSION,
        id: deployment.packages[0].id.clone(),
        publisher_key: deployment.packages[0].publisher_key.clone(),
        package_version: "fixture".into(),
        permissions,
        component: component.map(str::to_owned),
        channels: Vec::new(),
        actions: BTreeSet::new(),
        templates: assets
            .iter()
            .map(|(path, _)| *path)
            .filter(|path| manifest::template_root(path).is_some())
            .map(str::to_owned)
            .collect(),
        files: assets
            .iter()
            .map(|(path, content)| manifest::ContentFile {
                path: (*path).into(),
                bytes: content.len() as u64,
                sha256: crypto::digest(content),
            })
            .collect(),
    };
    let document =
        serde_json::to_vec(&crypto::sign(&manifest, crypto::MANIFEST_DOMAIN, &key).unwrap())
            .unwrap();
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(method);
    writer.start_file(bundle::MANIFEST_PATH, options).unwrap();
    writer.write_all(&document).unwrap();
    for (path, content) in assets {
        writer.start_file(*path, options).unwrap();
        writer.write_all(content).unwrap();
    }
    if extra {
        writer.start_file("unindexed.txt", options).unwrap();
        writer.write_all(b"hidden").unwrap();
    }
    let bytes = writer.finish().unwrap().into_inner();
    deployment.packages[0].bytes = bytes.len() as u64;
    deployment.packages[0].digest = crypto::digest(&bytes);
    (bytes, deployment)
}

#[test]
fn indexed_archive_is_verified_before_any_runtime_exists() {
    let (bytes, deployment) = archive("poster.txt", b"fixture", false);
    let bundle = bundle::VerifiedBundle::read(
        &bytes,
        &deployment.packages[0],
        &deployment.scope,
        policy::MAX_EXPANDED_BYTES,
    )
    .unwrap();
    assert_eq!(bundle.file("poster.txt"), Some(b"fixture".as_slice()));
    assert!(bundle.component().is_none());
    assert_eq!(bundle.expanded_bytes(), 7);
    assert!(bundle.into_component().is_none());
    assert!(
        bundle::VerifiedBundle::read(&bytes, &deployment.packages[0], &deployment.scope, 1,)
            .is_err()
    );
}

#[test]
fn verified_component_transfers_to_the_helper_without_copying() {
    let payload = b"\0asm\x01\0\0\0";
    let (bytes, deployment) = archive_files_with_permissions(
        &[("guest.wasm", payload), ("poster.txt", b"poster")],
        false,
        CompressionMethod::Stored,
        BTreeSet::new(),
        Some("guest.wasm"),
    );
    let mut bundle = bundle::VerifiedBundle::read(
        &bytes,
        &deployment.packages[0],
        &deployment.scope,
        policy::MAX_EXPANDED_BYTES,
    )
    .unwrap();
    let original = bundle.component().unwrap().as_ptr();
    let files = bundle.take_screen_files();
    let component = bundle.take_component().unwrap();
    assert!(files.templates.is_empty() && files.textures.is_empty());
    assert_eq!(component, payload);
    assert_eq!(component.as_ptr(), original);
}

#[test]
fn signed_archives_still_reject_unindexed_and_unsafe_entries() {
    for (path, extra) in [("poster.txt", true), ("../escape.txt", false)] {
        let (bytes, deployment) = archive(path, b"fixture", extra);
        assert!(
            bundle::VerifiedBundle::read(
                &bytes,
                &deployment.packages[0],
                &deployment.scope,
                policy::MAX_EXPANDED_BYTES,
            )
            .is_err()
        );
    }
}

#[test]
fn outer_integrity_and_publisher_pins_are_independent_checks() {
    let (mut bytes, mut deployment) = archive("poster.txt", b"fixture", false);
    deployment.packages[0].publisher_key = crypto::hex(&[1; 32]);
    assert!(
        bundle::VerifiedBundle::read(
            &bytes,
            &deployment.packages[0],
            &deployment.scope,
            policy::MAX_EXPANDED_BYTES,
        )
        .is_err()
    );
    bytes[0] ^= 1;
    assert!(
        bundle::VerifiedBundle::read(
            &bytes,
            &deployment.packages[0],
            &deployment.scope,
            policy::MAX_EXPANDED_BYTES,
        )
        .is_err()
    );
}

/// Updates only the outer digest after corrupting a directory; signed asset sizes stay unchanged.
fn read_mutated(
    bytes: &[u8],
    deployment: &mut manifest::Offer,
    limit: u64,
) -> anyhow::Result<bundle::VerifiedBundle> {
    deployment.packages[0].bytes = bytes.len() as u64;
    deployment.packages[0].digest = crypto::digest(bytes);
    bundle::VerifiedBundle::read(bytes, &deployment.packages[0], &deployment.scope, limit)
}

/// Finds central entries in these fixtures, whose contents do not contain ZIP signatures.
fn central_entries(bytes: &[u8]) -> Vec<usize> {
    bytes
        .windows(4)
        .enumerate()
        .filter_map(|(i, bytes)| (bytes == b"PK\x01\x02").then_some(i))
        .collect()
}

#[test]
fn forged_deflate_sizes_cannot_bypass_the_remaining_multi_asset_budget() {
    let asset = vec![b'x'; 4096];
    let (mut bytes, mut deployment) = archive_files(
        &[("first.bin", &asset), ("other.bin", &asset)],
        false,
        CompressionMethod::Deflated,
    );
    let entries = central_entries(&bytes);
    let manifest_size =
        u32::from_le_bytes(bytes[entries[0] + 24..entries[0] + 28].try_into().unwrap()) as u64;
    assert!(read_mutated(&bytes, &mut deployment, manifest_size + 8191).is_err());
    assert!(read_mutated(&bytes, &mut deployment, manifest_size + 8192).is_ok());
    for at in entries.into_iter().skip(1) {
        bytes[at + 24..at + 28].copy_from_slice(&1u32.to_le_bytes());
    }
    let error = read_mutated(&bytes, &mut deployment, manifest_size + 2).unwrap_err();
    assert!(error.to_string().contains("signed size"), "{error}");
}

#[test]
fn physical_duplicates_are_rejected_before_zip_deduplicates_them() {
    let (mut bytes, mut deployment) = archive_files(
        &[("first.bin", b"a"), ("other.bin", b"b")],
        false,
        CompressionMethod::Stored,
    );
    let at = central_entries(&bytes)[2];
    bytes[at + 46..at + 55].copy_from_slice(b"first.bin");
    assert!(
        read_mutated(&bytes, &mut deployment, policy::MAX_EXPANDED_BYTES)
            .unwrap_err()
            .to_string()
            .contains("duplicate physical")
    );
}

#[test]
fn physical_entry_count_is_bounded_before_library_indexing() {
    let (bytes, mut deployment) = archive("first.bin", b"a", false);
    let entries = central_entries(&bytes);
    let end = bytes.len() - 22;
    let entry = &bytes[entries[1]..end];
    let mut many = bytes[..entries[0]].to_vec();
    for _ in 0..policy::MAX_FILES + 2 {
        many.extend_from_slice(entry);
    }
    let size = many.len() - entries[0];
    let mut footer = bytes[end..].to_vec();
    let count = (policy::MAX_FILES + 2) as u16;
    footer[8..10].copy_from_slice(&count.to_le_bytes());
    footer[10..12].copy_from_slice(&count.to_le_bytes());
    footer[12..16].copy_from_slice(&(size as u32).to_le_bytes());
    many.extend_from_slice(&footer);
    assert!(
        read_mutated(&many, &mut deployment, policy::MAX_EXPANDED_BYTES)
            .unwrap_err()
            .to_string()
            .contains("too many physical")
    );
    let end = many.len() - 22;
    many[end + 8..end + 10].copy_from_slice(&1u16.to_le_bytes());
    many[end + 10..end + 12].copy_from_slice(&1u16.to_le_bytes());
    assert!(
        read_mutated(&many, &mut deployment, policy::MAX_EXPANDED_BYTES)
            .unwrap_err()
            .to_string()
            .contains("unaccounted physical")
    );
}

#[test]
fn malformed_final_aes_directory_never_retries_an_oversized_earlier_zip64_index() {
    let (mut bytes, mut deployment) = archive("first.bin", b"a", false);
    let entries = central_entries(&bytes);
    let original_end = bytes.len() - 22;
    let mut directory = bytes[entries[0]..original_end].to_vec();
    let footer = bytes[original_end..].to_vec();
    bytes.resize(16 * 1024, 0);
    let earlier_start = bytes.len() as u64;
    bytes.extend_from_slice(&directory);
    let zip64_offset = bytes.len() as u64;
    bytes.extend_from_slice(b"PK\x06\x06");
    bytes.extend_from_slice(&44u64.to_le_bytes());
    bytes.extend_from_slice(&45u16.to_le_bytes());
    bytes.extend_from_slice(&45u16.to_le_bytes());
    bytes.extend_from_slice(&[0; 8]);
    let count = (policy::MAX_FILES + 2) as u64;
    bytes.extend_from_slice(&count.to_le_bytes());
    bytes.extend_from_slice(&count.to_le_bytes());
    bytes.extend_from_slice(&(directory.len() as u64).to_le_bytes());
    bytes.extend_from_slice(&earlier_start.to_le_bytes());
    bytes.extend_from_slice(b"PK\x06\x07");
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&zip64_offset.to_le_bytes());
    bytes.extend_from_slice(&1u32.to_le_bytes());
    let mut earlier_footer = footer.clone();
    earlier_footer[8..12].fill(0xff);
    earlier_footer[12..20].fill(0xff);
    bytes.extend_from_slice(&earlier_footer);
    let final_start = bytes.len();
    directory[10..12].copy_from_slice(&99u16.to_le_bytes());
    bytes.extend_from_slice(&directory);
    let mut final_footer = footer;
    final_footer[16..20].copy_from_slice(&(final_start as u32).to_le_bytes());
    bytes.extend_from_slice(&final_footer);
    let error = read_mutated(&bytes, &mut deployment, policy::MAX_EXPANDED_BYTES).unwrap_err();
    assert!(
        error.to_string().contains("unsupported compression"),
        "{error}"
    );
}

#[test]
fn local_and_central_extra_metadata_are_rejected_before_streaming_decode() {
    for central in [false, true] {
        let (mut bytes, mut deployment) = archive("first.bin", b"a", false);
        let offset = if central {
            central_entries(&bytes)[0] + 30
        } else {
            28
        };
        bytes[offset..offset + 2].copy_from_slice(&1u16.to_le_bytes());
        let error = read_mutated(&bytes, &mut deployment, policy::MAX_EXPANDED_BYTES).unwrap_err();
        assert!(error.to_string().contains("extra metadata"), "{error}");
    }
}

/// Creates two independently verified signed archive revisions for a declarative media grant.
fn media_revisions() -> (
    bundle::VerifiedBundle,
    bundle::VerifiedBundle,
    negotiation::Grant,
) {
    let descriptor = media::descriptor::Descriptor {
        id: "cinema".into(),
        timeline: "cinema".into(),
        profile: media::descriptor::Profile::WebmAv1OpusBt709,
        url: "https://example.org/video.webm".into(),
        bytes: 1,
        chunk_bytes: 64 * 1024,
        chunk_hashes: vec![crypto::digest(b"x")],
        sha256: crypto::digest(b"x"),
        width: 2,
        height: 2,
        fps: 1,
        duration_us: 1_000_000,
        audio_channels: 1,
        poster: "poster.txt".into(),
    };
    let descriptor = serde_json::to_vec(&descriptor).unwrap();
    let read = |poster: &[u8]| {
        let (bytes, offer) = archive_files_with_permissions(
            &[("media.json", &descriptor), ("poster.txt", poster)],
            false,
            CompressionMethod::Stored,
            BTreeSet::from([manifest::Permission::Media]),
            None,
        );
        let bundle = bundle::VerifiedBundle::read(
            &bytes,
            &offer.packages[0],
            &offer.scope,
            policy::MAX_EXPANDED_BYTES,
        )
        .unwrap();
        (bundle, offer)
    };
    let (first, offer) = read(b"first poster");
    let (second, _) = read(b"second poster");
    let grant = negotiation::Grant {
        offer: negotiation::VerifiedOffer {
            offer,
            digest: String::new(),
        },
        session: "session".into(),
        connection: "connection".into(),
        subclient: 0,
        expires_unix: 1500,
        wire: negotiation::Wire::v1(),
    };
    (first, second, grant)
}

#[test]
fn media_grant_binds_the_exact_signed_archive_revision() {
    use std::sync::{Arc, atomic::AtomicU64};
    let (first, second, grant) = media_revisions();
    assert_eq!(first.manifest.id, second.manifest.id);
    assert_eq!(first.manifest.publisher_key, second.manifest.publisher_key);
    assert_ne!(first.digest(), second.digest());
    media::service::Player::prepare(
        &first,
        "media.json",
        &grant,
        1,
        1000,
        Arc::new(AtomicU64::new(0)),
        std::path::PathBuf::from("/nonexistent/mod-host"),
    )
    .unwrap();
    let result = media::service::Player::prepare(
        &second,
        "media.json",
        &grant,
        1,
        1000,
        Arc::new(AtomicU64::new(0)),
        std::path::PathBuf::from("/nonexistent/mod-host"),
    );
    assert!(result.err().unwrap().to_string().contains("foreign bundle"));
}

#[test]
fn delayed_probes_survive_polling_and_unsolicited_replies() {
    use std::sync::{Arc, atomic::AtomicU64};
    let (bundle, _, grant) = media_revisions();
    let mut player = media::service::Player::prepare(
        &bundle,
        "media.json",
        &grant,
        1,
        1000,
        Arc::new(AtomicU64::new(0)),
        std::path::PathBuf::from("/nonexistent/mod-host"),
    )
    .unwrap();
    let (id, c0) = player.ping(1_000_000).unwrap();
    assert!(player.ping(2_000_000).is_none());
    assert!(player.clock_reply(id + 1, c0, c0, c0, 2_500_000).is_err());
    assert!(player.clock_reply(id, c0 + 1, c0, c0, 2_500_000).is_err());
    player
        .clock_reply(id, c0, 1_750_000, 1_750_000, 2_500_000)
        .unwrap();
    let (next, sent) = player.ping(2_500_000).unwrap();
    assert!(player.ping(sent + 2_000_000).is_none());
    let replacement = player.ping(sent + 2_000_001).unwrap();
    assert!(
        player
            .clock_reply(next, sent, sent, sent, replacement.1)
            .is_err()
    );
    player
        .clock_reply(
            replacement.0,
            replacement.1,
            replacement.1,
            replacement.1,
            replacement.1 + 1000,
        )
        .unwrap();
}

/// Verifies a bundle of `assets` with the modal permission.
fn screen_bundle(assets: &[(&str, &[u8])]) -> anyhow::Result<bundle::VerifiedBundle> {
    let (bytes, deployment) = archive_files_with_permissions(
        assets,
        false,
        CompressionMethod::Stored,
        BTreeSet::from([manifest::Permission::ModalUi]),
        None,
    );
    bundle::VerifiedBundle::read(
        &bytes,
        &deployment.packages[0],
        &deployment.scope,
        policy::MAX_EXPANDED_BYTES,
    )
}

#[test]
fn screen_files_are_bounded_and_templates_own_the_bundle_namespace() {
    let terminal = br#"{"namespace": "example_cinema", "terminal@common.base_screen": {}}"#;
    let mut bundle = screen_bundle(&[
        ("ui/terminal.json", terminal),
        ("textures/panel.png", b"png"),
    ])
    .unwrap();
    assert_eq!(
        bundle.manifest.templates,
        BTreeSet::from(["ui/terminal.json".to_owned()])
    );
    let files = bundle.take_screen_files();
    assert_eq!(files.namespace, "example_cinema");
    assert_eq!(files.templates["ui/terminal.json"], terminal);
    assert_eq!(
        files.textures,
        [("textures/panel.png".to_owned(), b"png".to_vec())]
    );
    let foreign = br#"{"namespace": "common", "button": {}}"#;
    assert!(screen_bundle(&[("ui/terminal.json", foreign)]).is_err());
    assert!(screen_bundle(&[("ui/terminal.json", b"{not json")]).is_err());
    let large = vec![b' '; policy::MAX_TEMPLATE_BYTES + 1];
    assert!(screen_bundle(&[("ui/terminal.json", &large)]).is_err());
    assert!(screen_bundle(&[("textures/panel.jpg", b"jpg")]).is_err());
    let huge = vec![0; policy::MAX_TEXTURE_BYTES + 1];
    assert!(screen_bundle(&[("textures/panel.png", &huge)]).is_err());
}

// A media descriptor's poster may be a texture, so a bundle that holds `media` keeps its
// textures after the presenter takes its screen files; one without moves them out.
#[test]
fn media_bundles_keep_their_textures_for_posters() {
    let terminal = br#"{"namespace": "example_cinema", "terminal@common.base_screen": {}}"#;
    for (permissions, kept) in [
        (vec![manifest::Permission::ModalUi], false),
        (
            vec![manifest::Permission::ModalUi, manifest::Permission::Media],
            true,
        ),
    ] {
        let (bytes, deployment) = archive_files_with_permissions(
            &[
                ("ui/terminal.json", terminal),
                ("textures/poster.png", b"png"),
            ],
            false,
            CompressionMethod::Stored,
            BTreeSet::from_iter(permissions),
            None,
        );
        let mut bundle = bundle::VerifiedBundle::read(
            &bytes,
            &deployment.packages[0],
            &deployment.scope,
            policy::MAX_EXPANDED_BYTES,
        )
        .unwrap();
        let files = bundle.take_screen_files();
        assert_eq!(files.textures.len(), 1);
        assert!(bundle.file("ui/terminal.json").is_none());
        assert_eq!(bundle.file("textures/poster.png").is_some(), kept);
    }
}
