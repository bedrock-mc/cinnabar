use std::{
    sync::atomic::Ordering,
    time::{Duration, Instant},
};

use super::*;
use crate::local_worlds::{
    bds::link_world,
    test_support::{FakeMojang, build_zip, incompressible_zip, scratch},
};

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn part_path(provisioner: &Provisioner) -> PathBuf {
    provisioner
        .downloads_dir()
        .join(format!("bedrock-server-1.26.52.3{PART_SUFFIX}"))
}

fn installed_digest(binary: &Path) -> String {
    let manifest = fs::read(binary.with_file_name("manifest.json")).expect("manifest");
    let manifest: serde_json::Value = serde_json::from_slice(&manifest).expect("manifest json");
    manifest["zip_sha256"].as_str().expect("digest").to_owned()
}

/// An interrupted download continues from its `.zip.part` with a Range request.
#[test]
fn a_partial_archive_resumes() {
    let dir = scratch();
    let archive = incompressible_zip();
    let mojang = FakeMojang::start("1.26.52.3", archive.clone());
    let provisioner = mojang.provisioner(dir.path());
    provisioner.accept_eula().expect("accept");
    let half = archive.len() / 2;
    let part = part_path(&provisioner);
    fs::create_dir_all(part.parent().expect("downloads")).expect("downloads folder");
    fs::write(&part, &archive[..half]).expect("partial download");
    let binary = provisioner.ensure(&Cancel::never()).expect("ensure");
    assert_eq!(mojang.last_range(), Some(format!("bytes={half}-")));
    assert_eq!(
        installed_digest(&binary),
        sha256(&archive),
        "the digest covers the whole archive"
    );
    assert!(!part.exists(), "the part survived the install");
}

/// A resumed prefix that is not the archive fails to unpack once; the next attempt starts clean.
#[test]
fn a_corrupt_part_is_discarded_after_a_failed_unpack() {
    let dir = scratch();
    let archive = incompressible_zip();
    let mojang = FakeMojang::start("1.26.52.3", archive.clone());
    let provisioner = mojang.provisioner(dir.path());
    provisioner.accept_eula().expect("accept");
    let part = part_path(&provisioner);
    fs::create_dir_all(part.parent().expect("downloads")).expect("downloads folder");
    fs::write(&part, vec![0xABu8; archive.len() / 2]).expect("corrupt part");
    assert!(
        provisioner.ensure(&Cancel::never()).is_err(),
        "a corrupt resume installed"
    );
    assert!(!part.exists(), "the corrupt part was kept");
    provisioner.ensure(&Cancel::never()).expect("clean retry");
    assert_eq!(
        mojang.last_range().as_deref(),
        Some(""),
        "the retry must download in full"
    );
}

/// A server that goes silent mid-download fails within the stall timeout, and the bytes so far
/// are resumed next time.
#[test]
fn a_stalled_download_times_out_then_resumes() {
    let dir = scratch();
    let archive = incompressible_zip();
    let mojang = FakeMojang::start("1.26.52.3", archive.clone());
    let mut provisioner = mojang.provisioner(dir.path());
    provisioner.stall_timeout = Duration::from_millis(200);
    provisioner.accept_eula().expect("accept");
    let half = archive.len() / 2;
    mojang.stall_after.store(half, Ordering::SeqCst);
    let began = Instant::now();
    let error = provisioner.ensure(&Cancel::never()).expect_err("stall");
    assert!(
        error.to_string().contains("no data received for too long"),
        "{error}"
    );
    assert!(
        began.elapsed() < Duration::from_secs(5),
        "stall detected late"
    );
    let part = fs::metadata(part_path(&provisioner)).expect("part after the stall");
    assert_eq!(part.len(), half as u64);
    mojang.stall_after.store(0, Ordering::SeqCst);
    provisioner.ensure(&Cancel::never()).expect("resume");
    assert_eq!(mojang.last_range(), Some(format!("bytes={half}-")));
}

/// A new install removes older versions and stale downloads, but never world data linked into them.
#[test]
fn installing_prunes_old_versions_and_keeps_world_data() {
    let dir = scratch();
    let mojang = FakeMojang::start("1.26.52.3", build_zip(&[("bedrock_server", b"bin")]));
    let provisioner = mojang.provisioner(dir.path());
    provisioner.accept_eula().expect("accept");
    let world = dir.path().join("world");
    fs::create_dir_all(&world).expect("world");
    fs::write(world.join("level.dat"), b"keep").expect("world data");
    let old = provisioner.root.join("1.26.40.1");
    link_world(&old.join("worlds").join("linked"), &world).expect("link");
    fs::create_dir_all(old.join("worlds").join("mount-point")).expect("mount point");
    fs::write(old.join("bedrock_server"), b"old").expect("old binary");
    let occupied = provisioner
        .root
        .join("1.26.30.0")
        .join("worlds")
        .join("real");
    fs::create_dir_all(&occupied).expect("occupied");
    fs::write(occupied.join("level.dat"), b"x").expect("occupied data");
    let partial = provisioner.root.join("1.26.41.0.partial");
    fs::create_dir_all(&partial).expect("partial");
    let unrelated = provisioner.root.join("not-a-version");
    fs::create_dir_all(&unrelated).expect("unrelated");
    fs::create_dir_all(provisioner.downloads_dir()).expect("downloads");
    let stale = provisioner
        .downloads_dir()
        .join(format!("bedrock-server-1.26.40.1{PART_SUFFIX}"));
    fs::write(&stale, b"old").expect("stale part");
    provisioner.ensure(&Cancel::never()).expect("ensure");
    for (path, kept) in [
        (old, false),
        (partial, false),
        (stale, false),
        (occupied, true),
        (unrelated, true),
        (provisioner.root.join("eula.json"), true),
        (
            provisioner.root.join("1.26.52.3").join("bedrock_server"),
            true,
        ),
    ] {
        assert_eq!(fs::metadata(&path).is_ok(), kept, "{}", path.display());
    }
    assert_eq!(
        fs::read(world.join("level.dat")).expect("world data"),
        b"keep"
    );
}

#[test]
fn content_ranges_must_continue_the_offset() {
    assert_eq!(parse_content_range("bytes 5-9/10"), Some((5, Some(10))));
    assert_eq!(parse_content_range("bytes 5-9/*"), Some((5, None)));
    assert_eq!(parse_content_range("bytes 10-9/10"), None);
    assert_eq!(parse_content_range("items 5-9/10"), None);
    assert_eq!(
        zip_version("https://x/bin-linux/bedrock-server-1.26.52.3.zip"),
        Some("1.26.52.3")
    );
    assert_eq!(zip_version("https://x/bedrock-server-latest.zip"), None);
    assert!(official_host(
        &Url::parse("https://www.minecraft.net/x").expect("url")
    ));
    assert!(!official_host(
        &Url::parse("http://www.minecraft.net/x").expect("url")
    ));
    assert!(!official_host(
        &Url::parse("https://minecraft.net.evil.example/x").expect("url")
    ));
}
