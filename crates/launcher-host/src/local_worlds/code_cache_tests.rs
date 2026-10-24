//! Drives the generation state machine with plain files standing in for the translator.

use super::*;

const PLENTY: u64 = u64::MAX;
/// The server executable's name, which begins its cache file's.
const SERVER: &str = "bedrock_server";
const SERVER_CACHE: &str = "bedrock_server-1-2.box64";
/// A cache file for one of the server's libraries.
const LIBRARY_CACHE: &str = "libgcc_s.so.1-1-2.box64";

fn new_cache(root: &Path) -> CodeCache {
    CodeCache::new(root.to_path_buf(), SERVER, &["server", "helper"])
}

fn prepare(cache: &CodeCache) -> Use {
    cache.prepare(|_| Ok(PLENTY))
}

/// Stands in for the translator publishing the cache file `name` of `bytes` bytes.
fn publish(used: &Use, name: &str, bytes: u64) {
    fs::File::create(used.dir.join(name))
        .unwrap()
        .set_len(bytes)
        .unwrap();
}

/// A launch that produced and sealed one server cache file; returns the generation folder.
fn sealed(cache: &CodeCache) -> PathBuf {
    let used = prepare(cache);
    assert_eq!(used.mode(), Mode::Produce);
    publish(&used, SERVER_CACHE, 4096);
    let dir = used.dir.clone();
    used.finish(Ending::Clean);
    dir
}

/// The translator cache settings `used` gives a command: `None` for a cleared variable.
fn settings(used: &Use) -> Vec<(String, Option<String>)> {
    let mut command = Command::new("translator");
    used.apply(&mut command);
    let mut settings: Vec<_> = command
        .get_envs()
        .map(|(key, value)| {
            (
                key.to_string_lossy().into_owned(),
                value.map(|value| value.to_string_lossy().into_owned()),
            )
        })
        .collect();
    settings.sort();
    settings
}

fn set(key: &str, value: &str) -> (String, Option<String>) {
    (key.to_owned(), Some(value.to_owned()))
}

fn cleared(key: &str) -> (String, Option<String>) {
    (key.to_owned(), None)
}

#[test]
fn the_first_launch_produces_and_later_launches_only_load() {
    let root = tempfile::tempdir().unwrap();
    let cache = new_cache(root.path());
    let first = prepare(&cache);
    assert_eq!(first.mode(), Mode::Produce);
    let folder = first.dir.to_string_lossy().into_owned();
    assert_eq!(
        settings(&first),
        [
            set(MODE_VAR, "1"),
            set(COMPRESS_VAR, UNCOMPRESSED),
            set(FOLDER_VAR, &folder),
            set(LIMIT_VAR, &LIMIT_MIB.to_string()),
            cleared(MIN_VAR),
        ]
    );
    assert!(first.dir.starts_with(root.path()));
    publish(&first, SERVER_CACHE, 4096);
    first.finish(Ending::Clean);
    for _ in 0..3 {
        let next = prepare(&cache);
        assert_eq!(next.mode(), Mode::Load);
        assert!(settings(&next).contains(&set(MODE_VAR, "2")));
        assert!(settings(&next).contains(&set(FOLDER_VAR, &folder)));
        next.finish(Ending::Clean);
    }
}

#[test]
fn an_off_launch_clears_every_inherited_cache_setting() {
    assert_eq!(
        settings(&CodeCache::off(Off::Unidentified)),
        [
            set(MODE_VAR, "0"),
            cleared(COMPRESS_VAR),
            cleared(FOLDER_VAR),
            cleared(LIMIT_VAR),
            cleared(MIN_VAR),
        ]
    );
}

#[test]
fn a_file_the_translator_rejected_or_storage_changed_is_produced_again() {
    let root = tempfile::tempdir().unwrap();
    let cache = new_cache(root.path());
    let dir = sealed(&cache);
    fs::remove_file(dir.join(SERVER_CACHE)).unwrap();
    assert_eq!(prepare(&cache).mode(), Mode::Produce);

    let root = tempfile::tempdir().unwrap();
    let cache = new_cache(root.path());
    let dir = sealed(&cache);
    fs::write(dir.join(SERVER_CACHE), [0; 10]).unwrap();
    assert_eq!(prepare(&cache).mode(), Mode::Produce);
    assert!(!dir.join(SERVER_CACHE).exists());
}

#[test]
fn a_partial_or_corrupt_seal_is_never_loaded() {
    let root = tempfile::tempdir().unwrap();
    let cache = new_cache(root.path());
    let used = prepare(&cache);
    let dir = used.dir.clone();
    publish(&used, SERVER_CACHE, 4096);
    publish(&used, "libgcc_s.so.1-1-2.box64", 4096);
    used.finish(Ending::Clean);
    // A producer killed mid-exit can publish one file and not the other.
    fs::remove_file(dir.join("libgcc_s.so.1-1-2.box64")).unwrap();
    assert_eq!(prepare(&cache).mode(), Mode::Produce);
    assert!(!dir.join(SERVER_CACHE).exists());

    let root = tempfile::tempdir().unwrap();
    let cache = new_cache(root.path());
    let dir = sealed(&cache);
    fs::write(dir.join(SEALED), b"{not json").unwrap();
    assert_eq!(prepare(&cache).mode(), Mode::Produce);
    assert!(!dir.join(SERVER_CACHE).exists());
}

#[test]
fn producer_attempts_are_bounded_then_the_cache_stays_off() {
    let root = tempfile::tempdir().unwrap();
    let cache = new_cache(root.path());
    for _ in 0..PRODUCER_ATTEMPTS {
        let used = prepare(&cache);
        assert_eq!(used.mode(), Mode::Produce);
        publish(&used, SERVER_CACHE, 4096);
        used.finish(Ending::Failed);
    }
    let off = prepare(&cache);
    assert_eq!(off.mode(), Mode::Off);
    assert!(settings(&off).contains(&set(MODE_VAR, "0")));
    assert!(settings(&off).contains(&cleared(FOLDER_VAR)));
    assert!(!off.dir.join(SERVER_CACHE).exists());
    drop(off);
    assert_eq!(prepare(&cache).mode(), Mode::Off);
}

#[test]
fn an_attempt_that_died_with_the_app_still_counts() {
    let root = tempfile::tempdir().unwrap();
    let cache = new_cache(root.path());
    for _ in 0..PRODUCER_ATTEMPTS {
        // Never finished: the server died with the client.
        assert_eq!(prepare(&cache).mode(), Mode::Produce);
    }
    assert_eq!(prepare(&cache).mode(), Mode::Off);
}

#[test]
fn a_producer_that_never_ran_returns_its_attempt() {
    let root = tempfile::tempdir().unwrap();
    let cache = new_cache(root.path());
    for ending in [Ending::Cancelled, Ending::Unstarted] {
        for _ in 0..PRODUCER_ATTEMPTS + 1 {
            let used = prepare(&cache);
            assert_eq!(used.mode(), Mode::Produce);
            publish(&used, SERVER_CACHE, 4096);
            let dir = used.dir.clone();
            used.finish(ending);
            assert!(!dir.join(SERVER_CACHE).exists());
        }
    }
}

#[test]
fn a_clean_producer_that_wrote_nothing_uses_its_attempt() {
    let root = tempfile::tempdir().unwrap();
    let cache = new_cache(root.path());
    for _ in 0..PRODUCER_ATTEMPTS {
        let used = prepare(&cache);
        assert_eq!(used.mode(), Mode::Produce);
        used.finish(Ending::Clean);
    }
    assert_eq!(prepare(&cache).mode(), Mode::Off);
}

#[test]
fn an_oversized_generation_is_deleted_and_never_sealed() {
    let root = tempfile::tempdir().unwrap();
    let cache = new_cache(root.path());
    let used = prepare(&cache);
    let dir = used.dir.clone();
    publish(&used, SERVER_CACHE, (LIMIT_MIB << 20) + 1);
    used.finish(Ending::Clean);
    assert!(!dir.join(SERVER_CACHE).exists());
    assert_eq!(prepare(&cache).mode(), Mode::Off);
}

#[test]
fn a_failure_while_loading_turns_only_its_generation_off() {
    let root = tempfile::tempdir().unwrap();
    let cache = new_cache(root.path());
    let dir = sealed(&cache);
    for ending in [Ending::Cancelled, Ending::Unstarted, Ending::Clean] {
        let used = prepare(&cache);
        assert_eq!(used.mode(), Mode::Load);
        used.finish(ending);
    }
    prepare(&cache).finish(Ending::Failed);
    assert!(!dir.join(SERVER_CACHE).exists());
    assert_eq!(prepare(&cache).mode(), Mode::Off);

    let updated = CodeCache::new(root.path().to_path_buf(), SERVER, &["server", "new helper"]);
    assert_eq!(prepare(&updated).mode(), Mode::Produce);
}

#[test]
fn a_new_key_removes_the_previous_generation() {
    let root = tempfile::tempdir().unwrap();
    let old = sealed(&new_cache(root.path()));
    fs::write(root.path().join("stray"), b"").unwrap();
    let updated = CodeCache::new(root.path().to_path_buf(), SERVER, &["new server", "helper"]);
    assert_eq!(prepare(&updated).mode(), Mode::Produce);
    assert!(!old.exists());
    assert!(!root.path().join("stray").exists());
}

#[test]
fn every_key_part_selects_its_own_generation() {
    let root = PathBuf::from("root");
    let key = |parts: &[&str]| CodeCache::new(root.clone(), SERVER, parts).key;
    let base = key(&["server", "helper", "glibc", "guest", "3", "0"]);
    for changed in [
        ["other", "helper", "glibc", "guest", "3", "0"],
        ["server", "other", "glibc", "guest", "3", "0"],
        ["server", "helper", "other", "guest", "3", "0"],
        ["server", "helper", "glibc", "other", "3", "0"],
        ["server", "helper", "glibc", "guest", "2", "0"],
        ["server", "helper", "glibc", "guest", "3", "1"],
    ] {
        assert_ne!(key(&changed), base, "{changed:?}");
    }
    // Parts are delimited, so moving a boundary changes the key.
    assert_ne!(key(&["ab", "c"]), key(&["a", "bc"]));
}

#[test]
fn interrupted_translator_writes_are_removed_before_launch() {
    let root = tempfile::tempdir().unwrap();
    let cache = new_cache(root.path());
    let first = prepare(&cache);
    let partial = first.dir.join(format!("{SERVER_CACHE}.tmp.77.0"));
    fs::write(&partial, [0; 64]).unwrap();
    drop(first);
    prepare(&cache);
    assert!(!partial.exists());
}

#[test]
fn too_little_or_unknown_free_space_skips_producing_without_using_an_attempt() {
    let root = tempfile::tempdir().unwrap();
    let cache = new_cache(root.path());
    let short = cache.prepare(|_| Ok(((LIMIT_MIB + RESERVE_MIB) << 20) - 1));
    assert_eq!(short.mode(), Mode::Off);
    drop(short);
    let unknown = cache.prepare(|_| Err(io::Error::from(io::ErrorKind::Unsupported)));
    assert_eq!(unknown.mode(), Mode::Off);
    drop(unknown);
    for _ in 0..PRODUCER_ATTEMPTS - 1 {
        prepare(&cache).finish(Ending::Failed);
    }
    assert_eq!(prepare(&cache).mode(), Mode::Produce);
}

#[test]
fn a_second_launch_while_one_holds_the_cache_runs_with_it_off() {
    let root = tempfile::tempdir().unwrap();
    let cache = new_cache(root.path());
    let producing = prepare(&cache);
    assert_eq!(producing.mode(), Mode::Produce);
    let concurrent = prepare(&cache);
    assert_eq!(concurrent.mode(), Mode::Off);
    assert!(settings(&concurrent).contains(&cleared(FOLDER_VAR)));
    drop(concurrent);
    publish(&producing, SERVER_CACHE, 4096);
    producing.finish(Ending::Clean);
    assert_eq!(prepare(&cache).mode(), Mode::Load);
}

#[test]
fn readiness_follows_the_sealed_generation_without_using_an_attempt_or_the_lease() {
    let root = tempfile::tempdir().unwrap();
    let cache = new_cache(root.path());
    assert!(!cache.ready(), "no generation yet");
    let producing = prepare(&cache);
    publish(&producing, SERVER_CACHE, 4096);
    assert!(!cache.ready(), "published but not sealed");
    let dir = producing.dir.clone();
    producing.finish(Ending::Clean);
    let partial = dir.join(format!("other{PARTIAL_MARK}1"));
    fs::write(&partial, b"partial").unwrap();
    let attempts = fs::read(dir.join(ATTEMPTS)).unwrap();
    for _ in 0..=PRODUCER_ATTEMPTS {
        assert!(cache.ready());
    }
    assert!(partial.exists(), "checking readiness removes nothing");
    assert_eq!(fs::read(dir.join(ATTEMPTS)).unwrap(), attempts);
    let loading = prepare(&cache);
    assert!(cache.ready(), "a held lease does not change readiness");
    assert_eq!(loading.mode(), Mode::Load);
    drop(loading);

    fs::File::options()
        .write(true)
        .open(dir.join(SERVER_CACHE))
        .unwrap()
        .set_len(4095)
        .unwrap();
    assert!(!cache.ready(), "a sealed file changed length");
    assert_eq!(prepare(&cache).mode(), Mode::Produce);
}

#[test]
fn a_disabled_or_rekeyed_generation_is_not_ready() {
    let root = tempfile::tempdir().unwrap();
    let cache = new_cache(root.path());
    sealed(&cache);
    assert!(cache.ready());
    assert!(
        !CodeCache::new(
            root.path().to_path_buf(),
            SERVER,
            &["server", "other helper"]
        )
        .ready()
    );
    let loading = prepare(&cache);
    loading.finish(Ending::Failed);
    assert!(!cache.ready());
}

#[test]
fn an_off_launch_reports_why() {
    let root = tempfile::tempdir().unwrap();
    let cache = new_cache(root.path());
    let needed = (LIMIT_MIB + RESERVE_MIB) << 20;
    let short = cache.prepare(|_| Ok(needed - 1));
    assert_eq!(
        short.off_reason(),
        Some(&Off::LowSpace {
            free: needed - 1,
            needed
        })
    );
    drop(short);
    let unknown = cache.prepare(|_| Err(io::Error::from(io::ErrorKind::Unsupported)));
    assert!(matches!(unknown.off_reason(), Some(Off::Io(_))));
    drop(unknown);
    let producing = prepare(&cache);
    assert_eq!(producing.off_reason(), None);
    assert_eq!(prepare(&cache).off_reason(), Some(&Off::InUse));
    producing.finish(Ending::Failed);
    prepare(&cache).finish(Ending::Failed);
    assert_eq!(
        prepare(&cache).off_reason(),
        Some(&Off::Exhausted(PRODUCER_ATTEMPTS))
    );
}

#[test]
fn a_generation_without_the_server_executables_own_code_is_never_sealed_or_ready() {
    let root = tempfile::tempdir().unwrap();
    let cache = new_cache(root.path());
    let used = prepare(&cache);
    publish(&used, LIBRARY_CACHE, 4096);
    let dir = used.dir.clone();
    used.finish(Ending::Clean);
    assert!(!dir.join(SEALED).exists());
    assert!(!dir.join(LIBRARY_CACHE).exists());
    assert!(!cache.ready());

    // A seal holding only library code, as an earlier build could write, is produced again.
    let root = tempfile::tempdir().unwrap();
    let cache = new_cache(root.path());
    let dir = cache.generation();
    fs::create_dir_all(&dir).unwrap();
    fs::File::create(dir.join(LIBRARY_CACHE))
        .unwrap()
        .set_len(4096)
        .unwrap();
    write_private(
        &dir.join(SEALED),
        &serde_json::to_vec(&Sealed {
            files: vec![(LIBRARY_CACHE.to_owned(), 4096)],
        })
        .unwrap(),
    )
    .unwrap();
    assert!(!cache.ready());
    let other = CodeCache::new(
        root.path().to_path_buf(),
        "other_server",
        &["server", "helper"],
    );
    sealed(&cache);
    assert!(cache.ready());
    assert!(!dir.join(LIBRARY_CACHE).exists());
    assert!(
        !other.ready(),
        "another executable's name is not this server's code"
    );
}

#[test]
fn a_new_setup_attempt_can_repair_a_disabled_cache_without_automatic_retry() {
    let dir = tempfile::tempdir().unwrap();
    let cache = new_cache(&dir.path().join("code"));
    let generation = sealed(&cache);
    let loaded = prepare(&cache);
    loaded.finish(Ending::Failed);
    assert!(!cache.ready());
    let still_off = prepare(&cache);
    assert_eq!(still_off.mode(), Mode::Off);
    assert_eq!(
        cache.retry_disabled().unwrap_err().kind(),
        io::ErrorKind::WouldBlock
    );
    drop(still_off);
    let world = dir.path().join("world.dat");
    fs::write(&world, b"saved user world").unwrap();
    cache.retry_disabled().unwrap();
    assert!(!generation.exists());
    assert_eq!(fs::read(world).unwrap(), b"saved user world");
    let producer = prepare(&cache);
    assert_eq!(producer.mode(), Mode::Produce);
    producer.finish(Ending::Unstarted);
}
