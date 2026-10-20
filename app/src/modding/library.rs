//! The Settings Mods tab's library. Imports, removal and selection run on one worker, which
//! publishes the per-user mods directory with the host's recorded status.

use super::registration::{
    removal,
    selection::{self, Receipt, disable, forget, registered, same_path, select},
};
use bevy::prelude::*;
use client_shell::menu::MenuRuntime;
use crossbeam_channel::{Receiver, RecvTimeoutError, Sender, unbounded};
use launcher::mods::{
    Action, ActiveMod, COMPONENT_EXTENSION, InstalledMod, Library, Permission, UpdatePhase,
    UpdateProgress,
};
use launcher_host::desktop::picker::{self, Filter};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::Arc,
};

mod update;

enum Command {
    Action(Action),
    Update(launcher_host::mod_updates::Downloaded, Sender<()>),
    Message(String),
}

const FILTER: Filter = Filter {
    title: "Import mod",
    name: "Mods",
    extensions: &[COMPONENT_EXTENSION],
    unavailable_hint: "",
};

#[derive(Resource)]
pub(super) struct ModLibrary {
    commands: Sender<Command>,
    published: Receiver<Library>,
    updates: Option<launcher_host::mod_updates::Checker>,
    checks: bool,
    automatic: bool,
    busy: bool,
    progress: Vec<UpdateProgress>,
    installing: Vec<(u64, Receiver<()>)>,
}

/// Starts the library worker and the system that connects it to the Mods tab.
pub(super) fn configure(
    app: &mut App,
    mods: PathBuf,
    registration: PathBuf,
    removal: removal::Client,
) {
    let (commands, incoming) = unbounded();
    let (outgoing, published) = unbounded();
    let spawned = std::thread::Builder::new()
        .name("mod-library".into())
        .spawn(move || {
            run(
                &mods,
                &registration,
                &incoming,
                &outgoing,
                || picker::pick(&FILTER),
                |request| removal.wait(request),
            )
        });
    if let Err(error) = spawned {
        warn!("Mod library unavailable: {error}");
        return;
    }
    app.insert_resource(ModLibrary {
        commands,
        published,
        updates: launcher_host::mod_updates::Checker::start(
            mod_host::packet_mod::MAX_PACKET_MOD_BYTES,
        )
        .map_err(|error| {
            warn!("Mod update checks unavailable: {error}");
        })
        .ok(),
        checks: false,
        automatic: false,
        busy: false,
        progress: Vec::new(),
        installing: Vec::new(),
    })
    .add_systems(Update, drive);
}

/// Forwards Mods-tab work and publishes the newest library.
fn drive(mut library: ResMut<ModLibrary>, menu: Option<ResMut<MenuRuntime>>) {
    let Some(mut menu) = menu else { return };
    let mut requested = None;
    for action in std::mem::take(&mut menu.mod_actions) {
        if let Action::Update(id) = action {
            requested = Some(id);
        } else {
            let _ = library.commands.send(Command::Action(action));
        }
    }
    let latest = library.published.try_iter().last();
    let changed = latest.is_some();
    if let Some(latest) = latest {
        library.busy = latest.busy;
        library.progress = latest.update_progress.clone();
        Arc::make_mut(&mut menu.mods).publish(latest);
    }
    let checks = menu
        .settings_snapshot()
        .0
        .value(launcher::menu::settings_options::SettingId::ModUpdateChecks)
        != 0;
    let automatic = menu
        .settings_snapshot()
        .0
        .value(launcher::menu::settings_options::SettingId::ModAutoUpdate)
        != 0;
    let configure = changed || checks != library.checks || automatic != library.automatic;
    library.checks = checks;
    library.automatic = automatic;
    if let Some(checker) = &mut library.updates {
        if configure {
            checker.configure(&menu.mods.library.installed, checks, automatic);
        }
        let updates = checker.updates();
        let updates = (menu.mods.library.updates != updates).then(|| updates.to_vec());
        let started = requested.is_some_and(|id| checker.install(id));
        let pending = checker.pending();
        let message = checker.take_message().or_else(|| started.then(String::new));
        let downloads = checker.take_downloads();
        let progress = checker.progress();
        for download in downloads {
            let id = download.id;
            let (finished, completion) = unbounded();
            if library
                .commands
                .send(Command::Update(download, finished))
                .is_ok()
            {
                library.installing.push((id, completion));
            }
        }
        library.installing.retain(|(_, completion)| {
            matches!(
                completion.try_recv(),
                Err(crossbeam_channel::TryRecvError::Empty)
            )
        });
        let mut shown = library.progress.clone();
        shown.extend(progress);
        for (id, _) in &library.installing {
            if menu
                .mods
                .library
                .installed
                .iter()
                .any(|item| item.id == *id)
                && !shown.iter().any(|progress| progress.id == *id)
            {
                shown.push(UpdateProgress::stage(*id, UpdatePhase::Installing));
            }
        }
        let busy = library.busy || pending.is_some() || !library.installing.is_empty();
        if updates.is_some()
            || menu.mods.library.update_progress != shown
            || menu.mods.library.busy != busy
            || message.is_some()
        {
            let snapshot = Arc::make_mut(&mut menu.mods);
            if let Some(updates) = updates {
                snapshot.library.updates = updates;
            }
            snapshot.library.update_progress = shown;
            snapshot.library.busy = busy;
            if let Some(message) = message {
                snapshot.library.message = message.clone();
                let _ = library.commands.send(Command::Message(message));
            }
        }
    }
}

/// Serves commands in order and republishes when the directory or recorded status changes.
fn run(
    mods: &Path,
    registration: &Path,
    incoming: &Receiver<Command>,
    outgoing: &Sender<Library>,
    mut pick: impl FnMut() -> Result<Option<PathBuf>, String>,
    mut retire: impl FnMut(&Receipt) -> Result<removal::Permit, String>,
) {
    let mut requests = 0;
    let mut metadata = BTreeMap::new();
    let mut reloads = Vec::<update::PendingReload>::new();
    let mut library = scan_cached(mods, registration, String::new(), &mut metadata);
    let _ = outgoing.send(library.clone());
    loop {
        let message = match incoming.recv_timeout(super::RELOAD_INTERVAL) {
            Ok(Command::Message(message)) => message,
            Ok(Command::Update(download, finished)) => {
                library.busy = true;
                library.update_progress =
                    vec![UpdateProgress::stage(download.id, UpdatePhase::Installing)];
                let _ = outgoing.send(library.clone());
                match update::install(mods, &download) {
                    Ok(installed) => {
                        let message = format!(
                            "Installed update {}. Reloading enabled mods…",
                            download.version
                        );
                        requests += 1;
                        let request = format!("mod-update-{}-{requests}", std::process::id());
                        match update::PendingReload::start(
                            mods,
                            registration,
                            installed,
                            request,
                            finished,
                            std::time::Instant::now(),
                        ) {
                            Ok(Some(pending)) => {
                                reloads.push(pending);
                                message
                            }
                            Ok(None) => format!(
                                "Updated {} to {}.",
                                library
                                    .installed
                                    .iter()
                                    .find(|item| item.id == download.id)
                                    .map(|item| item.file.as_str())
                                    .unwrap_or("mod"),
                                download.version
                            ),
                            Err(error) => error,
                        }
                    }
                    Err(error) => {
                        let _ = finished.send(());
                        error
                    }
                }
            }
            Ok(Command::Action(action)) => {
                if matches!(action, Action::Import | Action::Remove(_)) {
                    library.busy = true;
                    let _ = outgoing.send(library.clone());
                }
                perform(
                    action,
                    mods,
                    registration,
                    &mut requests,
                    &mut pick,
                    &mut retire,
                )
                .unwrap_or_else(|error| error)
            }
            Err(RecvTimeoutError::Timeout) => library.message.clone(),
            Err(RecvTimeoutError::Disconnected) => return,
        };
        let mut next = scan_cached(mods, registration, message, &mut metadata);
        let now = std::time::Instant::now();
        reloads.retain_mut(|pending| {
            if let Some(result) = pending.poll(&next, now) {
                next.message = result.unwrap_or_else(|error| error);
                pending.finish();
                false
            } else {
                true
            }
        });
        next.update_progress = reloads
            .iter()
            .map(|pending| UpdateProgress::stage(pending.id, UpdatePhase::Reloading))
            .collect();
        next.busy = !reloads.is_empty();
        if next != library {
            library = next;
            let _ = outgoing.send(library.clone());
        }
    }
}

/// Runs one Mods-tab command; both outcomes are the message the tab shows.
fn perform(
    action: Action,
    mods: &Path,
    registration: &Path,
    requests: &mut u64,
    pick: &mut impl FnMut() -> Result<Option<PathBuf>, String>,
    retire: &mut impl FnMut(&Receipt) -> Result<removal::Permit, String>,
) -> Result<String, String> {
    let mut entries = installed(mods);
    if !matches!(action, Action::Enable(_) | Action::Remove(_)) {
        include_active(&mut entries, &registered(registration));
    }
    let path = |id: u64| {
        entries
            .iter()
            .find(|(item, _)| item.id == id)
            .map(|(_, path)| path.clone())
            .ok_or_else(|| "That mod is no longer installed.".to_owned())
    };
    let mut request = || {
        *requests += 1;
        format!("settings-{}-{}", std::process::id(), *requests)
    };
    match action {
        Action::Import => match pick() {
            Ok(Some(source)) => import(mods, &source).map(|name| format!("Imported {name}.")),
            Ok(None) => Ok(String::new()),
            Err(error) => Err(format!("{error}The mods folder is {}.", mods.display())),
        },
        Action::Enable(id) => {
            select(registration, &path(id)?, true, request()).map(|()| String::new())
        }
        Action::Disable(id) => disable(registration, &path(id)?, request()).map(|()| String::new()),
        Action::Reload(id) => {
            selection::reload(registration, &path(id)?, request()).map(|()| String::new())
        }
        Action::ReloadAll => selection::reload_all(registration, request()).map(|()| String::new()),
        Action::MoveUp(id) | Action::MoveDown(id) => selection::move_entry(
            registration,
            &path(id)?,
            matches!(action, Action::MoveUp(_)),
            request(),
        )
        .map(|()| String::new()),
        Action::Permission(id, capability, enabled) => {
            selection::permission(registration, &path(id)?, capability, enabled, request())
                .map(|()| String::new())
        }
        Action::AllowRequested(id) => {
            selection::allow_requested(registration, &path(id)?, request()).map(|()| String::new())
        }
        Action::RevokeAll(id) => {
            selection::revoke_all(registration, &path(id)?, request()).map(|()| String::new())
        }
        Action::Remove(id) => {
            let path = path(id)?;
            let request_id = request();
            let receipt = forget(registration, &path, request_id)?;
            let permit = retire(&receipt)?;
            if !installed(mods)
                .iter()
                .any(|(item, current)| item.id == id && same_path(current, &path))
            {
                return Err("That mod changed while removal was pending.".into());
            }
            let companion = settings_companion(&path);
            let shared = permit.shared_companion(&companion)
                || registered(registration).iter().any(|current| {
                    !same_path(&current.entry.component, &path)
                        && same_path(&settings_companion(&current.entry.component), &companion)
                });
            remove(
                &path,
                (!shared).then_some(companion.as_path()),
                registration,
                &receipt,
                &permit,
            )
            .map(|()| String::new())
        }
        Action::ReadMore(_)
        | Action::Update(_)
        | Action::ViewUpdates
        | Action::OpenUpdate(_)
        | Action::ToggleActive
        | Action::ToggleAvailable
        | Action::Customize(_)
        | Action::ShowAllPermissions(_) => Ok(String::new()),
    }
}

/// Validates a chosen component and copies it into the mods directory; returns its file name.
fn import(mods: &Path, source: &Path) -> Result<String, String> {
    let name = source
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|_| is_component(source))
        .ok_or_else(|| format!("Choose a .{COMPONENT_EXTENSION} mod component."))?;
    if name.starts_with('.') {
        return Err("Mod file names must not start with a dot.".into());
    }
    let bytes =
        mod_host::read_source(source).map_err(|error| format!("Could not read {name}: {error}"))?;
    mod_host::validate_source(&bytes)
        .map_err(|error| format!("{name} is not a valid mod: {error:#}"))?;
    publish_component(mods, name, &bytes)?;
    Ok(name.to_owned())
}

/// Publishes complete bytes under a new name without overwriting any existing file or link.
fn publish_component(mods: &Path, name: &str, bytes: &[u8]) -> Result<(), String> {
    fs::create_dir_all(mods).map_err(|error| format!("Could not install {name}: {error}"))?;
    let staging = mods.join(format!(".{name}.{}.importing", std::process::id()));
    let mut file = fs::File::create_new(&staging)
        .map_err(|error| format!("Could not stage {name}: {error}"))?;
    let result = file
        .write_all(bytes)
        .and_then(|()| fs::hard_link(&staging, mods.join(name)));
    drop(file);
    let _ = fs::remove_file(&staging);
    result.map_err(|error| {
        if error.kind() == std::io::ErrorKind::AlreadyExists {
            format!("A mod named {name} is already installed. Choose a different file name.")
        } else {
            format!("Could not install {name}: {error}")
        }
    })
}

/// The companion entry used for this component's persisted settings.
fn settings_companion(path: &Path) -> PathBuf {
    mod_host::settings_companion(path)
}

/// Rechecks the removal's selection before deleting each retired component or owned companion.
fn remove(
    path: &Path,
    companion: Option<&Path>,
    registration: &Path,
    receipt: &Receipt,
    permit: &removal::Permit,
) -> Result<(), String> {
    for file in std::iter::once(path).chain(companion) {
        receipt.confirm(registration)?;
        permit.confirm()?;
        match fs::remove_file(file) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound && file != path => {}
            Err(error) => return Err(format!("Could not remove the mod: {error}")),
        }
    }
    Ok(())
}

/// Recognizes the component extension accepted by the Mods tab.
fn is_component(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case(COMPONENT_EXTENSION))
}

/// Lists visible components by file name; staging files have a different extension.
fn installed(mods: &Path) -> Vec<(InstalledMod, PathBuf)> {
    let Ok(entries) = fs::read_dir(mods) else {
        return Vec::new();
    };
    let mut found: Vec<_> = entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let file = path.file_name()?.to_str()?.to_owned();
            let metadata = fs::symlink_metadata(&path)
                .ok()
                .filter(fs::Metadata::is_file)?;
            (!file.starts_with('.') && is_component(&path)).then(|| {
                let item = InstalledMod {
                    id: file_identity(&path, &metadata),
                    file,
                    bytes: metadata.len(),
                    permissions: Vec::new(),
                    metadata: Default::default(),
                };
                (item, path)
            })
        })
        .collect();
    found.sort_by(|(left, _), (right, _)| left.file.cmp(&right.file));
    found
}

/// Identifies a component's path and file revision without rereading its contents.
fn file_identity(path: &Path, metadata: &fs::Metadata) -> u64 {
    let mut digest = Sha256::new();
    digest.update(path.as_os_str().as_encoded_bytes());
    digest.update(metadata.len().to_le_bytes());
    for timestamp in [metadata.created(), metadata.modified()]
        .into_iter()
        .flatten()
    {
        if let Ok(elapsed) = timestamp.duration_since(std::time::UNIX_EPOCH) {
            digest.update(elapsed.as_nanos().to_le_bytes());
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        digest.update(metadata.dev().to_le_bytes());
        digest.update(metadata.ino().to_le_bytes());
    }
    let digest = digest.finalize();
    u64::from_le_bytes(digest[..8].try_into().expect("eight digest bytes"))
}

/// Includes enabled external sources with file revisions, and missing sources by path alone.
fn include_active(entries: &mut Vec<(InstalledMod, PathBuf)>, current: &[selection::Registered]) {
    for current in current.iter().filter(|current| current.entry.enabled) {
        let path = &current.entry.component;
        if entries
            .iter()
            .any(|(_, installed)| same_path(installed, path))
        {
            continue;
        }
        let metadata = fs::symlink_metadata(path)
            .ok()
            .filter(fs::Metadata::is_file);
        entries.push((
            InstalledMod {
                id: metadata.as_ref().map_or_else(
                    || selection::component_id(path),
                    |metadata| file_identity(path, metadata),
                ),
                file: path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
                bytes: metadata.as_ref().map_or(0, fs::Metadata::len),
                permissions: Vec::new(),
                metadata: Default::default(),
            },
            path.clone(),
        ));
    }
}

/// Builds card metadata, permissions and statuses from each component's declarations and the
/// host's canonical registration and grants.
#[cfg(test)]
fn scan(mods: &Path, registration: &Path, message: String) -> Library {
    scan_cached(mods, registration, message, &mut BTreeMap::new())
}

/// File identities invalidate cached declarations so unchanged large mods are not reread on polls.
fn scan_cached(
    mods: &Path,
    registration: &Path,
    message: String,
    metadata: &mut BTreeMap<u64, (Option<mod_host::ModGrants>, ui::mod_metadata::ModMetadata)>,
) -> Library {
    let mut installed = installed(mods);
    let current = registered(registration);
    include_active(&mut installed, &current);
    metadata.retain(|id, _| installed.iter().any(|(item, _)| item.id == *id));
    let active = current
        .iter()
        .filter(|current| current.entry.enabled)
        .filter_map(|current| {
            installed
                .iter()
                .find(|(_, path)| same_path(path, &current.entry.component))
                .map(|(item, _)| ActiveMod {
                    id: item.id,
                    status: current.status.clone(),
                })
        })
        .collect();
    for (item, path) in &mut installed {
        let grants = current
            .iter()
            .find(|current| same_path(path, &current.entry.component))
            .map(|current| &current.entry.grants);
        let (requested, declared_metadata) =
            metadata
                .entry(item.id)
                .or_insert_with(|| match mod_host::declarations(path) {
                    Ok(declared) => (declared.requested.ok().flatten(), declared.metadata),
                    Err(_) => (None, Default::default()),
                });
        item.metadata = declared_metadata.clone();
        item.permissions = mod_host::ModGrants::CAPABILITIES
            .iter()
            .map(|capability| Permission {
                name: capability.label.into(),
                description: capability.description.trim().into(),
                enabled: grants.is_some_and(capability.enabled),
                requested: requested.as_ref().map(capability.enabled),
                requested_allowed: requested.as_ref().is_some_and(|requested| {
                    grants.is_some_and(|grants| (capability.requested_allowed)(grants, requested))
                }),
                sensitive: capability.sensitive,
            })
            .collect();
    }
    Library {
        update_progress: Vec::new(),
        installed: installed.into_iter().map(|(item, _)| item).collect(),
        active,
        busy: false,
        message,
        updates: Vec::new(),
    }
}

#[cfg(test)]
mod tests;
