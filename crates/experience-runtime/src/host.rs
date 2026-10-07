//! The host side of the `server` world: generated bindings, per-store state and the imports.
//! The current WIT is `crates/experience-sdk/wit/server/server.wit`; [`v0_1`], [`v0_2`] and
//! [`v0_3`] keep the worlds that older artifacts target.

use std::fmt;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use wasmtime::component::{Component, HasSelf, Linker, Resource, ResourceTable};
use wasmtime::{Engine, ResourceLimiter, Store, StoreLimits, StoreLimitsBuilder};

use crate::limits::{
    EPOCH_PERIOD, MAX_CORE_INSTANCES, MAX_LOG_BYTES, MAX_LOGS, MAX_MEMORIES, MAX_MEMORY_BYTES,
    MAX_TABLE_ELEMENTS,
};
use crate::manifest::SERVER_WASM;

pub(crate) mod v0_1;
pub(crate) mod v0_2;
pub(crate) mod v0_3;

wasmtime::component::bindgen!({
    path: "../experience-sdk/wit/server",
    world: "server",
    imports: { default: trappable },
    with: { "cinnabar:experience-server/world-access/callback": crate::callback::CallbackRes },
});

use cinnabar::experience_server::types::{CallbackInfo, LogLevel, WorldError};
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
    /// The current world.
    V0_4,
}

impl Api {
    pub(crate) const ALL: [Api; 4] = [Api::V0_1, Api::V0_2, Api::V0_3, Api::V0_4];

    /// Whether this world's client messages and epochs take their player's focus.
    pub(crate) fn focus(self) -> bool {
        match self {
            Api::V0_1 | Api::V0_2 | Api::V0_3 => false,
            Api::V0_4 => true,
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
            Api::V0_4 => WIT,
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
    V0_4(ServerPre<HostState>),
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
                Server::add_to_linker::<_, HasSelf<_>>(&mut linker, |state| state)?;
                Self::V0_4(ServerPre::new(linker.instantiate_pre(component)?)?)
            }
        })
    }

    /// Runs `register` on a fresh instance in `store`.
    pub(crate) fn register(
        &self,
        store: &mut Store<HostState>,
    ) -> Result<Result<Vec<BlockDef>, GuestError>> {
        let instantiating = || format!("instantiating {SERVER_WASM}");
        let result = match self {
            Self::V0_1(pre) => pre
                .instantiate(&mut *store)
                .with_context(instantiating)?
                .call_register(store),
            Self::V0_2(pre) => pre
                .instantiate(&mut *store)
                .with_context(instantiating)?
                .call_register(store),
            Self::V0_3(pre) => pre
                .instantiate(&mut *store)
                .with_context(instantiating)?
                .call_register(store),
            Self::V0_4(pre) => pre
                .instantiate(&mut *store)
                .with_context(instantiating)?
                .call_register(store),
        };
        result.context("register trapped")
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
        store.set_fuel(fuel)?;
        store.set_epoch_deadline(epoch_ticks(deadline));
        Ok(store)
    }
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

    /// Only reachable for an owned handle, and the guest is only ever lent a callback.
    fn drop(&mut self, _: Resource<CallbackRes>) -> Result<()> {
        bail!("the guest dropped a callback it cannot own")
    }
}
