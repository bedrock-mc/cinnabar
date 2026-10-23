//! Drives setup preparation through a fake translator and server that record each launch's cache
//! mode, working folder and visible files, and write translated code on a producer's exit.
#![cfg(unix)]

use std::{sync::Arc, thread};

use super::*;
use crate::local_worlds::{
    bds::BdsRunner,
    cancel_pair,
    code_cache::PRODUCER_ATTEMPTS,
    process::Runner,
    provision::{Platform, RuntimeInfo},
    test_support::{FAKE_BDS, FakeMojang, installed_provisioner, scratch, spec, write_script},
    translated::{CODE_CACHE_DIR, TranslationPin},
    unpack::INSTALL_MANIFEST,
};

const ANDROID: Platform = Platform {
    os: "android",
    arch: "arm64",
};
const LIBRARY: &[u8] = b"original guest library fixture";

/// An installed fake server beside user files, a fake translator and a cached guest library.
struct Fixture {
    dir: tempfile::TempDir,
    provisioner: Arc<Provisioner>,
    translation: Translation,
    server: PathBuf,
    install: PathBuf,
    runtime: PathBuf,
    /// Holds the fake's logs. Creating `fail` makes the server exit before it is ready, `hang`
    /// makes it wait unready after creating `waiting`, `no-cache` keeps the translator from
    /// writing code and `library-only` makes it write code for a library but not the server.
    control: PathBuf,
    helper: PathBuf,
    service: FakeMojang,
    pin: TranslationPin,
}

impl Fixture {
    fn new() -> Self {
        let dir = scratch();
        let control = dir.path().join("control");
        fs::create_dir_all(&control).unwrap();
        let control_text = control.to_str().unwrap();
        let prelude = format!(
            "control='{control_text}'\n\
             printf '%s %s\\n' \"$(pwd -P)\" \"$(sed -n 's/^level-name=//p' server.properties)\" >> \"$control/runs\"\n\
             ls -A > \"$control/entries\"\n\
             cat permissions.json > \"$control/permissions\"\n\
             cat behavior_packs/pack.json > \"$control/pack\"\n\
             [ ! -e \"$control/fail\" ] || exit 3\n\
             if [ -e \"$control/hang\" ]; then : > \"$control/waiting\"; while IFS= read -r line; do :; done; exit 0; fi\n"
        );
        let original = String::from_utf8(FAKE_BDS.to_vec()).unwrap();
        let server_script = original
            .replacen("#!/bin/sh\n", &format!("#!/bin/sh\n{prelude}"), 1)
            // BDS creates a new world's folder itself.
            .replace(
                "    [ -e \"$world/level.dat\" ]",
                "    mkdir -p \"$world\"\n    [ -e \"$world/level.dat\" ]",
            );
        assert!(server_script.contains("mkdir -p"), "fake server changed");
        let provisioner = installed_provisioner(dir.path(), ANDROID, server_script.as_bytes());
        provisioner.set_runtime(RuntimeInfo::translated());
        let server = provisioner.installed().unwrap().0;
        let install = install_dir(&server).unwrap().to_path_buf();
        fs::create_dir_all(install.join("behavior_packs")).unwrap();
        fs::write(install.join("behavior_packs/pack.json"), b"pack").unwrap();
        fs::write(install.join("profanity_filter.wlist"), b"filter").unwrap();
        for (name, bytes) in [
            (PROPERTIES_FILE, &b"user properties"[..]),
            (PERMISSIONS_FILE, b"user operators"),
            (ALLOWLIST_FILE, b"user allow list"),
        ] {
            fs::write(install.join(name), bytes).unwrap();
        }
        fs::create_dir_all(install.join(WORLDS_DIR).join("user")).unwrap();
        fs::write(
            install.join(WORLDS_DIR).join("user/level.dat"),
            b"user world",
        )
        .unwrap();
        let helper = dir.path().join("translator");
        write_script(
            &helper,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$BOX64_DYNACACHE\" >> \"$0.modes\"\n\
                 code() {{ [ \"$BOX64_DYNACACHE\" = 1 ] && [ ! -e \"{control_text}/no-cache\" ] || return 0\n\
                   name=bedrock_server; [ ! -e \"{control_text}/library-only\" ] || name=libgcc_s.so.1\n\
                   printf code > \"$BOX64_DYNACACHE_FOLDER/$name-1-2.box64\"; }}\n\
                 trap code EXIT\n\
                 . \"$1\"\n"
            )
            .as_bytes(),
        );
        write_script(
            &dir.path().join("loader"),
            b"#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$0.args\"\nshift 5\nexec \"$@\"\n",
        );
        let service = FakeMojang::start("unused", LIBRARY.to_vec());
        let pin = TranslationPin::fixture(LIBRARY, format!("{}/bin-linux/", service.base));
        let runtime = dir.path().join("runtime");
        let guest = pin.guest_path(&runtime);
        fs::create_dir_all(guest.parent().unwrap()).unwrap();
        fs::write(guest, LIBRARY).unwrap();
        let fixture = Self {
            translation: Translation::new(helper.clone(), runtime.clone(), pin.clone()),
            provisioner: Arc::new(provisioner),
            dir,
            server,
            install,
            runtime,
            control,
            helper,
            service,
            pin,
        };
        fixture.set_build("build one");
        fixture
    }

    /// Records `digest` as the installed server archive's, which identifies its code cache.
    fn set_build(&self, digest: &str) {
        let manifest = serde_json::json!({ "zip_sha256": digest }).to_string();
        fs::write(self.install.join(INSTALL_MANIFEST), manifest).unwrap();
    }

    fn prepare(&self, cancel: &Cancel) -> Result<(), WorldError> {
        self.translation
            .prepare(&self.server, &self.provisioner, cancel)
    }

    fn prepared(&self) -> bool {
        self.translation.is_prepared(&self.server)
    }

    /// The translator cache mode of every launch so far.
    fn modes(&self) -> Vec<String> {
        lines(&self.helper.with_extension("modes"))
    }

    /// The working folder and level name of every server launch so far.
    fn runs(&self) -> Vec<String> {
        lines(&self.control.join("runs"))
    }

    fn touch(&self, name: &str) {
        fs::write(self.control.join(name), b"").unwrap();
    }

    fn scratch_dir(&self) -> PathBuf {
        self.runtime.join(SCRATCH_DIR)
    }

    /// The install's user configuration, access lists and worlds, as the fixture wrote them.
    fn assert_user_files_untouched(&self) {
        for (name, bytes) in [
            (PROPERTIES_FILE, &b"user properties"[..]),
            (PERMISSIONS_FILE, b"user operators"),
            (ALLOWLIST_FILE, b"user allow list"),
        ] {
            assert_eq!(fs::read(self.install.join(name)).unwrap(), bytes, "{name}");
        }
        let worlds: Vec<_> = fs::read_dir(self.install.join(WORLDS_DIR))
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(worlds, ["user"]);
        assert_eq!(
            fs::read(self.install.join(WORLDS_DIR).join("user/level.dat")).unwrap(),
            b"user world"
        );
    }
}

/// The lines of `path`; empty when it does not exist.
fn lines(path: &Path) -> Vec<String> {
    fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .map(str::to_owned)
        .collect()
}

#[test]
fn setup_produces_startup_code_once_from_scratch_and_the_first_world_only_loads_it() {
    let fixture = Fixture::new();
    assert!(!fixture.prepared());
    fixture.prepare(&Cancel::never()).unwrap();
    assert!(fixture.prepared());
    assert_eq!(fixture.modes(), ["1"]);
    let scratch_dir = fs::canonicalize(&fixture.runtime)
        .unwrap()
        .join(SCRATCH_DIR);
    assert_eq!(
        fixture.runs(),
        [format!("{} {SCRATCH_WORLD}", scratch_dir.display())]
    );
    let mut entries = lines(&fixture.control.join("entries"));
    entries.sort();
    assert_eq!(
        entries,
        [
            "bedrock_server",
            "behavior_packs",
            INSTALL_MANIFEST,
            PERMISSIONS_FILE,
            "profanity_filter.wlist",
            PROPERTIES_FILE,
        ]
    );
    assert_eq!(fs::read(fixture.control.join("pack")).unwrap(), b"pack");
    assert_eq!(
        fs::read(fixture.control.join("permissions")).unwrap(),
        b"[]"
    );
    assert_eq!(
        lines(&fixture.dir.path().join("loader.args"))
            .last()
            .unwrap(),
        fixture.server.to_str().unwrap(),
        "the exact installed binary runs"
    );
    assert!(fs::symlink_metadata(fixture.scratch_dir()).is_err());
    assert_eq!(
        fs::read(fixture.install.join("behavior_packs/pack.json")).unwrap(),
        b"pack"
    );
    fixture.assert_user_files_untouched();

    fixture.prepare(&Cancel::never()).unwrap();
    assert_eq!(
        fixture.modes(),
        ["1"],
        "a prepared server is not launched again"
    );

    let mut runner = BdsRunner::new(Arc::clone(&fixture.provisioner), String::new());
    let translation = Arc::new(Translation::new(
        fixture.helper.clone(),
        fixture.runtime.clone(),
        fixture.pin.clone(),
    ));
    crate::local_worlds::android_setup::configure_preparation(&fixture.provisioner, &translation);
    runner.translation = Some(translation);
    runner.start_timeout = Some(test_time::DEFAULT_TIMEOUT);
    let world = spec(&fixture.dir.path().join("world"));
    let instance = runner.start(&world, &Cancel::never()).unwrap();
    instance.stop(test_time::DEFAULT_TIMEOUT).unwrap();
    // The new world's create run and its final run both only load the prepared code.
    assert_eq!(fixture.modes(), ["1", "2", "2"]);
    assert!(fixture.prepared());
}

#[test]
fn a_new_server_build_needs_preparation_again() {
    let fixture = Fixture::new();
    fixture.prepare(&Cancel::never()).unwrap();
    fixture.set_build("build two");
    assert!(!fixture.prepared());
    fixture.prepare(&Cancel::never()).unwrap();
    assert!(fixture.prepared());
    assert_eq!(fixture.modes(), ["1", "1"]);
    let generations = fs::read_dir(fixture.runtime.join(CODE_CACHE_DIR))
        .unwrap()
        .count();
    assert_eq!(generations, 1, "the previous build's generation is removed");
}

#[test]
fn a_damaged_guest_library_is_redownloaded_without_relaunching_the_server() {
    let fixture = Fixture::new();
    fixture.prepare(&Cancel::never()).unwrap();
    let guest = fixture.pin.guest_path(&fixture.runtime);
    fs::write(&guest, b"damaged").unwrap();
    assert!(!fixture.prepared());
    fixture.prepare(&Cancel::never()).unwrap();
    assert!(fixture.prepared());
    assert_eq!(fs::read(guest).unwrap(), LIBRARY);
    assert_eq!(
        fixture
            .service
            .hits
            .load(std::sync::atomic::Ordering::SeqCst),
        1
    );
    assert_eq!(fixture.modes(), ["1"]);
}

#[test]
fn cancelling_before_the_server_is_ready_returns_its_attempt_and_cleans_up() {
    let fixture = Fixture::new();
    let (handle, cancelled) = cancel_pair();
    drop(handle);
    assert!(matches!(
        fixture.prepare(&cancelled),
        Err(WorldError::Cancelled)
    ));
    assert!(
        fixture.modes().is_empty(),
        "an already cancelled setup launches nothing"
    );

    fixture.touch("hang");
    for _ in 0..=PRODUCER_ATTEMPTS {
        let (handle, cancel) = cancel_pair();
        let waiting = fixture.control.join("waiting");
        let _ = fs::remove_file(&waiting);
        thread::scope(|scope| {
            let preparing = scope.spawn(|| fixture.prepare(&cancel));
            test_time::eventually("the setup server starts", || waiting.exists());
            drop(handle);
            assert!(matches!(
                preparing.join().unwrap(),
                Err(WorldError::Cancelled)
            ));
        });
        assert!(!fixture.prepared());
        assert!(fs::symlink_metadata(fixture.scratch_dir()).is_err());
    }
    fs::remove_file(fixture.control.join("hang")).unwrap();
    fixture.prepare(&Cancel::never()).unwrap();
    assert!(fixture.prepared());
    fixture.assert_user_files_untouched();
}

#[test]
fn failed_setup_runs_are_reported_bounded_and_never_retried() {
    let fixture = Fixture::new();
    fixture.touch("fail");
    for run in 1..=PRODUCER_ATTEMPTS {
        assert!(matches!(
            fixture.prepare(&Cancel::never()),
            Err(WorldError::Failed(_))
        ));
        assert_eq!(fixture.modes().len(), run as usize, "one launch per call");
        assert!(!fixture.prepared());
        assert!(fs::symlink_metadata(fixture.scratch_dir()).is_err());
    }
    fs::remove_file(fixture.control.join("fail")).unwrap();
    // The exhausted generation stays off: setup reports it and launches nothing.
    assert!(matches!(
        fixture.prepare(&Cancel::never()),
        Err(WorldError::Failed(_))
    ));
    assert_eq!(fixture.modes().len(), PRODUCER_ATTEMPTS as usize);
    assert!(!fixture.prepared());
    fixture.assert_user_files_untouched();

    fixture.set_build("build two");
    fixture.prepare(&Cancel::never()).unwrap();
    assert!(fixture.prepared());
}

#[test]
fn a_clean_run_without_the_servers_own_startup_code_is_not_prepared() {
    for control in ["no-cache", "library-only"] {
        let fixture = Fixture::new();
        fixture.touch(control);
        assert!(
            matches!(
                fixture.prepare(&Cancel::never()),
                Err(WorldError::Failed(_))
            ),
            "{control}"
        );
        assert_eq!(fixture.modes(), ["1"]);
        assert!(!fixture.prepared());
        assert!(fs::symlink_metadata(fixture.scratch_dir()).is_err());
        fs::remove_file(fixture.control.join(control)).unwrap();
        fixture.prepare(&Cancel::never()).unwrap();
        assert!(fixture.prepared(), "{control}: the next setup produces it");
    }
}

#[test]
fn an_install_without_a_recorded_build_cannot_be_prepared() {
    let fixture = Fixture::new();
    fs::remove_file(fixture.install.join(INSTALL_MANIFEST)).unwrap();
    assert!(!fixture.prepared());
    assert!(matches!(
        fixture.prepare(&Cancel::never()),
        Err(WorldError::Failed(_))
    ));
    assert!(fixture.modes().is_empty());
}

/// A world runner sharing the fixture's translator, runtime folder and install.
fn world_runner(fixture: &Fixture) -> BdsRunner {
    let mut runner = BdsRunner::new(Arc::clone(&fixture.provisioner), String::new());
    let translation = Arc::new(Translation::new(
        fixture.helper.clone(),
        fixture.runtime.clone(),
        fixture.pin.clone(),
    ));
    crate::local_worlds::android_setup::configure_preparation(&fixture.provisioner, &translation);
    runner.translation = Some(translation);
    runner.start_timeout = Some(test_time::DEFAULT_TIMEOUT);
    runner
}

#[test]
fn a_world_start_runs_with_the_cache_off_after_a_failed_run_disabled_it() {
    let fixture = Fixture::new();
    fixture.prepare(&Cancel::never()).unwrap();
    let runner = world_runner(&fixture);
    let world = spec(&fixture.dir.path().join("world"));
    // A world server that loaded the prepared code and then failed turns this build's code off.
    fixture.touch("fail");
    assert!(runner.start(&world, &Cancel::never()).is_err());
    fs::remove_file(fixture.control.join("fail")).unwrap();
    assert_eq!(fixture.modes(), ["1", "2"]);
    assert!(!fixture.prepared());
    assert!(
        matches!(
            fixture.prepare(&Cancel::never()),
            Err(WorldError::Failed(_))
        ),
        "setup reports the disabled code"
    );

    // World starts never run setup: the world's own runs use the cache as it chooses.
    let instance = runner.start(&world, &Cancel::never()).unwrap();
    instance.stop(test_time::DEFAULT_TIMEOUT).unwrap();
    assert_eq!(fixture.modes(), ["1", "2", "0", "0"]);
}

#[test]
fn a_world_start_runs_with_the_cache_off_once_setup_attempts_are_used() {
    let fixture = Fixture::new();
    fixture.touch("fail");
    for _ in 0..PRODUCER_ATTEMPTS {
        assert!(fixture.prepare(&Cancel::never()).is_err());
    }
    fs::remove_file(fixture.control.join("fail")).unwrap();
    assert_eq!(fixture.modes().len(), PRODUCER_ATTEMPTS as usize);
    let runner = world_runner(&fixture);
    let world = spec(&fixture.dir.path().join("world"));
    let instance = runner.start(&world, &Cancel::never()).unwrap();
    instance.stop(test_time::DEFAULT_TIMEOUT).unwrap();
    let modes = fixture.modes();
    assert_eq!(modes[PRODUCER_ATTEMPTS as usize..], ["0", "0"]);
}

#[test]
fn a_world_start_before_setup_produces_with_its_own_create_run() {
    let fixture = Fixture::new();
    let runner = world_runner(&fixture);
    let instance = runner
        .start(&spec(&fixture.dir.path().join("world")), &Cancel::never())
        .unwrap();
    instance.stop(test_time::DEFAULT_TIMEOUT).unwrap();
    assert_eq!(fixture.modes(), ["1", "2"], "no separate setup run");
    assert!(fixture.prepared());
}
