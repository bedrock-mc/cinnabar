//! Prepares the translated server's reusable startup code during setup, before any world opens.
//!
//! One run of the exact installed server binary, with production translator settings, produces
//! the code-cache generation that later world starts only load. The run's working folder is a
//! private scratch folder that links the install's shared content and holds its own server
//! configuration, operator and access lists and generated world, so it never reads or changes a
//! saved world or the install's per-start files.

use std::{
    ffi::OsStr,
    fs, io,
    path::{Path, PathBuf},
};

use bridge::{Backend, GameMode, contract::HOSTED_WORLD_MAX_PLAYERS};
use launcher::local_worlds::world::{Difficulty, Generator, World};

use super::{
    Cancel,
    bds::{
        BDS_READY_MARKER, BDS_START_TIMEOUT, PERMISSIONS_FILE, PROPERTIES_FILE, WORLDS_DIR,
        server_properties, timed, world_player_limit, write_host_permissions,
    },
    code_cache::{Mode, Off},
    create_private_dir,
    ports::{free_bds_port, loopback},
    process::{Ending, ExitHook, Instance, Launch, SAVE_BUDGET, StartSpec, launch},
    provision::{Provisioner, install_dir},
    translated::Translation,
    world::{WorldError, failed},
    write_private,
};

/// The scratch working folder beneath the runtime folder. The provisioner runs one preparation at
/// a time, so a fixed name lets each run remove what an interrupted one left behind.
const SCRATCH_DIR: &str = "server-setup";
/// BDS's access list; the scratch server creates its own.
const ALLOWLIST_FILE: &str = "allowlist.json";
/// Install entries the scratch server never sees: its per-start configuration, operator and
/// access lists, and the saved worlds.
const PRIVATE_ENTRIES: [&str; 4] = [
    PROPERTIES_FILE,
    PERMISSIONS_FILE,
    ALLOWLIST_FILE,
    WORLDS_DIR,
];
/// The scratch world's level name, inside the scratch folder's own `worlds` folder.
const SCRATCH_WORLD: &str = "setup";

impl Translation {
    /// Whether `server`'s startup code is ready to reuse: the pinned guest library is cached
    /// intact and the code-cache generation for this exact install and runtime is sealed with the
    /// server executable's own cache file and all its files present. Only reads: it launches
    /// nothing, takes no cache lease and uses no producer attempt.
    pub(crate) fn is_prepared(&self, server: &Path) -> bool {
        self.library_ready() && self.code_cache(server).is_some_and(|cache| cache.ready())
    }

    /// Starts a new setup attempt after an earlier failure disabled startup code. This removes
    /// only this build's disposable generation; a world start never resets its failure state.
    pub(super) fn retry_preparation(&self, server: &Path) -> Result<(), WorldError> {
        if let Some(cache) = self.code_cache(server) {
            cache
                .retry_disabled()
                .map_err(|error| failed(format!("reset startup code: {error}")))?;
        }
        Ok(())
    }

    /// Makes [`Self::is_prepared`] hold for the installed binary `server`, doing nothing when it
    /// already does. Otherwise it caches the guest library, runs `server` from its install path
    /// with production translator settings until BDS reports it started, stops it within the save
    /// budget and returns once the exit has sealed a reusable code-cache generation.
    ///
    /// The caller serializes this with other setup and world starts on the same install and never
    /// holds a world open meanwhile. A cache that is off, a server that fails or a clean run that
    /// leaves no reusable generation is an error; each uses the cache's bounded attempts, and
    /// nothing is retried here. A start cancelled before the server is ready returns its attempt.
    pub(crate) fn prepare(
        &self,
        server: &Path,
        provisioner: &Provisioner,
        cancel: &Cancel,
    ) -> Result<(), WorldError> {
        if cancel.is_cancelled() {
            return Err(WorldError::Cancelled);
        }
        if self.is_prepared(server) {
            return Ok(());
        }
        let install = install_dir(server)?;
        if self.code_cache(server).is_none() {
            return Err(unavailable(&Off::Unidentified));
        }
        timed("translated server setup", "startup code", || {
            self.produce(server, install, provisioner, cancel)
        })
    }

    /// One producer run of `server` from a scratch working folder, as [`Self::prepare`] describes.
    fn produce(
        &self,
        server: &Path,
        install: &Path,
        provisioner: &Provisioner,
        cancel: &Cancel,
    ) -> Result<(), WorldError> {
        let spec = scratch_spec(self.runtime_dir().join(SCRATCH_DIR));
        let max_players = world_player_limit(&spec, HOSTED_WORLD_MAX_PLAYERS);
        let port = free_bds_port(max_players, 0)?;
        // Ready before the cache is chosen, so no failure here can use a producer attempt.
        let scratch = Scratch::create(&spec.dir, install, &spec, port, max_players)
            .map_err(|error| failed(format!("prepare the server setup folder: {error}")))?;
        let (mut command, used) = self.command(server, provisioner, cancel)?;
        match used.mode() {
            Mode::Produce => {}
            // Downloading the guest library was all that was missing.
            Mode::Load if self.is_prepared(server) => return Ok(()),
            Mode::Load => {
                return Err(failed(
                    "the translated server's startup code changed during setup",
                ));
            }
            Mode::Off => {
                return Err(used.off_reason().map_or_else(
                    || failed("translated server startup code cannot be prepared"),
                    unavailable,
                ));
            }
        }
        command.current_dir(&scratch.0).env("LD_LIBRARY_PATH", ".");
        let (ended, ending) = crossbeam_channel::bounded(1);
        let on_exit: ExitHook = Box::new(move |end| {
            used.finish(end);
            let _ = ended.send(end);
        });
        let process = launch(
            command,
            Launch {
                address: loopback(port),
                timeout: BDS_START_TIMEOUT,
                can_pause: false,
                ready: |line| line.contains(BDS_READY_MARKER),
                label: "bds setup".to_owned(),
                on_exit: Some(on_exit),
            },
            cancel,
        )
        .map_err(|error| match error {
            WorldError::Cancelled => error,
            error => failed(format!("start the translated server setup run: {error}")),
        })?;
        let stopped = process.stop(SAVE_BUDGET);
        // The exit hook has run once the stop returns, so its ending and seal are settled.
        let ending = ending.recv().unwrap_or(Ending::Failed);
        stopped
            .map_err(|error| failed(format!("stop the translated server setup run: {error}")))?;
        if ending != Ending::Clean {
            return Err(failed(format!(
                "the translated server setup run ended {ending:?}"
            )));
        }
        drop(scratch);
        if !self.is_prepared(server) {
            return Err(failed(
                "the translated server setup run left no reusable startup code",
            ));
        }
        Ok(())
    }
}

/// The setup failure for a code cache that is off for `reason`.
fn unavailable(reason: &Off) -> WorldError {
    failed(format!(
        "translated server startup code cannot be prepared: {reason}"
    ))
}

/// The scratch server's world, in its own folder `dir`, with the defaults of a new world.
fn scratch_spec(dir: PathBuf) -> StartSpec {
    StartSpec {
        world: World {
            id: SCRATCH_WORLD.to_owned(),
            name: SCRATCH_WORLD.to_owned(),
            game_mode: GameMode::Survival,
            generator: Generator::default(),
            difficulty: Difficulty::default(),
            backend: Backend::Bds,
            seed: 0,
            options: Default::default(),
            options_unavailable: false,
            created_unix: 0,
            last_played_unix: 0,
            size_bytes: 0,
        },
        dir,
        view_distance: None,
        options_unapplied: true,
        host_xuid: None,
    }
}

/// A private scratch working folder, removed with its contents when dropped. Removal deletes the
/// links to the install, never what they point at.
struct Scratch(PathBuf);

impl Scratch {
    /// Replaces any leftover folder at `dir` with one linking `install`'s shared entries and
    /// holding the scratch server's own configuration and an empty operator list.
    fn create(
        dir: &Path,
        install: &Path,
        spec: &StartSpec,
        port: u16,
        max_players: u32,
    ) -> io::Result<Self> {
        remove_tree(dir)?;
        create_private_dir(dir)?;
        let scratch = Self(dir.to_path_buf());
        link_shared(install, dir)?;
        let properties = server_properties(spec, port, max_players, false);
        write_private(&dir.join(PROPERTIES_FILE), properties.as_bytes())?;
        write_host_permissions(dir, None).map_err(io::Error::other)?;
        Ok(scratch)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        if let Err(error) = remove_tree(&self.0) {
            tracing::warn!("remove the server setup folder: {error}");
        }
    }
}

/// Removes `dir` and everything in it without following links; a missing folder is fine.
fn remove_tree(dir: &Path) -> io::Result<()> {
    match fs::remove_dir_all(dir) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        result => result,
    }
}

/// Links every top-level entry of `install` into `scratch` except [`PRIVATE_ENTRIES`], so the
/// scratch server finds the install's packs, definitions and configuration as it ships them.
fn link_shared(install: &Path, scratch: &Path) -> io::Result<()> {
    for entry in fs::read_dir(install)? {
        let entry = entry?;
        let name = entry.file_name();
        if !is_private(&name) {
            link(&entry.path(), &scratch.join(&name))?;
        }
    }
    Ok(())
}

/// Whether the install entry `name` stays out of the scratch folder.
fn is_private(name: &OsStr) -> bool {
    PRIVATE_ENTRIES.iter().any(|private| name == *private)
}

/// Links one shared install entry without copying it into the temporary world.
#[cfg(unix)]
fn link(target: &Path, link: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

/// Only Android translates the server.
#[cfg(not(unix))]
fn link(_target: &Path, _link: &Path) -> io::Result<()> {
    Err(io::Error::from(io::ErrorKind::Unsupported))
}

#[cfg(test)]
#[path = "translated_setup_tests.rs"]
mod tests;
