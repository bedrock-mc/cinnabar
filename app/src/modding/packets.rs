//! Prepared packet mods belong to new network sessions, never the frame callback host.

use super::registration::{Entry, RegisteredHost, Retired, Watcher};
use bevy::prelude::*;
use client_extensions::modding::packet_relay::{PacketModLoader, PacketModSelection};
use mod_host::packet_mod::CompiledPacketMod;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
};

struct Prepared {
    registration: RegisteredHost,
    program: Arc<CompiledPacketMod>,
    authorized: bool,
}

/// The registration owner's packet programs and the selected source used by joins.
#[derive(Resource, Default)]
pub(crate) struct PacketMods {
    loader: PacketModLoader,
    programs: BTreeMap<PathBuf, Prepared>,
    selected: Option<[u8; 32]>,
    environment: Option<PacketModSelection>,
    environment_selected: bool,
    pub(super) menu: super::packet_menu::State,
}

/// Starts compilation for an explicitly selected developer packet mod before the first join.
pub(crate) fn from_environment() -> PacketMods {
    let mut mods = PacketMods::default();
    if let Some(selection) = client_extensions::modding::grants::environment_packet_mod() {
        mods.select_environment(selection);
    } else if let Some(set) = std::env::var_os(client_extensions::modding::multi::SET_ENV)
        && let Ok(entries) = client_extensions::modding::multi::read_set(Path::new(&set))
        && let Some(selection) = client_extensions::modding::multi::packet_mod_entry(&entries)
    {
        mods.select_environment(selection);
    }
    mods
}

impl PacketMods {
    /// Whether a packet-owned panel still owns input or its closing animation.
    pub(super) fn menu_owns_panel(&self) -> bool {
        self.menu.owned
    }

    /// Current session menu input and output share the packet worker's serialized lane.
    pub(super) fn native_menu(
        &self,
        input: mod_api::native_menu::Input,
    ) -> Option<Arc<client_extensions::modding::packet_relay::MenuOutput>> {
        self.loader.native_menu(input)
    }

    /// Menus remain unavailable until a granted guest starts successfully.
    pub(super) fn native_menu_identity(&self) -> Option<u64> {
        self.loader.native_menu_identity()
    }

    /// Whether snapshot construction is needed for this live packet session.
    pub(super) fn native_frame_available(&self) -> bool {
        self.loader.native_frame_available()
    }

    /// Exchanges native input and local effects without waiting for the packet worker.
    pub(super) fn native_frame(
        &self,
        frame: Option<mod_api::native_frame::Frame>,
    ) -> Option<mod_host::CameraDelta> {
        self.loader.native_frame(frame)
    }

    /// Records a developer-selected source as the fallback when Settings selects no packet mod.
    pub(super) fn select_environment(&mut self, selection: PacketModSelection) {
        self.environment = Some(selection.clone());
        if self.selected.is_none() {
            self.loader.select(Some(selection));
            self.environment_selected = true;
        }
    }
    /// A session relay captures the current source and its revocable authority.
    pub(crate) fn session_relay(&self) -> Option<Arc<dyn protocol::PacketRelay>> {
        self.loader.session_relay()
    }

    /// Whether this component remains admitted under its current grants.
    pub(super) fn admitted(&self, path: &Path) -> bool {
        self.programs
            .get(path)
            .is_some_and(|program| program.authorized)
    }

    /// Adds one immutable prepared source; the list's order chooses the source used by joins.
    pub(super) fn publish(
        &mut self,
        entry: Entry,
        identity: [u8; 32],
        program: Arc<CompiledPacketMod>,
    ) -> Option<Arc<CompiledPacketMod>> {
        self.programs
            .insert(
                entry.component.clone(),
                Prepared {
                    registration: RegisteredHost { entry, identity },
                    program,
                    authorized: true,
                },
            )
            .map(|old| old.program)
    }

    /// Revokes removed grants immediately, independently of compilation and reload progress.
    pub(super) fn authorize(&mut self, entries: &[Entry]) {
        for program in self.programs.values_mut() {
            program.authorized = entries.iter().any(|entry| {
                entry.enabled
                    && entry.component == program.registration.entry.component
                    && entry.grants == program.registration.entry.grants
            });
        }
        self.select(entries);
    }

    /// Reorders accepted sources without starting or compiling a guest on the frame thread.
    pub(super) fn select(&mut self, entries: &[Entry]) {
        let selected = entries
            .iter()
            .filter(|entry| entry.enabled)
            .find_map(|entry| {
                self.programs
                    .get(&entry.component)
                    .filter(|program| program.authorized)
            });
        let identity = selected.map(|program| program.registration.identity);
        if identity == self.selected
            && (identity.is_some() || self.environment_selected == self.environment.is_some())
        {
            return;
        }
        self.selected = identity;
        if let Some(program) = selected {
            self.environment_selected = false;
            self.loader.select_prepared(
                PacketModSelection {
                    path: program.registration.entry.component.clone(),
                    grants: program.registration.entry.grants.clone(),
                },
                Arc::clone(&program.program),
            );
        } else {
            self.loader.select(self.environment.clone());
            self.environment_selected = self.environment.is_some();
        }
    }

    /// Removes disabled sources and moves their executable resources to off-frame retirement.
    pub(super) fn remove_disabled(&mut self, entries: &[Entry]) -> Vec<Arc<CompiledPacketMod>> {
        let removed: Vec<_> = self
            .programs
            .keys()
            .filter(|path| {
                !entries
                    .iter()
                    .any(|entry| entry.enabled && &entry.component == *path)
            })
            .cloned()
            .collect();
        removed
            .into_iter()
            .filter_map(|path| self.programs.remove(&path).map(|program| program.program))
            .collect()
    }

    /// Removes a packet program when its registration changes to a frame component.
    pub(super) fn remove(&mut self, path: &Path) -> Option<Arc<CompiledPacketMod>> {
        self.programs.remove(path).map(|program| program.program)
    }

    /// Source identities join the watcher's existing acknowledgment protocol.
    pub(super) fn identities(&self) -> impl Iterator<Item = (PathBuf, [u8; 32])> + '_ {
        self.programs
            .iter()
            .filter(|(_, program)| program.authorized)
            .map(|(path, program)| (path.clone(), program.registration.identity))
    }
}

/// Installs a packet candidate alongside frame mods, with retirement on the existing worker.
pub(super) fn publish(
    world: &mut World,
    entry: Entry,
    identity: [u8; 32],
    program: Arc<CompiledPacketMod>,
) {
    world.init_resource::<PacketMods>();
    if let Some(old) = world
        .resource_mut::<PacketMods>()
        .publish(entry, identity, program)
    {
        world.resource::<Watcher>().retire(Retired::Packet(old));
    }
}
