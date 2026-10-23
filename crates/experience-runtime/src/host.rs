//! The host side of the `server` world: generated bindings, per-store state and the imports.
//! The current WIT is `crates/experience-sdk/wit/server/server.wit`; [`v0_1`], [`v0_2`],
//! [`v0_3`], [`v0_4`] and [`v0_5`] keep the worlds that older artifacts target.

use experience_sdk::artifact::SERVER_WASM;
use std::fmt;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use wasmtime::component::{Component, HasSelf, Linker, Resource, ResourceTable};
use wasmtime::{
    Engine, ResourceLimiter, Store, StoreLimits, StoreLimitsBuilder, Trap, UpdateDeadline,
};

use crate::limits::{
    EPOCH_PERIOD, MAX_CORE_INSTANCES, MAX_FRAME_BYTES, MAX_LOG_BYTES, MAX_LOGS, MAX_MEMORIES,
    MAX_MEMORY_BYTES, MAX_TABLE_ELEMENTS,
};

pub(crate) mod v0_1;
pub(crate) mod v0_2;
pub(crate) mod v0_3;
pub(crate) mod v0_4;
pub(crate) mod v0_5;

wasmtime::component::bindgen!({
    path: "../experience-sdk/wit/server",
    world: "server",
    imports: { default: trappable },
    with: { "cinnabar:experience-server/world-access/callback": crate::callback::CallbackRes },
});

use cinnabar::experience_server::types::{
    BlockDef, BlockState, BlockType, CallbackInfo, Crafted, Inventory, ItemKey, LogLevel, Network,
    NewStack, PlacementStates, WorldError,
};
use cinnabar::experience_server::{diagnostics, types, world_access};

use crate::callback::CallbackRes;

/// The WIT that `bindgen!` reads; its `package` line is the one source of the world's name and
/// version.
const WIT: &str = include_str!("../../experience-sdk/wit/server/server.wit");

/// A server WIT version that the runtime implements; an artifact's manifest `api` selects it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Api {
    /// The 0.1 world: no client messages.
    V0_1,
    /// The 0.2 world: client messages of scalars, and no epoch.
    V0_2,
    /// The 0.3 world: client messages and epochs without a focus.
    V0_3,
    /// The 0.4 world: plain cube blocks, and the player's focus.
    V0_4,
    /// The 0.5 world: no crafting recipes or scheduled updates.
    V0_5,
    /// The current world.
    V0_6,
}

impl Api {
    pub(crate) const ALL: [Api; 6] = [
        Api::V0_1,
        Api::V0_2,
        Api::V0_3,
        Api::V0_4,
        Api::V0_5,
        Api::V0_6,
    ];

    /// Whether this world's client messages and epochs take their player's focus.
    pub(crate) fn focus(self) -> bool {
        match self {
            Api::V0_1 | Api::V0_2 | Api::V0_3 => false,
            Api::V0_4 | Api::V0_5 | Api::V0_6 => true,
        }
    }

    /// The version whose manifest `api` is `api`.
    pub(crate) fn of(api: &str) -> Option<Api> {
        Self::ALL.into_iter().find(|version| version.api() == api)
    }

    /// The WIT package, `cinnabar:experience-server@<major.minor.patch>`.
    pub(crate) fn package(self) -> &'static str {
        let wit = match self {
            Api::V0_1 => v0_1::WIT,
            Api::V0_2 => v0_2::WIT,
            Api::V0_3 => v0_3::WIT,
            Api::V0_4 => v0_4::WIT,
            Api::V0_5 => v0_5::WIT,
            Api::V0_6 => WIT,
        };
        wit.lines()
            .find_map(|line| line.strip_prefix("package ")?.strip_suffix(';'))
            .expect("server.wit declares its package")
    }

    /// The manifest `api`: the WIT package's `major.minor`.
    pub(crate) fn api(self) -> &'static str {
        let (_, version) = self
            .package()
            .rsplit_once('@')
            .expect("server.wit's package is versioned");
        version.rsplit_once('.').map_or(version, |(api, _)| api)
    }
}

/// A guest component pre-linked against exactly the imports of its version's `server` world.
pub(crate) enum Pre {
    V0_1(v0_1::ServerPre<HostState>),
    V0_2(v0_2::ServerPre<HostState>),
    V0_3(v0_3::ServerPre<HostState>),
    V0_4(v0_4::ServerPre<HostState>),
    V0_5(v0_5::ServerPre<HostState>),
    V0_6(ServerPre<HostState>),
}

impl Pre {
    pub(crate) fn link(engine: &Engine, component: &Component, api: Api) -> Result<Self> {
        let mut linker = Linker::new(engine);
        Ok(match api {
            Api::V0_1 => {
                v0_1::Server::add_to_linker::<_, HasSelf<_>>(&mut linker, |state| state)?;
                Self::V0_1(v0_1::ServerPre::new(linker.instantiate_pre(component)?)?)
            }
            Api::V0_2 => {
                v0_2::Server::add_to_linker::<_, HasSelf<_>>(&mut linker, |state| state)?;
                Self::V0_2(v0_2::ServerPre::new(linker.instantiate_pre(component)?)?)
            }
            Api::V0_3 => {
                v0_3::Server::add_to_linker::<_, HasSelf<_>>(&mut linker, |state| state)?;
                Self::V0_3(v0_3::ServerPre::new(linker.instantiate_pre(component)?)?)
            }
            Api::V0_4 => {
                v0_4::Server::add_to_linker::<_, HasSelf<_>>(&mut linker, |state| state)?;
                Self::V0_4(v0_4::ServerPre::new(linker.instantiate_pre(component)?)?)
            }
            Api::V0_5 => {
                v0_5::Server::add_to_linker::<_, HasSelf<_>>(&mut linker, |state| state)?;
                Self::V0_5(v0_5::ServerPre::new(linker.instantiate_pre(component)?)?)
            }
            Api::V0_6 => {
                Server::add_to_linker::<_, HasSelf<_>>(&mut linker, |state| state)?;
                Self::V0_6(ServerPre::new(linker.instantiate_pre(component)?)?)
            }
        })
    }

    /// Runs `register` on a fresh instance in `store`. Worlds before 0.5 declare plain blocks,
    /// which become cube block types and no items.
    pub(crate) fn register(
        &self,
        store: &mut Store<HostState>,
    ) -> Result<Result<Registration, GuestError>> {
        let instantiating = || format!("instantiating {SERVER_WASM}");
        let plain = |result: Result<Vec<BlockDef>, GuestError>| result.map(cube_registration);
        let result = match self {
            Self::V0_1(pre) => pre
                .instantiate(&mut *store)
                .with_context(instantiating)?
                .call_register(store)
                .map(plain),
            Self::V0_2(pre) => pre
                .instantiate(&mut *store)
                .with_context(instantiating)?
                .call_register(store)
                .map(plain),
            Self::V0_3(pre) => pre
                .instantiate(&mut *store)
                .with_context(instantiating)?
                .call_register(store)
                .map(plain),
            Self::V0_4(pre) => pre
                .instantiate(&mut *store)
                .with_context(instantiating)?
                .call_register(store)
                .map(plain),
            Self::V0_5(pre) => pre
                .instantiate(&mut *store)
                .with_context(instantiating)?
                .call_register(store),
            Self::V0_6(pre) => pre
                .instantiate(&mut *store)
                .with_context(instantiating)?
                .call_register(store),
        };
        result.context("register trapped")
    }
}

/// The 0.5 registration of plain blocks: each a stateless cube, and no items.
fn cube_registration(defs: Vec<BlockDef>) -> Registration {
    Registration {
        blocks: defs
            .into_iter()
            .map(|def| BlockType {
                def,
                states: Vec::new(),
                placement: PlacementStates::empty(),
                visual: None,
                permutations: Vec::new(),
                network: false,
            })
            .collect(),
        items: Vec::new(),
    }
}

/// A store limit or a per-callback cap was exceeded. A trap with this error fails the callback
/// as `limit`.
#[derive(Debug)]
pub(crate) struct LimitExceeded(pub(crate) String);

impl fmt::Display for LimitExceeded {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for LimitExceeded {}

/// The data of one store, which serves one `register` or one callback.
pub(crate) struct HostState {
    limits: Limiter,
    logs: Logs,
    /// Holds the callback the guest borrows; `register` runs without one.
    pub(crate) table: ResourceTable,
}

impl HostState {
    /// A fresh store for Experience `id` with the store limits, `fuel`, and an epoch deadline
    /// `deadline` from now.
    pub(crate) fn store(
        engine: &Engine,
        id: &str,
        fuel: u64,
        deadline: Duration,
    ) -> Result<Store<Self>> {
        let limits = Limiter(
            StoreLimitsBuilder::new()
                .memory_size(MAX_MEMORY_BYTES)
                .memories(MAX_MEMORIES)
                .table_elements(MAX_TABLE_ELEMENTS)
                .instances(MAX_CORE_INSTANCES)
                .trap_on_grow_failure(true)
                .build(),
        );
        let logs = Logs {
            id: id.to_owned(),
            kept: 0,
            dropped: 0,
            truncated: 0,
        };
        let state = Self {
            limits,
            logs,
            table: ResourceTable::new(),
        };
        let mut store = Store::new(engine, state);
        store.limiter(|state| &mut state.limits);
        // Guest-to-host copies stay within the same ceiling as one protocol frame.
        store.set_hostcall_fuel(MAX_FRAME_BYTES);
        store.set_fuel(fuel)?;
        let start = Instant::now();
        set_deadline(&mut store, deadline, move || start.elapsed());
        Ok(store)
    }
}

/// Counts only this store's elapsed work when delayed epoch ticks reach its deadline.
fn set_deadline<T>(
    store: &mut Store<T>,
    deadline: Duration,
    mut elapsed: impl FnMut() -> Duration + Send + Sync + 'static,
) {
    store.epoch_deadline_callback(move |_| {
        let remaining = deadline.saturating_sub(elapsed());
        if remaining.is_zero() {
            return Err(Trap::Interrupt.into());
        }
        Ok(UpdateDeadline::Continue(epoch_ticks(remaining)))
    });
    store.set_epoch_deadline(epoch_ticks(deadline));
}

/// Whole epoch periods in `deadline`, rounded up.
fn epoch_ticks(deadline: Duration) -> u64 {
    let ticks = deadline.as_nanos().div_ceil(EPOCH_PERIOD.as_nanos());
    u64::try_from(ticks).unwrap_or(u64::MAX)
}

/// The store limits from [`crate::limits`]. Growth they refuse traps with [`LimitExceeded`].
struct Limiter(StoreLimits);

impl ResourceLimiter for Limiter {
    fn memory_growing(
        &mut self,
        current: usize,
        desired: usize,
        maximum: Option<usize>,
    ) -> Result<bool> {
        self.0
            .memory_growing(current, desired, maximum)
            .map_err(exceeded)
    }

    fn memory_grow_failed(&mut self, error: anyhow::Error) -> Result<()> {
        self.0.memory_grow_failed(error).map_err(exceeded)
    }

    fn table_growing(
        &mut self,
        current: usize,
        desired: usize,
        maximum: Option<usize>,
    ) -> Result<bool> {
        self.0
            .table_growing(current, desired, maximum)
            .map_err(exceeded)
    }

    fn table_grow_failed(&mut self, error: anyhow::Error) -> Result<()> {
        self.0.table_grow_failed(error).map_err(exceeded)
    }

    fn instances(&self) -> usize {
        self.0.instances()
    }

    fn tables(&self) -> usize {
        self.0.tables()
    }

    fn memories(&self) -> usize {
        self.0.memories()
    }
}

fn exceeded(error: anyhow::Error) -> anyhow::Error {
    LimitExceeded(format!("{error:#}")).into()
}

/// Guest log lines of one store. Logs are not host calls: the first [`MAX_LOGS`] go to stderr,
/// each cut to [`MAX_LOG_BYTES`] at a char boundary, and later ones are dropped. Drops and cuts
/// are counted on stderr when the store ends.
struct Logs {
    id: String,
    kept: usize,
    dropped: usize,
    truncated: usize,
}

impl Logs {
    #[expect(
        clippy::print_stderr,
        reason = "guest logs reach the supervisor as `log <id> <level> ...` stderr lines"
    )]
    fn push(&mut self, level: LogLevel, text: &str) {
        if self.kept == MAX_LOGS {
            self.dropped += 1;
            return;
        }
        self.kept += 1;
        let end = text.floor_char_boundary(MAX_LOG_BYTES);
        if end < text.len() {
            self.truncated += 1;
        }
        let level = match level {
            LogLevel::Debug => "debug",
            LogLevel::Info => "info",
            LogLevel::Warn => "warn",
            LogLevel::Error => "error",
        };
        // Debug formatting escapes control characters, so one log stays one stderr line.
        eprintln!("log {} {level} {:?}", self.id, &text[..end]);
    }
}

impl Drop for Logs {
    #[expect(
        clippy::print_stderr,
        reason = "guest log drops reach the supervisor as a stderr line"
    )]
    fn drop(&mut self) {
        if self.dropped > 0 || self.truncated > 0 {
            eprintln!(
                "log {} dropped {} truncated {}",
                self.id, self.dropped, self.truncated
            );
        }
    }
}

impl types::Host for HostState {}

impl diagnostics::Host for HostState {
    fn log(&mut self, level: LogLevel, text: String) -> Result<()> {
        self.logs.push(level, &text);
        Ok(())
    }
}

impl world_access::Host for HostState {}

/// Each method acts on the callback that `ctx` names; see [`CallbackRes`] for the rules.
impl world_access::HostCallback for HostState {
    fn info(&mut self, ctx: Resource<CallbackRes>) -> Result<CallbackInfo> {
        self.table.get_mut(&ctx)?.info()
    }

    fn get_block(
        &mut self,
        ctx: Resource<CallbackRes>,
        pos: BlockPos,
    ) -> Result<Result<String, WorldError>> {
        self.table.get_mut(&ctx)?.get_block(pos.into())
    }

    fn set_block(
        &mut self,
        ctx: Resource<CallbackRes>,
        pos: BlockPos,
        id: String,
    ) -> Result<Result<(), WorldError>> {
        self.table.get_mut(&ctx)?.set_block(pos.into(), id)
    }

    fn block_data(
        &mut self,
        ctx: Resource<CallbackRes>,
        pos: BlockPos,
    ) -> Result<Result<Option<Vec<u8>>, WorldError>> {
        self.table.get_mut(&ctx)?.block_data(pos.into())
    }

    fn set_block_data(
        &mut self,
        ctx: Resource<CallbackRes>,
        pos: BlockPos,
        data: Option<Vec<u8>>,
    ) -> Result<Result<(), WorldError>> {
        self.table.get_mut(&ctx)?.set_block_data(pos.into(), data)
    }

    fn tell(
        &mut self,
        ctx: Resource<CallbackRes>,
        player: String,
        text: String,
    ) -> Result<Result<(), WorldError>> {
        self.table.get_mut(&ctx)?.tell(player, text)
    }

    fn send_client(
        &mut self,
        ctx: Resource<CallbackRes>,
        player: String,
        channel: String,
        schema: u16,
        payload: Vec<ValueNode>,
    ) -> Result<Result<(), WorldError>> {
        self.table
            .get_mut(&ctx)?
            .send_client(player, channel, schema, payload)
    }

    fn focus(&mut self, ctx: Resource<CallbackRes>) -> Result<Option<BlockPos>> {
        Ok(self.table.get_mut(&ctx)?.focus()?.map(Into::into))
    }

    fn block_states(
        &mut self,
        ctx: Resource<CallbackRes>,
        pos: BlockPos,
    ) -> Result<Result<Vec<BlockState>, WorldError>> {
        let states = self.table.get_mut(&ctx)?.block_states(pos.into())?;
        Ok(states.map(|states| states.into_iter().map(Into::into).collect()))
    }

    fn set_block_state(
        &mut self,
        ctx: Resource<CallbackRes>,
        pos: BlockPos,
        states: Vec<BlockState>,
    ) -> Result<Result<(), WorldError>> {
        let states = states.into_iter().map(Into::into).collect();
        self.table
            .get_mut(&ctx)?
            .set_block_state(pos.into(), states)
    }

    fn network(
        &mut self,
        ctx: Resource<CallbackRes>,
    ) -> Result<Result<Option<Network>, WorldError>> {
        let network = self.table.get_mut(&ctx)?.network()?;
        Ok(network.map(|network| {
            network.map(|network| Network {
                blocks: network.blocks.into_iter().map(Into::into).collect(),
                truncated: network.truncated,
            })
        }))
    }

    fn inventory(&mut self, ctx: Resource<CallbackRes>) -> Result<Result<Inventory, WorldError>> {
        let inventory = self.table.get_mut(&ctx)?.inventory()?;
        Ok(inventory.map(Into::into))
    }

    fn set_slot(
        &mut self,
        ctx: Resource<CallbackRes>,
        slot: u32,
        stack: Option<NewStack>,
    ) -> Result<Result<(), WorldError>> {
        self.table
            .get_mut(&ctx)?
            .set_slot(slot, stack.map(Into::into))
    }

    fn drop_item(
        &mut self,
        ctx: Resource<CallbackRes>,
        pos: BlockPos,
        stack: NewStack,
    ) -> Result<Result<(), WorldError>> {
        self.table
            .get_mut(&ctx)?
            .drop_item(pos.into(), stack.into())
    }

    fn crafting_result(
        &mut self,
        ctx: Resource<CallbackRes>,
        grid: Vec<Option<ItemKey>>,
    ) -> Result<Option<Crafted>> {
        self.table.get_mut(&ctx)?.crafting_result(&grid)
    }

    fn schedule_update(
        &mut self,
        ctx: Resource<CallbackRes>,
        pos: BlockPos,
        ticks: u32,
    ) -> Result<Result<(), WorldError>> {
        self.table.get_mut(&ctx)?.schedule_update(pos.into(), ticks)
    }

    /// Only reachable for an owned handle, and the guest is only ever lent a callback.
    fn drop(&mut self, _: Resource<CallbackRes>) -> Result<()> {
        bail!("the guest dropped a callback it cannot own")
    }
}

#[cfg(test)]
mod deadline_tests {
    use super::*;
    use crate::limits::{CALLBACK_DEADLINE, REGISTER_DEADLINE, REGISTER_FUEL};
    use std::sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    };
    use wasmtime::{Instance, Module};

    #[test]
    fn idle_catch_up_preserves_work_until_its_elapsed_deadline() {
        let (engine, _ticker) = crate::load::engine().unwrap();
        let module = Module::new(&engine, "(module (func (export \"idle\")))").unwrap();
        for deadline in [CALLBACK_DEADLINE, REGISTER_DEADLINE] {
            let mut store = HostState::store(&engine, "catch-up", REGISTER_FUEL, deadline).unwrap();
            let elapsed = Arc::new(AtomicU64::new(0));
            let clock = Arc::clone(&elapsed);
            set_deadline(&mut store, deadline, move || {
                Duration::from_nanos(clock.load(Ordering::Acquire))
            });
            let instance = Instance::new(&mut store, &module, &[]).unwrap();
            let idle = instance
                .get_typed_func::<(), ()>(&mut store, "idle")
                .unwrap();
            for elapsed_time in [Duration::ZERO, deadline - Duration::from_nanos(1), deadline] {
                elapsed.store(elapsed_time.as_nanos() as u64, Ordering::Release);
                for _ in 0..=epoch_ticks(REGISTER_DEADLINE) {
                    engine.increment_epoch();
                }
                let result = idle.call(&mut store, ());
                if elapsed_time < deadline {
                    result.unwrap();
                } else {
                    let error = result.unwrap_err();
                    assert_eq!(error.downcast_ref::<Trap>(), Some(&Trap::Interrupt));
                }
            }
        }
    }
}
