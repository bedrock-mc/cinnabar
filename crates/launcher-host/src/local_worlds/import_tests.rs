use std::io::{Cursor, Write};

use super::*;
use crate::local_worlds::{
    store::LEVEL_DIR,
    test_support::{build_zip, scratch},
};

/// An NBT tag name, little-endian length first.
fn name(out: &mut Vec<u8>, name: &str) {
    out.extend((name.len() as u16).to_le_bytes());
    out.extend(name.as_bytes());
}

/// A `level.dat`: creative, hard, seed 42, flat, behind unrelated nested tags the reader skips.
fn level_dat() -> Vec<u8> {
    level_dat_with_generator(2)
}

/// Builds level data with the requested Bedrock generator tag.
fn level_dat_with_generator(generator: i32) -> Vec<u8> {
    let mut nbt = vec![TAG_COMPOUND];
    name(&mut nbt, "");
    nbt.push(TAG_COMPOUND);
    name(&mut nbt, "abilities");
    nbt.push(5);
    name(&mut nbt, "flySpeed");
    nbt.extend(0.05f32.to_le_bytes());
    nbt.push(TAG_END);
    nbt.push(TAG_LIST);
    name(&mut nbt, "lastOpenedWithVersion");
    nbt.push(TAG_INT);
    nbt.extend(2i32.to_le_bytes());
    nbt.extend(1i32.to_le_bytes());
    nbt.extend(26i32.to_le_bytes());
    for (tag, value) in [
        ("GameType", 1i32),
        ("Difficulty", 3),
        ("Generator", generator),
    ] {
        nbt.push(TAG_INT);
        name(&mut nbt, tag);
        nbt.extend(value.to_le_bytes());
    }
    nbt.push(TAG_LONG);
    name(&mut nbt, "RandomSeed");
    nbt.extend(42i64.to_le_bytes());
    nbt.push(TAG_STRING);
    name(&mut nbt, "LevelName");
    name(&mut nbt, "Name From Level");
    nbt.push(TAG_END);
    let mut file = 10u32.to_le_bytes().to_vec();
    file.extend((nbt.len() as u32).to_le_bytes());
    file.extend(nbt);
    file
}

struct Fixture {
    _dir: tempfile::TempDir,
    worlds: PathBuf,
    store: Store,
    archives: PathBuf,
}

fn fixture() -> Fixture {
    let dir = scratch();
    let worlds = dir.path().join("worlds");
    let archives = dir.path().join("archives");
    fs::create_dir_all(&archives).expect("archives");
    Fixture {
        store: Store::open(worlds.clone()).expect("store"),
        worlds,
        archives,
        _dir: dir,
    }
}

impl Fixture {
    fn archive(&self, file: &str, entries: &[(&str, &[u8])]) -> PathBuf {
        let path = self.archives.join(file);
        fs::write(&path, build_zip(entries)).expect("archive");
        path
    }

    fn import(&self, archive: &Path) -> Result<World, WorldError> {
        import(
            &self.store,
            archive,
            Backend::Dragonfly,
            &WORLD_LIMITS,
            &|| false,
        )
    }

    /// Everything in the worlds folder, so a refused import is seen to leave nothing.
    fn leftovers(&self) -> Vec<String> {
        fs::read_dir(&self.worlds)
            .expect("worlds")
            .map(|entry| {
                entry
                    .expect("entry")
                    .file_name()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect()
    }
}

/// A world zipped at the root imports into its own folder with its name and settings.
#[test]
fn a_root_level_world_imports_with_its_name_and_settings() {
    let fixture = fixture();
    let level = level_dat();
    let archive = fixture.archive(
        "Castle.mcworld",
        &[
            ("level.dat", &level),
            ("levelname.txt", b"My Castle\n"),
            ("db/CURRENT", b"MANIFEST-000001\n"),
            ("world_icon.jpeg", b"jpeg"),
        ],
    );
    let world = fixture.import(&archive).expect("import");
    assert_eq!(
        (
            world.name.as_str(),
            world.game_mode,
            world.difficulty,
            world.generator,
            world.seed
        ),
        (
            "My Castle",
            GameMode::Creative,
            Difficulty::Hard,
            Generator::Flat,
            42
        )
    );
    let dir = fixture.store.dir(&world.id).expect("dir");
    let level_dir = dir.join(LEVEL_DIR);
    assert_eq!(fs::read(level_dir.join("level.dat")).expect("level"), level);
    assert_eq!(
        fs::read(level_dir.join("db/CURRENT")).expect("db"),
        b"MANIFEST-000001\n"
    );
    assert_eq!(
        fixture.store.thumbnail(&world.id).as_deref(),
        Some(&b"jpeg"[..])
    );
    assert_eq!(
        fixture.store.list().expect("list"),
        std::slice::from_ref(&world)
    );
    assert_eq!(fixture.leftovers(), [world.id]);
}

/// A world inside one top-level folder imports, ignoring Finder's metadata folder; without
/// `levelname.txt` it takes `level.dat`'s name.
#[test]
fn a_world_in_one_top_level_folder_imports() {
    let fixture = fixture();
    let level = level_dat();
    let archive = fixture.archive(
        "nested.mcworld",
        &[
            ("Survival/level.dat", &level),
            ("Survival/db/CURRENT", b"x"),
            ("__MACOSX/Survival/._level.dat", b"fork"),
        ],
    );
    let world = fixture.import(&archive).expect("import");
    assert_eq!(world.name, "Name From Level");
    let level_dir = fixture.store.dir(&world.id).expect("dir").join(LEVEL_DIR);
    assert!(level_dir.join("level.dat").is_file() && level_dir.join("db/CURRENT").is_file());
}

/// An unreadable `level.dat` still imports, named after the file, with default settings.
#[test]
fn a_damaged_level_dat_falls_back_to_the_file_name_and_defaults() {
    let fixture = fixture();
    let archive = fixture.archive("Old Save.mcworld", &[("level.dat", &level_dat()[..20])]);
    let world = import(
        &fixture.store,
        &archive,
        Backend::Bds,
        &WORLD_LIMITS,
        &|| false,
    )
    .expect("import");
    assert_eq!(
        (world.name.as_str(), world.game_mode, world.generator),
        ("Old Save", GameMode::Survival, Generator::Normal)
    );
}

/// Importing the same file twice makes two worlds and never touches the first one's folder.
#[test]
fn a_duplicate_import_gets_its_own_folder() {
    let dir = scratch();
    let manager = crate::local_worlds::open_store(dir.path().join("worlds")).expect("manager");
    let archive = dir.path().join("twice.mcworld");
    fs::write(
        &archive,
        build_zip(&[("level.dat", &level_dat()), ("levelname.txt", b"Twice")]),
    )
    .expect("archive");
    let first = manager.import(&archive, &|| false).expect("first");
    let second = manager.import(&archive, &|| false).expect("second");
    assert_ne!(first.id, second.id);
    assert_eq!(first.name, second.name);
    let mut listed: Vec<String> = manager
        .list()
        .expect("list")
        .into_iter()
        .map(|world| world.id)
        .collect();
    listed.sort();
    let mut expected = vec![first.id, second.id];
    expected.sort();
    assert_eq!(listed, expected);
}

/// A zip whose only entry is a symlink.
fn symlink_zip() -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default();
    zip.start_file("level.dat", options).expect("entry");
    zip.write_all(&level_dat()).expect("body");
    zip.add_symlink("db", "/etc", options).expect("link");
    zip.finish().expect("zip").into_inner()
}

/// Unsafe, oversized, malformed and world-less archives are refused with a message naming the
/// problem, and leave no folder behind.
#[test]
fn refused_archives_leave_nothing_behind() {
    let level = level_dat();
    let small = UnpackLimits {
        max_entries: 3,
        max_total_bytes: 1 << 20,
        ..WORLD_LIMITS
    };
    let big = vec![7u8; (1 << 20) + 1];
    let cases: Vec<(&str, Vec<u8>, &UnpackLimits, &str)> = vec![
        (
            "escape",
            build_zip(&[("level.dat", &level), ("../evil", b"x")]),
            &WORLD_LIMITS,
            "escapes",
        ),
        (
            "absolute",
            build_zip(&[("level.dat", &level), ("/tmp/evil", b"x")]),
            &WORLD_LIMITS,
            "absolute",
        ),
        ("symlink", symlink_zip(), &WORLD_LIMITS, "link"),
        (
            "oversize",
            build_zip(&[("level.dat", &level), ("db/big.ldb", &big)]),
            &small,
            "larger than 1 MiB",
        ),
        (
            "entries",
            build_zip(&[("level.dat", &level), ("a", b"1"), ("b", b"1"), ("c", b"1")]),
            &small,
            "more than 3 entries",
        ),
        (
            "no level.dat",
            build_zip(&[("levelname.txt", b"Nothing"), ("db/CURRENT", b"x")]),
            &WORLD_LIMITS,
            "no level.dat",
        ),
        (
            "two folders",
            build_zip(&[("a/level.dat", &level), ("b/level.dat", &level)]),
            &WORLD_LIMITS,
            "no level.dat",
        ),
        ("not a zip", b"not a zip".to_vec(), &WORLD_LIMITS, "corrupt"),
    ];
    for (case, bytes, limits, message) in cases {
        let fixture = fixture();
        let archive = fixture.archives.join("bad.mcworld");
        fs::write(&archive, bytes).expect("archive");
        let error = import(
            &fixture.store,
            &archive,
            Backend::Dragonfly,
            limits,
            &|| false,
        )
        .expect_err(case);
        assert!(
            matches!(&error, WorldError::Rejected(text) if text.contains(message)),
            "{case}: {error:?}"
        );
        assert!(
            error.public_message().contains(message),
            "{case}: the menu shows why"
        );
        assert!(
            fixture.leftovers().is_empty(),
            "{case}: {:?}",
            fixture.leftovers()
        );
    }
}

/// Another client starting on the same worlds never deletes an import still running here.
#[test]
fn a_starting_client_keeps_a_running_import() {
    let fixture = fixture();
    let staging = fixture.store.staging().expect("staging");
    let partial = staging.path().join("level.dat");
    fs::write(&partial, b"partial").expect("partial");
    Store::open(fixture.worlds.clone())
        .expect("second client")
        .reclaim_abandoned_imports();
    assert!(partial.is_file(), "the running import lost its folder");
    drop(staging);
    assert!(fixture.leftovers().is_empty(), "{:?}", fixture.leftovers());
}

/// Leaves only finished worlds and nothing else in the worlds folder, once reclaimed.
fn assert_only_listed_worlds(fixture: &Fixture) {
    fixture.store.reclaim_abandoned_imports();
    let mut listed: Vec<String> = fixture
        .store
        .list()
        .expect("list")
        .into_iter()
        .map(|world| world.id)
        .collect();
    listed.sort();
    let mut left = fixture.leftovers();
    left.sort();
    assert_eq!(left, listed, "a folder exists that is not a listed world");
}

/// A client killed while publishing an import leaves no folder that is not a listed world.
#[test]
fn a_kill_while_publishing_leaves_no_invisible_world() {
    let fixture = fixture();
    let archive = fixture.archive("Killed.mcworld", &[("level.dat", &level_dat())]);
    // Dies once the world folder has been moved into place, the last step before publishing.
    let worlds = fixture.worlds.clone();
    let kill = move || {
        let moved = fs::read_dir(&worlds)
            .expect("worlds")
            .flatten()
            .any(|entry| entry.path().join(LEVEL_DIR).is_dir());
        assert!(!moved, "killed while publishing");
        false
    };
    let killed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        import(
            &fixture.store,
            &archive,
            Backend::Dragonfly,
            &WORLD_LIMITS,
            &kill,
        )
    }));
    assert!(killed.is_err());
    assert_only_listed_worlds(&fixture);
}

/// A cancelled import publishes nothing and leaves nothing; retrying it makes exactly one world.
#[test]
fn a_cancelled_import_publishes_nothing_and_a_retry_makes_one_world() {
    let fixture = fixture();
    let archive = fixture.archive("Retry.mcworld", &[("level.dat", &level_dat())]);
    let cancelled = import(
        &fixture.store,
        &archive,
        Backend::Dragonfly,
        &WORLD_LIMITS,
        &|| true,
    );
    assert!(
        matches!(cancelled, Err(WorldError::Cancelled)),
        "{cancelled:?}"
    );
    assert_only_listed_worlds(&fixture);
    assert!(fixture.leftovers().is_empty());
    fixture.import(&archive).expect("retry");
    assert_eq!(fixture.store.list().expect("list").len(), 1);
    assert_only_listed_worlds(&fixture);
}

/// The imported record reports oversized level data at once, before any relisting.
#[test]
fn an_import_with_oversized_level_data_reports_options_unavailable() {
    let fixture = fixture();
    let level = crate::local_worlds::test_support::padded_level_dat(
        &level_dat(),
        crate::local_worlds::level_dat::MAX_LEVEL_DAT_BYTES as usize,
    );
    let archive = fixture.archive("Huge.mcworld", &[("level.dat", &level)]);
    let world = fixture.import(&archive).expect("import");
    assert!(world.options_unavailable);
    assert_eq!(
        (
            world.game_mode,
            world.difficulty,
            world.generator,
            world.seed
        ),
        (GameMode::Creative, Difficulty::Hard, Generator::Flat, 42),
        "metadata comes from the bounded prefix"
    );
    let level_dir = fixture.store.dir(&world.id).expect("dir").join(LEVEL_DIR);
    assert!(fs::read(level_dir.join("level.dat")).expect("level") == level);
    let small = fixture.archive("Small.mcworld", &[("level.dat", &level_dat())]);
    assert!(!fixture.import(&small).expect("import").options_unavailable);
}

/// Unsupported imports leave no saved world and can be retried with BDS.
#[test]
fn natural_import_requires_bds_before_publishing() {
    for level in [level_dat_with_generator(1), level_dat()[..20].to_vec()] {
        let fixture = fixture();
        let archive = fixture.archive("Natural.mcworld", &[("level.dat", &level)]);
        let original = fs::read(&archive).expect("archive");
        let error = fixture
            .import(&archive)
            .expect_err("natural terrain requires BDS");
        assert!(matches!(error, WorldError::Invalid(_)), "{error}");
        assert!(fixture.store.list().expect("list").is_empty());
        assert!(fixture.leftovers().is_empty());
        assert_eq!(fs::read(&archive).expect("archive"), original);
        let world = import(
            &fixture.store,
            &archive,
            Backend::Bds,
            &WORLD_LIMITS,
            &|| false,
        )
        .expect("retry with BDS");
        assert_eq!(world.backend, Backend::Bds);
        assert_eq!(world.generator, Generator::Normal);
    }
}
