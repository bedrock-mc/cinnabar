//! Process-bounded structural observation, never crafting admission or sending.
use super::{CraftingAuthority, InventoryAuthorityEvent};
use protocol::{ContainerIdentity, InventoryEvent};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    io::{self, Write},
    net::SocketAddr,
    sync::{
        Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
};

const MAX_ROWS: usize = 16;
const OUTPUT_BYTES: usize = 16 * 1024;
const META_RECORDS: usize = 64;
const BANKS: [usize; 5] = [2048, 8192, 2048, 2048, 512];
static CONFIGURED: AtomicBool = AtomicBool::new(false);
static RETIRED: AtomicBool = AtomicBool::new(false);
static OWNER: OnceLock<Mutex<Probe>> = OnceLock::new();
#[cfg(test)]
thread_local! {
    static FIXTURE: std::cell::RefCell<Option<std::sync::Arc<FixtureOwner>>> = const { std::cell::RefCell::new(None) };
}
#[cfg(test)]
struct FixtureOwner {
    probe: Mutex<Probe>,
    retired: AtomicBool,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
struct Source {
    #[serde(rename = "k")]
    kind: u8,
    #[serde(rename = "w")]
    window: Option<i32>,
    #[serde(rename = "n")]
    name: Option<u8>,
    #[serde(rename = "d")]
    dynamic: Option<u32>,
    #[serde(rename = "s")]
    slots: [u16; 4],
    #[serde(rename = "m")]
    mask: u8,
    #[serde(rename = "q")]
    sequence: u64,
    #[serde(rename = "e")]
    epoch: Option<u64>,
}

impl Source {
    fn new(
        kind: u8,
        container: ContainerIdentity,
        slots: [u16; 4],
        mask: u8,
        sequence: u64,
    ) -> Self {
        Self {
            kind,
            window: container.window_id,
            name: container.slot_type,
            dynamic: container.dynamic_id,
            slots,
            mask,
            sequence,
            epoch: None,
        }
    }
    fn from_event(event: &InventoryAuthorityEvent, sequence: u64) -> Option<Self> {
        match event {
            InventoryAuthorityEvent::Inventory(InventoryEvent::Slot(slot)) => {
                let container = &slot.identity.container;
                let index = match protocol::project_container_cell(container, slot.identity.slot) {
                    Some(protocol::CanonicalCell::CraftInput(index)) => Some(index),
                    Some(protocol::CanonicalCell::Cursor) => Some(4),
                    _ => protocol::personal_craft_slot_index(container, slot.identity.slot),
                }?;
                Some(Self::new(
                    1,
                    *container,
                    [slot.identity.slot, 0, 0, 0],
                    1 << index,
                    sequence,
                ))
            }
            InventoryAuthorityEvent::Inventory(InventoryEvent::Content(content)) => {
                if let Some(indices) = protocol::personal_craft_content_indices(
                    &content.container,
                    content.slots.len(),
                ) {
                    Some(Self::new(
                        2,
                        content.container,
                        indices.map(|index| index as u16),
                        15,
                        sequence,
                    ))
                } else if content.slots.len() == 1
                    && content.container.slot_type == Some(protocol::CONTAINER_NAME_CURSOR)
                {
                    Some(Self::new(2, content.container, [0; 4], 16, sequence))
                } else {
                    None
                }
            }
            InventoryAuthorityEvent::Inventory(event @ InventoryEvent::Transaction(_)) => {
                let mut mask = 0;
                let mut slots = [0; 4];
                for slot in event.slot_updates() {
                    if let Some(index) = super::projection::transaction_cell_index(slot.identity) {
                        mask |= 1 << index;
                        if let Some(position) = slots.get_mut(index) {
                            *position = slot.identity.slot;
                        }
                    }
                }
                (mask != 0).then(|| {
                    Self::new(
                        3,
                        ContainerIdentity {
                            window_id: Some(protocol::UI_INVENTORY_WINDOW_ID),
                            slot_type: Some(0),
                            dynamic_id: None,
                        },
                        slots,
                        mask,
                        sequence,
                    )
                })
            }
            _ => None,
        }
    }
}

#[derive(Debug)]
struct Probe {
    session: Option<u64>,
    stream: Option<u64>,
    through: u64,
    epoch: u64,
    retired: bool,
    terminal_deferred: bool,
    terminal_pending: bool,
    incomplete: bool,
    pending: [Option<Source>; META_RECORDS],
    origins: [Option<Source>; 5],
    registry_source: Option<u64>,
    bootstrap_registry: bool,
    recipe_source: Option<u64>,
    recipe_applied_available: Option<bool>,
    recipe_revision: Option<(u64, u64)>,
    spent: [bool; 5],
    rows: usize,
    bytes: usize,
    opened: bool,
    closed: bool,
}

impl Probe {
    fn new() -> Self {
        Self {
            session: None,
            stream: None,
            through: 0,
            epoch: 0,
            retired: false,
            terminal_deferred: false,
            terminal_pending: false,
            incomplete: false,
            pending: [None; META_RECORDS],
            origins: [None; 5],
            registry_source: None,
            bootstrap_registry: false,
            recipe_source: None,
            recipe_applied_available: None,
            recipe_revision: None,
            spent: [false; 5],
            rows: 0,
            bytes: 0,
            opened: false,
            closed: false,
        }
    }
    fn bind(&mut self, session: u64, registry: bool) {
        if self.retired {
            return;
        }
        if session == 0 || self.session.is_some() {
            self.retire();
            return;
        }
        self.session = Some(session);
        self.bootstrap_registry = registry;
    }
    fn retire(&mut self) {
        if self.retired {
            return;
        }
        self.retired = true;
        self.incomplete = true;
        self.pending = [None; META_RECORDS];
        self.origins = [None; 5];
        self.terminal();
    }
    fn synchronize(&mut self, session: u64, identity: Option<(u64, u64, Option<u64>)>) {
        if self.retired || self.session.is_none() {
            return;
        }
        let Some((stream, epoch, Some(through))) = identity else {
            self.retire();
            return;
        };
        if self.session != Some(session)
            || stream == 0
            || epoch > through
            || self.stream.is_some_and(|old| old != stream)
            || through < self.through
            || epoch < self.epoch
        {
            self.retire();
            return;
        }
        if epoch != self.epoch {
            self.origins = [None; 5];
        }
        self.stream = Some(stream);
        self.epoch = epoch;
        self.through = through;
    }
    fn stage(&mut self, session: u64, sequence: u64, event: &InventoryAuthorityEvent) {
        if self.retired || self.session != Some(session) {
            return;
        }
        let Some(source) = Source::from_event(event, sequence) else {
            return;
        };
        if self
            .pending
            .iter()
            .flatten()
            .any(|old| old.sequence == sequence)
        {
            self.retire();
            return;
        }
        let Some(entry) = self.pending.iter_mut().find(|entry| entry.is_none()) else {
            self.retire();
            return;
        };
        *entry = Some(source);
    }
    fn applied_cells(&mut self, session: u64, sequence: u64, epoch: u64, mask: u8) {
        if self.retired || self.session != Some(session) {
            return;
        }
        if epoch != self.epoch || sequence > self.through {
            self.retire();
            return;
        }
        let Some(entry) = self
            .pending
            .iter_mut()
            .find(|entry| entry.is_some_and(|source| source.sequence == sequence))
        else {
            self.incomplete = true;
            for (index, origin) in self.origins.iter_mut().enumerate() {
                if mask & (1 << index) != 0 {
                    *origin = None;
                }
            }
            return;
        };
        let mut source = entry.take().expect("matched pending origin");
        if source.mask != mask {
            self.retire();
            return;
        }
        source.epoch = Some(epoch);
        for (index, origin) in self.origins.iter_mut().enumerate() {
            if mask & (1 << index) != 0 {
                *origin = Some(source);
            }
        }
    }
    fn discard(&mut self, sequence: u64) {
        for entry in &mut self.pending {
            if entry.is_some_and(|source| source.sequence == sequence) {
                *entry = None;
            }
        }
    }
    fn clear_cells(&mut self) {
        self.origins = [None; 5];
    }
    fn loss(&mut self) {
        if self.session.is_none() {
            return;
        }
        self.pending = [None; META_RECORDS];
        self.clear_cells();
        self.incomplete = true;
    }
    fn reserve(&mut self, bank: usize) -> Option<usize> {
        let capacity = *BANKS.get(bank)?;
        if self.spent[bank] {
            return None;
        }
        if self.rows >= MAX_ROWS
            || self
                .bytes
                .checked_add(capacity)
                .is_none_or(|bytes| bytes > OUTPUT_BYTES)
        {
            self.incomplete = true;
            return None;
        }
        self.spent[bank] = true;
        self.rows += 1;
        self.bytes += capacity;
        Some(capacity)
    }
    fn emit<T: Serialize>(&mut self, bank: usize, build: impl FnOnce() -> T) {
        let Some(capacity) = self.reserve(bank) else {
            return;
        };
        let mut writer = BoundedWriter {
            bytes: Vec::new(),
            capacity,
        };
        if writer.bytes.try_reserve_exact(capacity).is_err()
            || serde_json::to_writer(&mut writer, &build()).is_err()
        {
            self.incomplete = true;
            return;
        }
        self.write_output(&writer.bytes);
    }
    fn snapshot(&mut self, state: &CraftingAuthority, open: bool) {
        if self.retired
            || self.session != Some(state.session)
            || self.stream != state.stream
            || state.through != Some(self.through)
        {
            return;
        }
        if self
            .origins
            .iter()
            .flatten()
            .any(|source| source.sequence > state.consumed)
            || self
                .registry_source
                .is_some_and(|source| source > state.consumed)
            || self
                .recipe_source
                .is_some_and(|source| source > state.consumed)
        {
            self.retire();
            return;
        }
        if !self.spent[0] {
            // Reserve before all digest/hash/format work; copying fixed metadata is inert.
            self.emit_snapshot(0, state, "BootstrapCommittedSnapshot");
        }
        if open
            && !self.spent[1]
            && state.catalog.is_available()
            && let Some(registry) = state.registry.as_ref()
        {
            let registry = &registry.snapshot;
            let source = self.recipe_source;
            let applied_available = self.recipe_applied_available;
            self.recipe_revision = Some((state.catalog.revision(), registry.revision().get()));
            self.emit(1, || RecipeRow {
                phase: "AdvertisedSupportedRecipes",
                session: state.session,
                epoch: state.epoch,
                through: state.through,
                source,
                applied_available,
                observations: state.catalog.observations(registry),
            });
        }
        if open && !self.opened {
            self.opened = true;
            self.emit_snapshot(2, state, "FirstLocalOpen");
        }
        if !open && self.opened && !self.closed {
            self.closed = true;
            if self.recipe_revision.is_some_and(|revision| {
                Some(revision)
                    != state.registry.as_ref().map(|registry| {
                        (state.catalog.revision(), registry.snapshot.revision().get())
                    })
            }) {
                self.incomplete = true;
            }
            if !self.spent[1] {
                self.emit(1, || MissingRecipeRow {
                    phase: "AdvertisedSupportedRecipesUnavailable",
                    session: state.session,
                    catalog_available: state.catalog.is_available(),
                    registry_available: state.registry.is_some(),
                });
            }
            self.emit_snapshot(3, state, "FinalLocalClose");
            self.terminal();
        }
    }
    fn emit_snapshot(&mut self, bank: usize, state: &CraftingAuthority, phase: &'static str) {
        let Some(capacity) = self.reserve(bank) else {
            return;
        };
        let row = snapshot_row(state, self, phase);
        self.write_reserved(capacity, &row);
    }
    fn write_reserved<T: Serialize>(&mut self, capacity: usize, row: &T) {
        let mut writer = BoundedWriter {
            bytes: Vec::new(),
            capacity,
        };
        if writer.bytes.try_reserve_exact(capacity).is_err()
            || serde_json::to_writer(&mut writer, row).is_err()
        {
            self.incomplete = true;
            return;
        }
        self.write_output(&writer.bytes);
    }
    fn write_output(&mut self, bytes: &[u8]) {
        let mut stderr = io::stderr().lock();
        if stderr
            .write_all(b"crafting-observation=")
            .and_then(|()| stderr.write_all(bytes))
            .and_then(|()| stderr.write_all(b"\n"))
            .is_err()
        {
            self.incomplete = true;
        }
    }
    fn terminal(&mut self) {
        if self.terminal_deferred {
            self.terminal_pending = true;
            return;
        }
        let Some(capacity) = self.reserve(4) else {
            return;
        };
        let complete =
            self.spent[..4].iter().all(|value| *value) && !self.incomplete && !self.retired;
        let row = TerminalRow {
            phase: "Terminal",
            capture_complete: complete,
            incomplete: self.incomplete,
            retired: self.retired,
            opened: self.opened,
            closed: self.closed,
            rows: self.rows,
            reserved_bytes: self.bytes,
        };
        self.write_reserved(capacity, &row);
    }
}

#[derive(Serialize)]
struct CellRow {
    state: u8,
    numbers: Option<[i64; 6]>,
    origin: Option<Source>,
    extra: [bool; 2],
    capacity: Option<u16>,
}
#[derive(Serialize)]
struct SnapshotRow {
    phase: &'static str,
    session: u64,
    stream: Option<u64>,
    epoch: u64,
    through: Option<u64>,
    barrier: u64,
    authority: Option<u8>,
    registry_revision: Option<u64>,
    registry_source: Option<u64>,
    bootstrap_registry: bool,
    catalog_available: bool,
    catalog_revision: u64,
    cells: [CellRow; 5],
    preview: u8,
    incomplete: bool,
}
#[derive(Serialize)]
struct RecipeRow {
    phase: &'static str,
    session: u64,
    epoch: u64,
    through: Option<u64>,
    source: Option<u64>,
    applied_available: Option<bool>,
    observations: protocol::RecipeObservations,
}
#[derive(Serialize)]
struct MissingRecipeRow {
    phase: &'static str,
    session: u64,
    catalog_available: bool,
    registry_available: bool,
}
#[derive(Serialize)]
struct TerminalRow {
    phase: &'static str,
    capture_complete: bool,
    incomplete: bool,
    retired: bool,
    opened: bool,
    closed: bool,
    rows: usize,
    reserved_bytes: usize,
}

fn snapshot_row(state: &CraftingAuthority, probe: &Probe, phase: &'static str) -> SnapshotRow {
    let cells = std::array::from_fn(|index| {
        let stack = if index == 4 {
            state.cursor.as_ref()
        } else {
            state.grid[index].as_ref()
        };
        let Some(stack) = stack else {
            return CellRow {
                state: 0,
                numbers: None,
                origin: None,
                extra: [false; 2],
                capacity: None,
            };
        };
        let stack = &stack.stack;
        CellRow {
            state: if stack.is_empty() { 1 } else { 2 },
            numbers: Some([
                i64::from(stack.network_id),
                i64::from(stack.stack_network_id),
                i64::from(stack.count),
                i64::from(stack.metadata),
                i64::from(u32::from_ne_bytes(stack.block_runtime_id.to_ne_bytes())),
                i64::from(stack.stack_network_id > 0),
            ]),
            origin: probe.origins[index],
            extra: [
                stack.extra_data.is_empty() || stack.extra_data.as_ref() == [0; 10],
                <[u8; 32]>::from(Sha256::digest(&stack.extra_data)) == stack.nbt_digest,
            ],
            capacity: state
                .registry
                .as_ref()
                .and_then(|registry| registry.snapshot.get(stack.network_id))
                .and_then(|entry| entry.negotiated_max_stack_size)
                .map(u16::from),
        }
    });
    SnapshotRow {
        phase,
        session: state.session,
        stream: state.stream,
        epoch: state.epoch,
        through: state.through,
        barrier: state.barrier,
        authority: state.authority.map(|value| {
            if value == protocol::InventoryAuthority::Server {
                1
            } else {
                0
            }
        }),
        registry_revision: state
            .registry
            .as_ref()
            .map(|registry| registry.snapshot.revision().get()),
        registry_source: probe.registry_source,
        bootstrap_registry: probe.bootstrap_registry,
        catalog_available: state.catalog.is_available(),
        catalog_revision: state.catalog.revision(),
        cells,
        preview: match state.preview.as_ref().map(|preview| &preview.value) {
            None => 0,
            Some(crate::ManualCraftMatch::Unavailable) => 1,
            Some(crate::ManualCraftMatch::NoMatch) => 2,
            Some(crate::ManualCraftMatch::Ambiguous) => 3,
            Some(crate::ManualCraftMatch::Unique(_)) => 4,
        },
        incomplete: probe.incomplete,
    }
}

struct BoundedWriter {
    bytes: Vec<u8>,
    capacity: usize,
}
impl Write for BoundedWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.capacity.saturating_sub(self.bytes.len()) {
            return Err(io::ErrorKind::WriteZero.into());
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn try_action(owner: &Mutex<Probe>, retired: &AtomicBool, action: impl FnOnce(&mut Probe)) {
    if retired.load(Ordering::Acquire) {
        if let Ok(mut probe) = owner.try_lock() {
            probe.retire();
            probe.terminal();
        }
        return;
    }
    match owner.try_lock() {
        Ok(mut probe) => {
            probe.terminal_deferred = true;
            action(&mut probe);
            if retired.load(Ordering::Acquire) {
                probe.retire();
            }
            if probe.terminal_pending && retired.swap(true, Ordering::AcqRel) {
                probe.retire();
            }
            probe.terminal_deferred = false;
            if std::mem::take(&mut probe.terminal_pending) {
                probe.terminal();
            }
        }
        Err(_) => {
            retired.store(true, Ordering::Release);
        }
    }
}

fn with_probe(action: impl FnOnce(&mut Probe)) {
    let mut action = Some(action);
    #[cfg(test)]
    if FIXTURE.with(|fixture| {
        let fixture = fixture.borrow();
        let Some(owner) = fixture.as_ref() else {
            return false;
        };
        try_action(
            &owner.probe,
            &owner.retired,
            action.take().expect("one fixture action"),
        );
        true
    }) {
        return;
    }
    let Some(owner) = OWNER.get() else {
        return;
    };
    try_action(owner, &RETIRED, action.take().expect("one process action"));
}

pub(super) fn bootstrap(session: u64, registry: bool) {
    with_probe(|probe| probe.bind(session, registry));
}
pub(super) fn synchronize(session: u64, identity: Option<(u64, u64, Option<u64>)>) {
    with_probe(|probe| probe.synchronize(session, identity));
}
pub(super) fn stage(session: u64, sequence: u64, event: &InventoryAuthorityEvent) {
    with_probe(|probe| probe.stage(session, sequence, event));
}
pub(super) fn cells(session: u64, sequence: u64, epoch: u64, mask: u8) {
    with_probe(|probe| probe.applied_cells(session, sequence, epoch, mask));
}
pub(super) fn discard(sequence: u64) {
    with_probe(|probe| probe.discard(sequence));
}
pub(super) fn clear_cells() {
    with_probe(Probe::clear_cells);
}
pub(super) fn forget_absent_cells(state: &CraftingAuthority) {
    with_probe(|probe| {
        for (index, origin) in probe.origins.iter_mut().enumerate() {
            let absent = if index == 4 {
                state.cursor.is_none()
            } else {
                state.grid[index].is_none()
            };
            if absent {
                *origin = None;
            }
        }
    });
}
pub(super) fn loss() {
    with_probe(Probe::loss);
}
pub(super) fn registry(sequence: u64) {
    with_probe(|probe| {
        probe.registry_source = Some(sequence);
        probe.bootstrap_registry = false;
    });
}
pub(super) fn recipe(sequence: u64, available: bool) {
    with_probe(|probe| {
        probe.recipe_source = Some(sequence);
        probe.recipe_applied_available = Some(available);
    });
}

fn qualified(address: Option<&str>, marker: Option<&str>) -> bool {
    let (Some(address), Some(marker)) = (address, marker) else {
        return false;
    };
    address == marker
        && address
            .parse::<SocketAddr>()
            .is_ok_and(|address| address.ip().is_loopback() && address.port() != 0)
}

fn configure_once(
    configured: &AtomicBool,
    owner: &OnceLock<Mutex<Probe>>,
    address: Option<&str>,
    marker: Option<&str>,
) {
    if configured.swap(true, Ordering::AcqRel) {
        return;
    }
    if qualified(address, marker) {
        let _ = owner.set(Mutex::new(Probe::new()));
    }
}

impl crate::InventorySession {
    /// Enable the bounded observer only when the explicit endpoint and marker agree.
    pub fn configure_crafting_observation(address: Option<&str>, marker: Option<&str>) {
        if CONFIGURED.load(Ordering::Acquire) {
            return;
        }
        configure_once(&CONFIGURED, &OWNER, address, marker);
    }
    /// Retire the bounded observer before connection teardown or transfer.
    pub fn retire_crafting_observation() {
        #[cfg(test)]
        if FIXTURE.with(|fixture| {
            let fixture = fixture.borrow();
            let Some(owner) = fixture.as_ref() else {
                return false;
            };
            owner.retired.store(true, Ordering::Release);
            if let Ok(mut probe) = owner.probe.try_lock() {
                probe.retire();
                probe.terminal();
            }
            true
        }) {
            return;
        }
        RETIRED.store(true, Ordering::Release);
        if let Some(owner) = OWNER.get()
            && let Ok(mut probe) = owner.try_lock()
        {
            probe.retire();
            probe.terminal();
        }
    }
    /// Sample a completed drain with the caller's screen visibility.
    pub fn sample_crafting_observation(&self, inventory_open: bool) {
        with_probe(|probe| probe.snapshot(&self.crafting_authority, inventory_open));
    }
}

#[cfg(test)]
mod tests;
