//! Packet mods built as WASI preview 1 reactor core modules, such as a language runtime's
//! shared-library build. The host calls the `packet-mod` world's exports through their canonical
//! ABI itself, so the module's `cabi_realloc` only runs after its `_initialize` has finished.

use super::{
    Direction, Guest, Outbound, PACKET_MOD_LOAD_DEADLINE, PACKET_MOD_TRANSFER_BYTES,
    SessionIdentity, SessionPack,
    limits::PacketLimits,
    native_capabilities::{FRAME, MENU, NATIVE_INIT, NativeExports},
    wasi,
};
use crate::{ModGrants, execution};
use anyhow::{Context, Result, bail, ensure};
use std::time::Duration;
use wasmtime::{Engine, ExternType, InstancePre, Memory, Module, Store, TypedFunc};
use wasmtime_wasi::preview1::WasiP1Ctx;

const MEMORY: &str = "memory";
const REALLOC: &str = "cabi_realloc";
const INITIALIZE: &str = "_initialize";
const START: &str = "start";
const ON_BATCH: &str = "on-batch";
const TICK: &str = "tick";
const STOP: &str = "stop";
/// Prefix of the optional export that frees a call's returned memory.
const POST_RETURN: &str = "cabi_post_";
/// Flat `i32` parameters and results of each canonical export: `start` passes three identity
/// strings, the client data and the startup list as pointer and length pairs.
const SIGNATURES: [(&str, usize, usize); 6] = [
    (REALLOC, 4, 1),
    (START, 10, 1),
    (ON_BATCH, 3, 1),
    (TICK, 0, 1),
    (STOP, 0, 0),
    (INITIALIZE, 0, 0),
];
/// Byte size of one `session-pack`: an archive list and a boolean, aligned to four.
const SESSION_PACK_BYTES: u32 = 12;

type StartParams = (i32, i32, i32, i32, i32, i32, i32, i32, i32, i32);

/// Store data: the locked-down WASI preview 1 context and the memory limiter.
struct State {
    wasi: WasiP1Ctx,
    limits: PacketLimits,
    log: wasi::LogBudget,
}

/// A compiled module linked against WASI preview 1 only with the `wasi` grant.
#[derive(Clone)]
pub(super) struct Program {
    pre: InstancePre<State>,
    native_capabilities: u32,
    camera_players: bool,
}

impl Program {
    /// Compiles the module, checks its ABI, and reports its native image size.
    pub(super) fn new(engine: &Engine, bytes: &[u8], grants: &ModGrants) -> Result<(Self, usize)> {
        let module = Module::new(engine, bytes).context("compile packet mod")?;
        let image = module.image_range();
        let image_bytes = image.end as usize - image.start as usize;
        for (name, params, results) in SIGNATURES {
            let required = name != INITIALIZE;
            match module.get_export(name) {
                Some(ExternType::Func(ty)) => ensure!(
                    ty.params().len() == params
                        && ty.results().len() == results
                        && ty.params().chain(ty.results()).all(|ty| ty.is_i32()),
                    "packet mod export {name} has the wrong signature"
                ),
                None if !required => {}
                _ => bail!("packet mod does not export function {name}"),
            }
        }
        let native_capabilities = NativeExports::from_module(&module).negotiate(grants)?;
        ensure!(
            matches!(module.get_export(MEMORY), Some(ExternType::Memory(_))),
            "packet mod does not export its memory"
        );
        let mut linker = wasmtime::Linker::new(engine);
        if grants.wasi {
            wasmtime_wasi::preview1::add_to_linker_async(&mut linker, |state: &mut State| {
                &mut state.wasi
            })?;
        }
        let pre = linker.instantiate_pre(&module).context("link packet mod")?;
        Ok((
            Self {
                pre,
                native_capabilities,
                camera_players: grants.players,
            },
            image_bytes,
        ))
    }

    /// Instantiates the module and runs its `_initialize` under the load deadline.
    pub(super) fn instantiate(&self, engine: &Engine) -> Result<Box<dyn Guest>> {
        let log = wasi::LogBudget::default();
        let mut store = Store::new(
            engine,
            State {
                wasi: wasi::builder(&log).build_p1(),
                limits: PacketLimits::default(),
                log,
            },
        );
        store.limiter(|state| &mut state.limits);
        execution::arm_deadline(&mut store, PACKET_MOD_LOAD_DEADLINE);
        let deadline = wasi::CallDeadline::new(PACKET_MOD_LOAD_DEADLINE)?;
        let instance = deadline.run(self.pre.instantiate_async(&mut store))?;
        if instance.get_func(&mut store, INITIALIZE).is_some() {
            deadline.run(
                instance
                    .get_typed_func::<(), ()>(&mut store, INITIALIZE)?
                    .call_async(&mut store, ()),
            )?;
        }
        if instance.get_func(&mut store, NATIVE_INIT).is_some() {
            deadline.run(
                instance
                    .get_typed_func::<i32, ()>(&mut store, NATIVE_INIT)?
                    .call_async(&mut store, self.native_capabilities as i32),
            )?;
        }
        let memory = instance
            .get_memory(&mut store, MEMORY)
            .context("packet mod memory")?;
        let mut post = |name: &str| {
            let name = format!("{POST_RETURN}{name}");
            match instance.get_func(&mut store, &name) {
                Some(func) => func.typed::<i32, ()>(&store).map(Some),
                None => Ok(None),
            }
        };
        let posts = [
            post(START)?,
            post(ON_BATCH)?,
            post(TICK)?,
            post(FRAME)?,
            post(MENU)?,
            post(super::native_capabilities::CAMERA_FRAME)?,
            post(super::native_capabilities::RENDER_FRAME)?,
        ];
        let frame = if self.native_capabilities & mod_api::native_frame::CAMERA_PLAYERS != 0 {
            Some(instance.get_typed_func(&mut store, FRAME)?)
        } else {
            None
        };
        let menu = if self.native_capabilities & mod_api::native_menu::CONTROLS != 0 {
            Some(instance.get_typed_func(&mut store, MENU)?)
        } else {
            None
        };
        let camera_frame = if self.native_capabilities & mod_api::native_camera::CAPABILITY != 0 {
            Some(instance.get_typed_func(&mut store, super::native_capabilities::CAMERA_FRAME)?)
        } else {
            None
        };
        let render_frame = if self.native_capabilities & mod_api::native_render::CAPABILITY != 0 {
            Some(instance.get_typed_func(&mut store, super::native_capabilities::RENDER_FRAME)?)
        } else {
            None
        };
        let calls = Calls {
            realloc: instance.get_typed_func(&mut store, REALLOC)?,
            start: instance.get_typed_func(&mut store, START)?,
            on_batch: instance.get_typed_func(&mut store, ON_BATCH)?,
            tick: instance.get_typed_func(&mut store, TICK)?,
            stop: instance.get_typed_func(&mut store, STOP)?,
            frame,
            camera_frame,
            render_frame,
            menu,
            posts,
        };
        Ok(Box::new(CoreGuest {
            store,
            memory,
            calls,
            deadline,
            camera_players: self.camera_players,
        }))
    }
}

/// The module's canonical exports, typed once at instantiation.
struct Calls {
    realloc: TypedFunc<(i32, i32, i32, i32), i32>,
    start: TypedFunc<StartParams, i32>,
    on_batch: TypedFunc<(i32, i32, i32), i32>,
    tick: TypedFunc<(), i32>,
    stop: TypedFunc<(), ()>,
    frame: Option<TypedFunc<(i32, i32), i32>>,
    camera_frame: Option<TypedFunc<(i32, i32), i32>>,
    render_frame: Option<TypedFunc<(i32, i32), i32>>,
    menu: Option<TypedFunc<(i32, i32), i32>>,
    /// Post-return functions of `start`, `on-batch`, `tick` and optional `frame` and `menu`.
    posts: [Option<TypedFunc<i32, ()>>; 7],
}

struct CoreGuest {
    store: Store<State>,
    memory: Memory,
    calls: Calls,
    deadline: wasi::CallDeadline,
    camera_players: bool,
}

impl CoreGuest {
    /// Copies `bytes` into a fresh guest allocation, as the canonical ABI lowers a byte list or
    /// a UTF-8 string, and returns its pointer and length.
    fn lower(&mut self, bytes: &[u8]) -> Result<(i32, i32)> {
        let length = i32::try_from(bytes.len()).context("argument exceeds guest memory")?;
        let pointer = self.deadline.run(
            self.calls
                .realloc
                .call_async(&mut self.store, (0, 0, 1, length)),
        )?;
        self.memory
            .write(&mut self.store, pointer as u32 as usize, bytes)
            .context("guest allocation is out of bounds")?;
        Ok((pointer, length))
    }

    /// Reads one returned value from guest memory, then runs its post-return function.
    fn lift<T>(
        &mut self,
        post: usize,
        result: i32,
        read: impl FnOnce(&mut Reader, u32) -> Result<T>,
    ) -> Result<T> {
        self.lift_bounded(post, result, PACKET_MOD_TRANSFER_BYTES, read)
    }

    /// Reads a result under the callback's own transfer allowance.
    fn lift_bounded<T>(
        &mut self,
        post: usize,
        result: i32,
        budget: usize,
        read: impl FnOnce(&mut Reader, u32) -> Result<T>,
    ) -> Result<T> {
        let address = result as u32;
        ensure!(address.is_multiple_of(4), "misaligned return area");
        let mut reader = Reader {
            memory: self.memory.data(&self.store),
            budget,
        };
        reader.range(address, 12)?;
        let value = read(&mut reader, address)?;
        if let Some(post) = &self.calls.posts[post] {
            self.deadline
                .run(post.call_async(&mut self.store, result))?;
        }
        Ok(value)
    }
}

impl Guest for CoreGuest {
    fn arm(&mut self, deadline: Duration) {
        self.deadline.arm(deadline);
        self.store.data().log.refill();
        execution::arm_deadline(&mut self.store, deadline);
    }

    fn start(
        &mut self,
        identity: &SessionIdentity,
        client_data: &str,
        startup: &[u8],
    ) -> Result<Result<Vec<SessionPack>, String>> {
        let name = self.lower(identity.display_name.as_bytes())?;
        let xuid = self.lower(identity.xuid.as_bytes())?;
        let uuid = self.lower(identity.uuid.as_bytes())?;
        let data = self.lower(client_data.as_bytes())?;
        let startup = self.lower(startup)?;
        let params = (
            name.0, name.1, xuid.0, xuid.1, uuid.0, uuid.1, data.0, data.1, startup.0, startup.1,
        );
        let result = self
            .deadline
            .run(self.calls.start.call_async(&mut self.store, params))?;
        self.lift(0, result, |reader, at| {
            reader.result(at, Reader::session_packs)
        })
    }

    fn on_batch(
        &mut self,
        direction: Direction,
        batch: &[u8],
    ) -> Result<Result<Option<Vec<u8>>, String>> {
        let direction = match direction {
            Direction::FromClient => 0,
            Direction::FromServer => 1,
        };
        let (pointer, length) = self.lower(batch)?;
        let result = self.deadline.run(
            self.calls
                .on_batch
                .call_async(&mut self.store, (direction, pointer, length)),
        )?;
        self.lift(1, result, |reader, at| {
            reader.result(at, |reader, at| match reader.u8(at)? {
                0 => Ok(None),
                1 => reader.list(at + 4).map(Some),
                other => bail!("invalid option discriminant {other}"),
            })
        })
    }

    fn tick(&mut self) -> Result<Result<Outbound, String>> {
        let result = self
            .deadline
            .run(self.calls.tick.call_async(&mut self.store, ()))?;
        self.lift(2, result, |reader, at| {
            reader.result(at, |reader, at| {
                Ok(Outbound {
                    to_client: reader.list(at)?,
                    to_server: reader.list(at + 8)?,
                })
            })
        })
    }

    fn stop(&mut self) -> Result<()> {
        self.deadline
            .run(self.calls.stop.call_async(&mut self.store, ()))
    }

    fn has_native_frame(&self) -> bool {
        self.calls.frame.is_some()
    }

    fn frame(&mut self, json: &[u8]) -> Result<Result<Option<Vec<u8>>, String>> {
        let Some(frame) = self.calls.frame.clone() else {
            return Ok(Ok(None));
        };
        let input = self.lower(json)?;
        let result = self
            .deadline
            .run(frame.call_async(&mut self.store, input))?;
        self.lift_bounded(3, result, mod_api::native_frame::MAX_BYTES, |reader, at| {
            reader.result(at, |reader, at| reader.list(at).map(Some))
        })
    }

    fn has_native_render(&self) -> bool {
        self.calls.render_frame.is_some()
    }
    fn render_frame(&mut self, json: &[u8]) -> Result<Result<Option<Vec<u8>>, String>> {
        let Some(frame) = self.calls.render_frame.clone() else {
            return Ok(Ok(None));
        };
        let input = self.lower(json)?;
        let result = self
            .deadline
            .run(frame.call_async(&mut self.store, input))?;
        self.lift_bounded(6, result, mod_api::native_frame::MAX_BYTES, |reader, at| {
            reader.result(at, |reader, at| reader.list(at).map(Some))
        })
    }
    fn has_native_camera(&self) -> bool {
        self.calls.camera_frame.is_some()
    }
    fn native_camera_players(&self) -> bool {
        self.camera_players
    }
    fn camera_frame(&mut self, json: &[u8]) -> Result<Result<Option<Vec<u8>>, String>> {
        let Some(frame) = self.calls.camera_frame.clone() else {
            return Ok(Ok(None));
        };
        let input = self.lower(json)?;
        let result = self
            .deadline
            .run(frame.call_async(&mut self.store, input))?;
        self.lift_bounded(5, result, mod_api::native_frame::MAX_BYTES, |reader, at| {
            reader.result(at, |reader, at| reader.list(at).map(Some))
        })
    }

    fn has_native_menu(&self) -> bool {
        self.calls.menu.is_some()
    }

    fn menu(&mut self, json: &[u8]) -> Result<Result<Option<Vec<u8>>, String>> {
        let Some(menu) = self.calls.menu.clone() else {
            return Ok(Ok(None));
        };
        let input = self.lower(json)?;
        let result = self.deadline.run(menu.call_async(&mut self.store, input))?;
        self.lift_bounded(
            4,
            result,
            mod_api::native_menu::MAX_OUTPUT_BYTES,
            |reader, at| reader.result(at, |reader, at| reader.list(at).map(Some)),
        )
    }

    fn peak_memory(&self) -> usize {
        self.store.data().limits.peak()
    }
}

/// Bounds-checked canonical reads from guest memory with a per-call host allocation budget.
struct Reader<'a> {
    memory: &'a [u8],
    budget: usize,
}

impl Reader<'_> {
    /// Copies a session pack list, charging its native entries before allocating or reading archives.
    fn session_packs(&mut self, address: u32) -> Result<Vec<SessionPack>> {
        let (pointer, count) = (self.u32(address)?, self.u32(address + 4)?);
        ensure!(pointer.is_multiple_of(4), "misaligned session pack list");
        let capacity = usize::try_from(count)?;
        self.charge::<SessionPack>(capacity)?;
        let size = u64::from(count) * u64::from(SESSION_PACK_BYTES);
        self.range(pointer, size)?;
        let mut packs = Vec::with_capacity(capacity);
        for index in 0..count {
            let pack = pointer + index * SESSION_PACK_BYTES;
            packs.push(SessionPack {
                archive: self.list(pack)?,
                above_server: self.u8(pack + 8)? != 0,
            });
        }
        Ok(packs)
    }

    /// Reserves the host allocation for `count` values before their vector is created.
    fn charge<T>(&mut self, count: usize) -> Result<()> {
        let bytes = count
            .checked_mul(size_of::<T>())
            .context("packet mod returned too many bytes")?;
        self.budget = self
            .budget
            .checked_sub(bytes)
            .context("packet mod returned too many bytes")?;
        Ok(())
    }

    /// Returns `length` bytes at `address`, refusing ranges outside guest memory.
    fn range(&self, address: u32, length: u64) -> Result<&[u8]> {
        let start = usize::try_from(address)?;
        let end = u64::from(address) + length;
        ensure!(
            end <= self.memory.len() as u64,
            "guest pointer is out of bounds"
        );
        Ok(&self.memory[start..usize::try_from(end)?])
    }

    fn u8(&self, address: u32) -> Result<u8> {
        Ok(self.range(address, 1)?[0])
    }

    fn u32(&self, address: u32) -> Result<u32> {
        let bytes = self.range(address, 4)?;
        Ok(u32::from_le_bytes(bytes.try_into()?))
    }

    /// Copies the byte list whose pointer and length are stored at `address`.
    fn list(&mut self, address: u32) -> Result<Vec<u8>> {
        let (pointer, length) = (self.u32(address)?, self.u32(address + 4)?);
        let length = usize::try_from(length)?;
        self.charge::<u8>(length)?;
        Ok(self.range(pointer, length as u64)?.to_vec())
    }

    /// Reads a `result<T, string>` stored at `address`, its payload four bytes in.
    fn result<T>(
        &mut self,
        address: u32,
        ok: impl FnOnce(&mut Self, u32) -> Result<T>,
    ) -> Result<Result<T, String>> {
        match self.u8(address)? {
            0 => ok(self, address + 4).map(Ok),
            1 => {
                let text = self.list(address + 4)?;
                Ok(Err(
                    String::from_utf8(text).context("guest error is not UTF-8")?
                ))
            }
            other => bail!("invalid result discriminant {other}"),
        }
    }
}

#[cfg(test)]
mod tests;
