use std::{
    fs::{self, File},
    io::Write,
    path::Path,
    time::{Duration, SystemTime},
};

use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

use super::*;

struct Fixture {
    _dir: tempfile::TempDir,
    paths: PackPaths,
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let paths = PackPaths {
            archive: dir.path().join("pack.zip"),
            partial: dir.path().join("pack.zip.partial"),
            cache: dir.path().join(".local/assets/pack/full"),
        };
        Self { _dir: dir, paths }
    }

    fn write_zip(&self, build: impl FnOnce(&mut ZipWriter<File>)) {
        let mut zip = ZipWriter::new(File::create(&self.paths.archive).unwrap());
        build(&mut zip);
        zip.finish().unwrap();
    }

    fn unpack(&self, limits: &UnpackLimits) -> Result<Unpacked, UnpackError> {
        unpack(&self.paths, limits, &|| false)
    }

    /// Nothing may reach the cache's parent but the published cache itself.
    fn parent_entries(&self) -> Vec<String> {
        fs::read_dir(self.paths.cache.parent().unwrap())
            .map(|entries| {
                entries
                    .flatten()
                    .map(|entry| entry.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default()
    }

    fn assert_rejected(&self, limits: &UnpackLimits, expected: &str) {
        let error = self.unpack(limits).unwrap_err().to_string();
        assert!(error.contains(expected), "{error}");
        assert!(!self.paths.cache.exists());
        assert!(
            self.parent_entries().is_empty(),
            "{:?}",
            self.parent_entries()
        );
    }
}

fn deflated() -> SimpleFileOptions {
    SimpleFileOptions::default().compression_method(CompressionMethod::Deflated)
}

fn file(zip: &mut ZipWriter<File>, name: &str, bytes: &[u8]) {
    zip.start_file(name, deflated()).unwrap();
    zip.write_all(bytes).unwrap();
}

fn pack(zip: &mut ZipWriter<File>, root: &str) {
    zip.add_directory(format!("{root}resource_pack/"), deflated())
        .unwrap();
    file(zip, &format!("{root}resource_pack/blocks.json"), b"{}");
    file(zip, &format!("{root}behavior_pack/items/a.json"), b"[1]");
}

#[test]
fn nested_archive_publishes_its_single_top_level_directory() {
    let fixture = Fixture::new();
    fixture.write_zip(|zip| pack(zip, "bedrock-samples-v1/"));
    assert_eq!(
        fixture.unpack(&UnpackLimits::PINNED).unwrap(),
        Unpacked::Published
    );
    let cache = &fixture.paths.cache;
    assert_eq!(
        fs::read(cache.join("resource_pack/blocks.json")).unwrap(),
        b"{}"
    );
    assert_eq!(
        fs::read(cache.join("behavior_pack/items/a.json")).unwrap(),
        b"[1]"
    );
    assert_eq!(fixture.parent_entries(), ["full"]);
    assert_eq!(
        fixture.unpack(&UnpackLimits::PINNED).unwrap(),
        Unpacked::AlreadyPresent
    );
}

#[test]
fn flat_archive_publishes_the_staging_root() {
    let fixture = Fixture::new();
    fixture.write_zip(|zip| pack(zip, ""));
    fixture.unpack(&UnpackLimits::PINNED).unwrap();
    assert!(fixture.paths.is_unpacked());
    assert_eq!(fixture.parent_entries(), ["full"]);
}

#[test]
fn archive_without_the_marker_or_with_two_roots_is_rejected() {
    let fixture = Fixture::new();
    fixture.write_zip(|zip| {
        pack(zip, "a/");
        file(zip, "b/readme.txt", b"x");
    });
    fixture.assert_rejected(&UnpackLimits::PINNED, "exactly one top-level directory");
    fixture.write_zip(|zip| file(zip, "a/resource_pack/other.json", b"x"));
    fixture.assert_rejected(&UnpackLimits::PINNED, "missing resource_pack/blocks.json");
}

#[test]
fn every_limit_rejects_an_archive_just_over_it() {
    let fixture = Fixture::new();
    fixture.write_zip(|zip| {
        pack(zip, "");
        file(zip, "big.bin", &vec![0; 64 << 10]);
    });
    let cases = [
        (
            UnpackLimits {
                max_entries: 3,
                ..UnpackLimits::PINNED
            },
            "archive entry count 4 exceeds the maximum 3",
        ),
        (
            UnpackLimits {
                max_file_bytes: (64 << 10) - 1,
                ..UnpackLimits::PINNED
            },
            "ZIP entry 'big.bin' declared expanded size 65536 exceeds the maximum",
        ),
        (
            UnpackLimits {
                max_total_bytes: 64 << 10,
                ..UnpackLimits::PINNED
            },
            "archive total declared expanded size",
        ),
        (
            UnpackLimits {
                min_ratio_sample: 1,
                max_entry_ratio: 10.0,
                ..UnpackLimits::PINNED
            },
            "ZIP entry 'big.bin' compression ratio 65536:",
        ),
        (
            UnpackLimits {
                min_ratio_sample: 1,
                max_entry_ratio: f64::MAX,
                max_aggregate_ratio: 10.0,
                ..UnpackLimits::PINNED
            },
            "archive aggregate compression ratio",
        ),
    ];
    for (limits, expected) in cases {
        fixture.assert_rejected(&limits, expected);
    }
    fixture.unpack(&UnpackLimits::PINNED).unwrap();
}

#[test]
fn symlink_entries_are_rejected_before_anything_is_written() {
    let fixture = Fixture::new();
    fixture.write_zip(|zip| {
        pack(zip, "");
        zip.add_symlink("resource_pack/escape", "../../..", deflated())
            .unwrap();
    });
    fixture.assert_rejected(&UnpackLimits::PINNED, "link entries are not allowed");
}

#[test]
fn unsafe_entry_names_are_rejected() {
    let cases = [
        ("../evil.txt", "traversal components are not allowed"),
        (
            "resource_pack/../../evil.txt",
            "traversal components are not allowed",
        ),
        ("/etc/evil.txt", "absolute and UNC paths are not allowed"),
        (
            "\\\\server\\share.txt",
            "absolute and UNC paths are not allowed",
        ),
        (
            "C:/evil.txt",
            "drive and alternate-stream paths are not allowed",
        ),
        ("a//b.txt", "empty path components are not allowed"),
        ("a/nul.txt", "reserved filename component 'nul.txt'"),
        ("a/b?.txt", "invalid filename component 'b?.txt'"),
        ("a/trailing.", "invalid filename component 'trailing.'"),
    ];
    for (name, expected) in cases {
        let fixture = Fixture::new();
        fixture.write_zip(|zip| {
            pack(zip, "");
            file(zip, name, b"x");
        });
        fixture.assert_rejected(&UnpackLimits::PINNED, expected);
    }
}

#[test]
fn colliding_and_duplicate_paths_are_rejected() {
    let fixture = Fixture::new();
    fixture.write_zip(|zip| {
        pack(zip, "");
        file(zip, "a/B.txt", b"1");
        file(zip, "a/b.txt", b"2");
    });
    fixture.assert_rejected(
        &UnpackLimits::PINNED,
        "ZIP entry path collision at 'a/b.txt'",
    );
    fixture.write_zip(|zip| {
        pack(zip, "");
        file(zip, "a/x.txt", b"1");
        file(zip, "a\\x.txt", b"2");
    });
    fixture.assert_rejected(&UnpackLimits::PINNED, "duplicate ZIP entry path 'a/x.txt'");
}

#[test]
fn exact_duplicate_central_directory_names_are_rejected() {
    let fixture = Fixture::new();
    fixture.write_zip(|zip| {
        pack(zip, "");
        file(zip, "dupe-a.txt", b"1");
        file(zip, "dupe-b.txt", b"2");
    });
    // The writer refuses duplicates, so rename the second entry in place.
    let mut bytes = fs::read(&fixture.paths.archive).unwrap();
    let mut renamed = 0;
    for start in 0..bytes.len() - 10 {
        if &bytes[start..start + 10] == b"dupe-b.txt" {
            bytes[start + 5] = b'a';
            renamed += 1;
        }
    }
    assert_eq!(renamed, 2, "local and central headers");
    fs::write(&fixture.paths.archive, bytes).unwrap();
    fixture.assert_rejected(
        &UnpackLimits::PINNED,
        "duplicate ZIP entry path 'dupe-a.txt'",
    );
}

#[test]
fn publication_never_replaces_an_existing_directory() {
    let fixture = Fixture::new();
    let parent = fixture.paths.cache.parent().unwrap();
    let (staged, existing) = (parent.join("staged"), parent.join("existing"));
    fs::create_dir_all(staged.join("resource_pack")).unwrap();
    fs::create_dir_all(&existing).unwrap();
    let error = publish::rename_no_replace(&staged, &existing).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("cache directory appeared during extraction")
    );
    fs::write(existing.join("keep"), b"1").unwrap();
    publish::rename_no_replace(&staged, &existing).unwrap_err();
    assert!(staged.join("resource_pack").is_dir());
    assert_eq!(fs::read(existing.join("keep")).unwrap(), b"1");
    assert!(!existing.join("resource_pack").exists());
    let fresh = parent.join("fresh");
    publish::rename_no_replace(&staged, &fresh).unwrap();
    assert!(fresh.join("resource_pack").is_dir() && !staged.exists());
}

#[test]
fn an_incomplete_cache_directory_is_left_alone() {
    let fixture = Fixture::new();
    fixture.write_zip(|zip| pack(zip, ""));
    fs::create_dir_all(fixture.paths.cache.join("partial")).unwrap();
    let error = fixture
        .unpack(&UnpackLimits::PINNED)
        .unwrap_err()
        .to_string();
    assert!(error.contains("cache directory exists without"), "{error}");
    assert!(fixture.paths.cache.join("partial").is_dir());
}

#[test]
fn cancellation_stops_before_publication() {
    let fixture = Fixture::new();
    fixture.write_zip(|zip| pack(zip, ""));
    let error = unpack(&fixture.paths, &UnpackLimits::PINNED, &|| true).unwrap_err();
    assert!(matches!(error, UnpackError::Cancelled));
    assert!(fixture.parent_entries().is_empty());
}

#[test]
fn stale_staging_is_reclaimed_by_age_and_count() {
    let fixture = Fixture::new();
    let cache = &fixture.paths.cache;
    fs::create_dir_all(cache.parent().unwrap()).unwrap();
    for index in 0..6 {
        fs::create_dir(cache.with_file_name(format!("full.extracting-{index}"))).unwrap();
    }
    fs::create_dir(cache.with_file_name("other.extracting-0")).unwrap();
    let now = SystemTime::now();
    assert_eq!(publish::reclaim_stale_staging(cache, now), 2);
    assert_eq!(fixture.parent_entries().len(), 5);
    let later = now + Duration::from_secs(25 * 60 * 60);
    assert_eq!(publish::reclaim_stale_staging(cache, later), 4);
    assert_eq!(fixture.parent_entries(), ["other.extracting-0"]);
}

#[test]
fn manifest_paths_stay_below_the_local_asset_root() {
    let workspace = Path::new("/work");
    let paths = crate::vanilla_source().local_paths(workspace).unwrap();
    assert!(paths.archive.starts_with(workspace.join(DOWNLOAD_DIR)));
    assert!(paths.cache.starts_with(workspace.join(CACHE_ROOT)));
    let manifest = |archive: &str, cache: &str| VanillaSource {
        archive: archive.into(),
        cache_dir: cache.into(),
        ..serde_json::from_str(crate::VANILLA_SOURCE_MANIFEST).unwrap()
    };
    for (source, expected) in [
        (
            manifest("../x.zip", ".local/assets/a"),
            "exactly one nonempty basename",
        ),
        (
            manifest("C:x.zip", ".local/assets/a"),
            "exactly one nonempty basename",
        ),
        (
            manifest("x.zip", ".local/other"),
            "must stay below .local/assets",
        ),
        (
            manifest("x.zip", ".local/assets/a/../../b"),
            "empty or traversal components",
        ),
        (manifest("x.zip", ".local/assets/a\\b"), "forward-slash"),
        (
            manifest("x.zip", ".local/assets/C:escaped/full"),
            "drive, UNC or stream components",
        ),
        (
            manifest("x.zip", ".local/assets/pack/full:stream"),
            "drive, UNC or stream components",
        ),
        (
            manifest("x:y.zip", ".local/assets/a"),
            "exactly one nonempty basename",
        ),
    ] {
        let error = source.local_paths(workspace).unwrap_err().to_string();
        assert!(error.contains(expected), "{error}");
    }
}
