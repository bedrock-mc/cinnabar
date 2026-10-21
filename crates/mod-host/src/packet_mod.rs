//! Trusted packet mods: one component relays every session packet under wall-clock limits
//! instead of the per-frame fuel budget. Any trap, error or missed deadline quarantines the
//! instance, after which every call passes its batch through unchanged.

mod adapt;
mod cache;
pub mod capture;
mod component;
mod core;
mod limits;
pub(crate) mod native_capabilities;
mod native_frame;
pub mod native_menu;
mod native_render;
mod wasi;

pub use adapt::packet_mod_component;
pub use component::bindings::cinnabar::extension::packet_relay::{
    Direction, Outbound, SessionIdentity, SessionPack,
};

use crate::{ModGrants, execution, local_files};
use anyhow::{Context, Result, anyhow, ensure};
use std::{path::Path, sync::Arc, time::Duration};
use wasmtime::Engine;

/// Largest packet-mod component or module, which may carry a whole language runtime.
pub const MAX_PACKET_MOD_BYTES: usize = 256 * 1024 * 1024;
/// Largest linear memory a packet mod may grow.
pub const PACKET_MOD_MEMORY_BYTES: usize = 1024 * 1024 * 1024;
/// Per-call returned data budget; core modules also charge each native session pack entry.
pub const PACKET_MOD_TRANSFER_BYTES: usize = 256 * 1024 * 1024;
/// Wall-clock budget for instantiation, including the guest runtime's own initialization.
pub const PACKET_MOD_LOAD_DEADLINE: Duration = Duration::from_secs(60);
/// Wall-clock budget for `start`, which reads the whole startup sequence once per session.
pub const PACKET_MOD_START_DEADLINE: Duration = Duration::from_secs(5);
/// Wall-clock budget for one relayed batch, tick or stop.
pub const PACKET_MOD_CALL_DEADLINE: Duration = Duration::from_millis(250);
/// Characters of a guest error kept in the quarantine log.
const MAX_ERROR_CHARS: usize = 256;

/// One instance's calls, whichever binary form implements the `packet-mod` world.
trait Guest: Send {
    /// Refills the output allowance and gives the next call `deadline`.
    fn arm(&mut self, deadline: Duration);
    fn start(
        &mut self,
        identity: &SessionIdentity,
        client_data: &str,
        startup: &[u8],
    ) -> Result<Result<Vec<SessionPack>, String>>;
    fn on_batch(
        &mut self,
        direction: Direction,
        batch: &[u8],
    ) -> Result<Result<Option<Vec<u8>>, String>>;
    fn tick(&mut self) -> Result<Result<Outbound, String>>;
    fn stop(&mut self) -> Result<()>;
    /// Whether startup negotiated the optional input-owned native frame capability.
    fn has_native_frame(&self) -> bool {
        false
    }
    /// Runs the optional core native callback with bounded UTF-8 JSON.
    fn frame(&mut self, _json: &[u8]) -> Result<Result<Option<Vec<u8>>, String>> {
        Ok(Ok(None))
    }
    /// Whether the optional general camera callback was negotiated.
    fn has_native_camera(&self) -> bool {
        false
    }
    /// Whether frame observations may include remote player positions.
    fn native_camera_players(&self) -> bool {
        self.has_native_frame()
    }
    /// Exchanges bounded generic camera effects, independently of legacy rotation support.
    fn camera_frame(&mut self, _json: &[u8]) -> Result<Result<Option<Vec<u8>>, String>> {
        Ok(Ok(None))
    }
    /// Whether startup admitted the independent drawing callback.
    /// Consent bits for optional actor and block output in the drawing callback.
    fn native_render_capabilities(&self) -> u32 {
        0
    }

    fn has_native_render(&self) -> bool {
        false
    }
    /// Exchanges a bounded desired world scene.
    fn render_frame(&mut self, _json: &[u8]) -> Result<Result<Option<Vec<u8>>, String>> {
        Ok(Ok(None))
    }
    /// Whether startup negotiated the optional focused menu capability.
    fn has_native_menu(&self) -> bool {
        false
    }
    /// Exchanges bounded retained menu state through the session's serial worker.
    fn menu(&mut self, _json: &[u8]) -> Result<Result<Option<Vec<u8>>, String>> {
        Ok(Ok(None))
    }
    /// The largest linear memory the guest has requested.
    fn peak_memory(&self) -> usize;
}

/// The two accepted binary forms: a `packet-mod` component, or a WASI preview 1 reactor core
/// module exporting the world's canonical ABI.
#[derive(Clone)]
enum Program {
    Component(component::Program),
    Core(core::Program),
}

/// A compiled and linked packet mod; each session instantiates it afresh.
#[derive(Clone)]
pub struct CompiledPacketMod {
    engine: Engine,
    ticker: Arc<execution::EpochTicker>,
    program: Program,
}

impl CompiledPacketMod {
    /// Reads a bounded local file without following a linked final entry, then compiles it,
    /// reusing an admitted program retained in this process when available.
    pub fn load(path: &Path, grants: &ModGrants) -> Result<Self> {
        let bytes = local_files::read(path, MAX_PACKET_MOD_BYTES)
            .with_context(|| format!("read packet mod {}", path.display()))?;
        Self::compile(&bytes, grants)
    }

    /// Compiles and links a component or core module under `grants`; WASI is linked only with
    /// the `wasi` grant. Compiled programs remain in host memory and never load native files.
    pub fn compile(bytes: &[u8], grants: &ModGrants) -> Result<Self> {
        ensure!(grants.packets, "packet mods need the packets grant");
        ensure!(
            bytes.len() <= MAX_PACKET_MOD_BYTES,
            "packet mod exceeds {MAX_PACKET_MOD_BYTES} bytes"
        );
        grants.validate()?;
        cache::load(bytes, grants, || {
            let (engine, ticker) = execution::trusted_engine()?;
            let (program, image_bytes) = if wasmparser::Parser::is_component(bytes) {
                let (program, size) = component::Program::new(&engine, bytes, grants)?;
                (Program::Component(program), size)
            } else {
                let (program, size) = core::Program::new(&engine, bytes, grants)?;
                (Program::Core(program), size)
            };
            Ok((
                Self {
                    engine,
                    ticker: Arc::new(ticker),
                    program,
                },
                image_bytes,
            ))
        })
    }

    /// Instantiates a fresh session instance, running the guest's initialization under the load
    /// deadline. Calls must not run on a Tokio runtime thread: WASI blocks on its own runtime.
    pub fn instantiate(&self) -> Result<PacketMod> {
        let guest = match &self.program {
            Program::Component(program) => program.instantiate(&self.engine),
            Program::Core(program) => program.instantiate(&self.engine),
        }
        .context("initialize packet mod")?;
        Ok(PacketMod {
            guest,
            status: Status::Active,
            menu_input_extensions: mod_api::native_menu::InputExtensions::default(),
            call_deadline: PACKET_MOD_CALL_DEADLINE,
            _ticker: Arc::clone(&self.ticker),
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Status {
    Active,
    Stopped,
    Quarantined,
}

/// One session's packet-mod instance. Every call is synchronous and bounded by a deadline.
pub struct PacketMod {
    guest: Box<dyn Guest>,
    status: Status,
    menu_input_extensions: mod_api::native_menu::InputExtensions,
    call_deadline: Duration,
    _ticker: Arc<execution::EpochTicker>,
}

impl PacketMod {
    /// Begins the session and returns the packs the mod adds; none once inactive.
    pub fn start(
        &mut self,
        identity: &SessionIdentity,
        client_data: &str,
        startup: &[u8],
    ) -> Result<Vec<SessionPack>> {
        self.call("start", PACKET_MOD_START_DEADLINE, |guest| {
            guest.start(identity, client_data, startup)
        })
        .map(Option::unwrap_or_default)
    }

    /// Relays one batch body: `None` forwards it unchanged, an empty replacement drops it.
    pub fn on_batch(&mut self, direction: Direction, batch: &[u8]) -> Result<Option<Vec<u8>>> {
        let deadline = self.call_deadline;
        self.call("on-batch", deadline, |guest| {
            guest.on_batch(direction, batch)
        })
        .map(Option::flatten)
    }

    /// Initial server bodies initialize world registries under the join deadline, before play.
    pub fn on_startup_batch(&mut self, batch: &[u8]) -> Result<Option<Vec<u8>>> {
        self.call("startup-batch", PACKET_MOD_START_DEADLINE, |guest| {
            guest.on_batch(Direction::FromServer, batch)
        })
        .map(Option::flatten)
    }

    /// Runs the mod's timed work and returns what it sends now; nothing once inactive.
    pub fn tick(&mut self) -> Result<Outbound> {
        let deadline = self.call_deadline;
        self.call("tick", deadline, |guest| guest.tick())
            .map(|outbound| {
                outbound.unwrap_or(Outbound {
                    to_client: Vec::new(),
                    to_server: Vec::new(),
                })
            })
    }

    /// Ends the session; later calls pass everything through.
    pub fn stop(&mut self) {
        let deadline = self.call_deadline;
        let _ = self.call("stop", deadline, |guest| guest.stop().map(Ok));
        if self.status == Status::Active {
            self.status = Status::Stopped;
        }
    }

    /// Whether calls still reach the guest.
    pub fn is_active(&self) -> bool {
        self.status == Status::Active
    }

    /// The largest linear memory the guest has requested so far.
    pub fn peak_memory_bytes(&self) -> usize {
        self.guest.peak_memory()
    }

    /// Runs one guest call under `deadline`; any failure quarantines this instance.
    fn call<R>(
        &mut self,
        name: &str,
        deadline: Duration,
        call: impl FnOnce(&mut dyn Guest) -> Result<Result<R, String>>,
    ) -> Result<Option<R>> {
        if self.status != Status::Active {
            return Ok(None);
        }
        self.guest.arm(deadline);
        let result = call(self.guest.as_mut()).and_then(|result| {
            result.map_err(|error| {
                let error: String = error.chars().take(MAX_ERROR_CHARS).collect();
                anyhow!("guest refused: {error}")
            })
        });
        result.map(Some).map_err(|error| {
            self.status = Status::Quarantined;
            tracing::warn!(
                call = name,
                error = format!("{error:#}"),
                "packet mod quarantined; packets now pass through unchanged"
            );
            error.context(format!("packet mod {name}"))
        })
    }
}

#[cfg(test)]
mod tests;
