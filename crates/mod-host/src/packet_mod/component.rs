//! Packet mods built as components of the `packet-mod` world, called through generated bindings.

use super::{
    Direction, Guest, Outbound, PACKET_MOD_LOAD_DEADLINE, PACKET_MOD_TRANSFER_BYTES,
    SessionIdentity, SessionPack, cache, limits::PacketLimits, wasi,
};
use crate::{ModGrants, execution};
use anyhow::{Context, Result};
use std::{path::Path, time::Duration};
use wasmtime::{
    Engine, Store,
    component::{Component, Linker, ResourceTable},
};
use wasmtime_wasi::{WasiCtx, WasiCtxView, WasiView};

pub(super) mod bindings {
    wasmtime::component::bindgen!({
        path: [
            "../experience-sdk/wit/client/deps/server-experience",
            "../experience-sdk/wit/session",
            "../mod-api/wit",
        ],
        world: "cinnabar:extension/packet-mod@0.1.0",
        additional_derives: [PartialEq, Eq],
    });
}

/// Store data: the locked-down WASI context, its resources and the memory limiter.
struct State {
    wasi: WasiCtx,
    table: ResourceTable,
    limits: PacketLimits,
    log: wasi::LogBudget,
}

impl WasiView for State {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView {
            ctx: &mut self.wasi,
            table: &mut self.table,
        }
    }
}

/// A compiled component linked against WASI preview 2 only with the `wasi` grant.
pub(super) struct Program {
    pre: bindings::PacketModPre<State>,
}

impl Program {
    /// Compiles or reuses the component and checks its imports and exports.
    pub(super) fn new(
        engine: &Engine,
        bytes: &[u8],
        grants: &ModGrants,
        cache: Option<&Path>,
    ) -> Result<Self> {
        let component: Component = cache::load(engine, bytes, cache)?;
        let mut linker = Linker::new(engine);
        if grants.wasi {
            wasmtime_wasi::p2::add_to_linker_sync(&mut linker)?;
        }
        let pre = linker
            .instantiate_pre(&component)
            .context("link packet mod")?;
        let pre = bindings::PacketModPre::new(pre).context("packet mod exports")?;
        Ok(Self { pre })
    }

    /// Instantiates the component; its start function runs under the load deadline.
    pub(super) fn instantiate(&self, engine: &Engine) -> Result<Box<dyn Guest>> {
        let log = wasi::LogBudget::default();
        let mut store = Store::new(
            engine,
            State {
                wasi: wasi::builder(&log).build(),
                table: ResourceTable::new(),
                limits: PacketLimits::default(),
                log,
            },
        );
        store.limiter(|state| &mut state.limits);
        store.set_hostcall_fuel(PACKET_MOD_TRANSFER_BYTES);
        execution::arm_deadline(&mut store, PACKET_MOD_LOAD_DEADLINE);
        let bindings = self.pre.instantiate(&mut store)?;
        Ok(Box::new(ComponentGuest { store, bindings }))
    }
}

struct ComponentGuest {
    store: Store<State>,
    bindings: bindings::PacketMod,
}

impl Guest for ComponentGuest {
    fn arm(&mut self, deadline: Duration) {
        self.store.data().log.refill();
        execution::arm_deadline(&mut self.store, deadline);
    }

    fn start(
        &mut self,
        identity: &SessionIdentity,
        client_data: &str,
        startup: &[u8],
    ) -> Result<Result<Vec<SessionPack>, String>> {
        self.bindings
            .call_start(&mut self.store, identity, client_data, startup)
    }

    fn on_batch(
        &mut self,
        direction: Direction,
        batch: &[u8],
    ) -> Result<Result<Option<Vec<u8>>, String>> {
        self.bindings
            .call_on_batch(&mut self.store, direction, batch)
    }

    fn tick(&mut self) -> Result<Result<Outbound, String>> {
        self.bindings.call_tick(&mut self.store)
    }

    fn stop(&mut self) -> Result<()> {
        self.bindings.call_stop(&mut self.store)
    }

    fn peak_memory(&self) -> usize {
        self.store.data().limits.peak()
    }
}
