//! Transactional server-bundle host. Production callers must use a restricted helper.

use crate::helper::Event;
use anyhow::{Result, ensure};
use server_experience::{
    policy::*,
    runtime::{CALLBACK_FUEL, Capabilities, Command, MediaOperation, Principal, Transaction},
    screen,
};
use wasmtime::{
    Config, Engine, Store, StoreLimits, StoreLimitsBuilder,
    component::{Component, Func, HasSelf, Instance, Linker},
};

wasmtime::component::bindgen!({
    path: "../experience-sdk/wit/client",
    world: "server-bundle",
    imports: { default: trappable },
});

struct State {
    limits: StoreLimits,
    owner: Principal,
    epoch: u64,
    capabilities: Capabilities,
    /// The declared action this callback delivers, the only one `input.pressed` reports.
    action: Option<String>,
    /// The open modal's size, which `ui.modal-size` returns.
    gui: Option<screen::GuiSize>,
    commands: Vec<Command>,
    bytes: usize,
    calls: usize,
}

impl State {
    /// Reserves the serialized owner, epoch and empty command array before guest output.
    fn begin_output(&mut self) -> Result<()> {
        self.commands.clear();
        self.calls = 0;
        self.bytes = serde_json::to_vec(&Transaction {
            owner: self.owner.clone(),
            epoch: self.epoch,
            commands: Vec::new(),
        })?
        .len();
        ensure!(
            self.bytes <= MAX_HOST_OUTPUT,
            "host output envelope too large"
        );
        Ok(())
    }

    /// Stops host-call floods even when the guest repeatedly ignores denied results.
    fn charge(&mut self) -> Result<()> {
        self.calls += 1;
        ensure!(self.calls <= 256, "host-call limit exceeded");
        Ok(())
    }

    /// Stages output privately; denied operations never reach the engine.
    fn stage(&mut self, command: Command) -> Result<Result<(), String>> {
        self.charge()?;
        if let Err(error) = self.capabilities.validate(&command) {
            return Ok(Err(error.to_string()));
        }
        let size = serde_json::to_vec(&command)?.len() + usize::from(!self.commands.is_empty());
        if size > MAX_HOST_OUTPUT - self.bytes {
            return Ok(Err("host output budget exceeded".into()));
        }
        self.bytes += size;
        self.commands.push(command);
        Ok(Ok(()))
    }
}

impl cinnabar::server_experience::ui::Host for State {
    /// Stages bounded label text for an owned widget.
    fn set_widget(&mut self, id: String, text: String) -> Result<Result<(), String>> {
        self.stage(Command::Widget { id, text })
    }

    /// Opens only a template the signed manifest indexes; `None` closes the modal.
    fn open_screen(&mut self, template: Option<String>) -> Result<Result<(), String>> {
        self.stage(Command::Screen { template })
    }

    fn close_screen(&mut self) -> Result<Result<(), String>> {
        self.stage(Command::Screen { template: None })
    }

    /// Parses rows within the output budget; malformed rows are denied, not trapped.
    fn set_collection(&mut self, name: String, rows_json: Vec<u8>) -> Result<Result<(), String>> {
        if rows_json.len() > MAX_HOST_OUTPUT {
            self.charge()?;
            return Ok(Err("collection too large".into()));
        }
        match serde_json::from_slice(&rows_json) {
            Ok(rows) => self.stage(Command::Collection { name, rows }),
            Err(error) => {
                self.charge()?;
                Ok(Err(format!("invalid rows: {error}")))
            }
        }
    }

    fn set_value(
        &mut self,
        name: String,
        value: cinnabar::server_experience::ui::Value,
    ) -> Result<Result<(), String>> {
        use cinnabar::server_experience::ui::Value;
        let value = match value {
            Value::Boolean(value) => screen::Value::Bool(value),
            Value::Integer(value) => screen::Value::Integer(value),
            Value::Number(value) => screen::Value::Number(value),
            Value::Text(value) => screen::Value::Text(value),
            Value::Numbers(values) => screen::Value::Numbers(values),
        };
        self.stage(Command::Value { name, value })
    }

    fn modal_size(&mut self) -> Result<Option<cinnabar::server_experience::ui::GuiSize>> {
        self.charge()?;
        Ok(self
            .gui
            .map(|size| cinnabar::server_experience::ui::GuiSize {
                width: size.width,
                height: size.height,
                scale: size.scale,
            }))
    }

    /// Stages bounded plain text for the open modal's edit boxes named `control`.
    fn set_text(&mut self, control: String, text: String) -> Result<Result<(), String>> {
        self.stage(Command::Text { control, text })
    }
}

impl cinnabar::server_experience::input::Host for State {
    /// True only for the declared, granted action this callback delivers.
    fn pressed(&mut self, action: String) -> Result<bool> {
        self.charge()?;
        Ok(self.action.as_ref() == Some(&action) && self.capabilities.may_deliver(&action))
    }
}

impl cinnabar::server_experience::messaging::Host for State {
    /// Parses and validates the signed channel record before queuing it.
    fn send(
        &mut self,
        channel: String,
        schema: u16,
        record_json: Vec<u8>,
    ) -> Result<Result<(), String>> {
        ensure!(record_json.len() <= MAX_PAYLOAD_BYTES, "message too large");
        let record = serde_json::from_slice(&record_json)?;
        self.stage(Command::Send {
            channel,
            schema,
            record,
        })
    }
}

impl cinnabar::server_experience::scene::Host for State {
    /// Accepts a bounded declarative object, never a native renderer handle.
    fn put(&mut self, id: u32, object_json: Option<Vec<u8>>) -> Result<Result<(), String>> {
        let object = object_json
            .map(|bytes| {
                ensure!(bytes.len() <= MAX_PAYLOAD_BYTES, "scene object too large");
                Ok::<_, anyhow::Error>(serde_json::from_slice(&bytes)?)
            })
            .transpose()?;
        self.stage(Command::Scene { id, object })
    }
}

impl cinnabar::server_experience::media::Host for State {
    /// Refers to an approved media descriptor, without exposing fetch APIs.
    fn control(
        &mut self,
        id: String,
        op: cinnabar::server_experience::media::Operation,
        position_ms: u64,
    ) -> Result<Result<(), String>> {
        use cinnabar::server_experience::media::Operation;
        let operation = match op {
            Operation::Prepare => MediaOperation::Prepare,
            Operation::Play => MediaOperation::Play,
            Operation::Pause => MediaOperation::Pause,
            Operation::Seek => MediaOperation::Seek,
            Operation::Stop => MediaOperation::Stop,
        };
        self.stage(Command::Media {
            id,
            operation,
            position_ms,
        })
    }
}

/// The guest's callbacks. A component built against 1.0 exports only `init` and `dispatch`;
/// 1.1 adds `action` and `epoch`, both or neither; 1.2 its three editing callbacks, all or none;
/// 1.3 `scroll-changed`.
struct Exports {
    dispatch: Func,
    events: Option<(Func, Func)>,
    /// 1.2's `modal-resized`, `text-changed` and `secondary-action`.
    editing: Option<(Func, Func, Func)>,
    /// 1.3's `scroll-changed`.
    scrolled: Option<Func>,
}

impl Exports {
    /// Type-checks every callback once, before the guest runs.
    fn find(store: &mut Store<State>, instance: &Instance) -> Result<(Func, Self)> {
        let mut find = |name: &str| instance.get_func(&mut *store, name);
        let (init, dispatch, action, epoch) = (
            find("init"),
            find("dispatch"),
            find("action"),
            find("epoch"),
        );
        let (resized, text, secondary) = (
            find("modal-resized"),
            find("text-changed"),
            find("secondary-action"),
        );
        let scrolled = find("scroll-changed");
        let (Some(init), Some(dispatch)) = (init, dispatch) else {
            anyhow::bail!("component lacks init or dispatch");
        };
        init.typed::<(), ()>(&*store)?;
        dispatch.typed::<(&str, &[u8]), ()>(&*store)?;
        let events = match (action, epoch) {
            (Some(action), Some(epoch)) => {
                action.typed::<(&str, Option<u32>), ()>(&*store)?;
                epoch.typed::<(), ()>(&*store)?;
                Some((action, epoch))
            }
            (None, None) => None,
            _ => anyhow::bail!("component exports only half of action and epoch"),
        };
        let editing = match (resized, text, secondary) {
            (Some(resized), Some(text), Some(secondary)) => {
                resized.typed::<(GuiSize,), ()>(&*store)?;
                text.typed::<(&str, &str), ()>(&*store)?;
                secondary.typed::<(&str, Option<u32>), ()>(&*store)?;
                Some((resized, text, secondary))
            }
            (None, None, None) => None,
            _ => anyhow::bail!(
                "component exports only some of modal-resized, text-changed and secondary-action"
            ),
        };
        if let Some(scrolled) = &scrolled {
            scrolled.typed::<(&str, ScrollRange), ()>(&*store)?;
        }
        Ok((
            init,
            Self {
                dispatch,
                events,
                editing,
                scrolled,
            },
        ))
    }
}

pub struct BundleHost {
    store: Store<State>,
    exports: Exports,
    /// The linked component, kept to start a fresh instance after a failed callback.
    linker: Linker<State>,
    component: Component,
    active: bool,
    /// Fuel the last `init` or callback consumed.
    last_fuel: u64,
}

impl BundleHost {
    /// Unsafe for production remote code: this explicit developer path has no OS sandbox.
    pub fn developer_in_process(
        bytes: &[u8],
        owner: Principal,
        capabilities: Capabilities,
        epoch: u64,
    ) -> Result<Self> {
        ensure!(
            std::env::var(DEVELOPER_ENV).as_deref() == Ok("1"),
            "in-process server code is developer-only"
        );
        Self::instantiate(bytes, owner, capabilities, epoch)
    }

    /// Compiles inside the helper, with no WASI or precompiled native cache input.
    pub(crate) fn instantiate(
        bytes: &[u8],
        owner: Principal,
        capabilities: Capabilities,
        epoch: u64,
    ) -> Result<Self> {
        ensure!(
            bytes.len() <= MAX_COMPONENT_BYTES && bytes.starts_with(b"\0asm"),
            "invalid component bytes"
        );
        Self::launch(bytes, owner, capabilities, epoch)
    }

    /// Links the 1.1 imports, which also satisfy a 1.0 component's semver-compatible ones, and
    /// runs `init`.
    fn launch(
        source: &[u8],
        owner: Principal,
        capabilities: Capabilities,
        epoch: u64,
    ) -> Result<Self> {
        capabilities.scope.validate()?;
        ensure!(
            capabilities.scope.memory_bytes > 0
                && capabilities.scope.memory_bytes <= MAX_GUEST_MEMORY,
            "invalid guest memory limit"
        );
        let mut config = Config::new();
        config
            .wasm_component_model(true)
            .consume_fuel(true)
            .max_wasm_stack(256 * 1024);
        let engine = Engine::new(&config)?;
        let component = Component::new(&engine, source)?;
        let mut linker = Linker::new(&engine);
        ServerBundle::add_to_linker::<_, HasSelf<_>>(&mut linker, |state: &mut State| state)?;
        let (store, exports) = Self::start(&linker, &component, owner, capabilities, epoch)?;
        let mut host = Self {
            store,
            exports,
            linker,
            component,
            active: true,
            last_fuel: 0,
        };
        host.last_fuel = host.fuel_used();
        Ok(host)
    }

    /// A fresh store and instance of `component` that has run `init`, its output staged.
    fn start(
        linker: &Linker<State>,
        component: &Component,
        owner: Principal,
        capabilities: Capabilities,
        epoch: u64,
    ) -> Result<(Store<State>, Exports)> {
        let mut state = State {
            limits: StoreLimitsBuilder::new()
                .memory_size(capabilities.scope.memory_bytes as usize)
                .table_elements(4096)
                .instances(16)
                .memories(1)
                .tables(2)
                .trap_on_grow_failure(true)
                .build(),
            owner,
            epoch,
            capabilities,
            action: None,
            gui: None,
            commands: Vec::new(),
            bytes: 0,
            calls: 0,
        };
        state.begin_output()?;
        let mut store = Store::new(linker.engine(), state);
        store.limiter(|state| &mut state.limits);
        store.set_fuel(CALLBACK_FUEL)?;
        let instance = linker.instantiate(&mut store, component)?;
        let (init, exports) = Exports::find(&mut store, &instance)?;
        let init = init.typed::<(), ()>(&store)?;
        init.call(&mut store, ())?;
        init.post_return(&mut store)?;
        Ok((store, exports))
    }

    /// Runs one fuel-bounded callback. A failed callback (a trap, its fuel running out, or a
    /// host rule it broke) discards all its output and leaves an instance that may be
    /// inconsistent, so it is replaced by a fresh one, whose guest memory starts over; the
    /// fresh instance's `init` output is discarded too, since the first one's still stands. Only
    /// a failed restart quarantines the bundle. A 1.0 component skips `action` and `epoch` and
    /// returns an empty transaction.
    pub fn dispatch(&mut self, event: &Event, epoch: u64) -> Result<Transaction> {
        ensure!(self.active, "bundle quarantined");
        event.check()?;
        let state = self.store.data_mut();
        if let Event::Action { id, .. }
        | Event::SecondaryAction { id, .. }
        | Event::Text { control: id, .. }
        | Event::Scrolled { view: id, .. } = event
        {
            ensure!(state.capabilities.may_deliver(id), "action not granted");
        }
        if let Event::Resized { size } = event {
            state.gui = Some(*size);
        }
        state.action = match event {
            Event::Action { id, .. } | Event::SecondaryAction { id, .. } => Some(id.clone()),
            _ => None,
        };
        state.epoch = epoch;
        state.begin_output()?;
        self.store.set_fuel(CALLBACK_FUEL)?;
        let called = self.call(event);
        self.last_fuel = self.fuel_used();
        if let Err(error) = called {
            self.store.data_mut().commands.clear();
            self.restart();
            return Err(error);
        }
        Ok(self.take_transaction())
    }

    /// Sets the open modal's size that `ui.modal-size` returns from the next callback on; none
    /// while the modal is closed.
    pub fn set_modal_size(&mut self, size: Option<screen::GuiSize>) {
        self.store.data_mut().gui = size;
    }

    /// The fuel that the last `init` or callback consumed, of the `CALLBACK_FUEL` each gets; a
    /// failed callback's included, which is all of it when it ran out.
    pub fn last_fuel_used(&self) -> u64 {
        self.last_fuel
    }

    /// The fuel the current store has consumed since its fuel was last set.
    fn fuel_used(&self) -> u64 {
        CALLBACK_FUEL - self.store.get_fuel().unwrap_or(0).min(CALLBACK_FUEL)
    }

    /// Replaces the instance with a fresh one after a failed callback, or quarantines the bundle
    /// when that fails. Either way it says so on stderr, which the client logs.
    fn restart(&mut self) {
        let state = self.store.data();
        let (owner, capabilities, epoch) =
            (state.owner.clone(), state.capabilities.clone(), state.epoch);
        let bundle = owner.bundle.clone();
        match Self::start(&self.linker, &self.component, owner, capabilities, epoch) {
            Ok((store, exports)) => {
                (self.store, self.exports) = (store, exports);
                self.store.data_mut().commands.clear();
                eprintln!(
                    "bundle {bundle} restarted after a failed callback; its guest state is lost"
                );
            }
            Err(error) => {
                self.active = false;
                eprintln!("bundle {bundle} quarantined: its restart failed: {error:#}");
            }
        }
    }

    fn call(&mut self, event: &Event) -> Result<()> {
        let store = &mut self.store;
        match (event, &self.exports.events) {
            (Event::Message { channel, record }, _) => {
                let dispatch = self.exports.dispatch.typed::<(&str, &[u8]), ()>(&*store)?;
                dispatch.call(&mut *store, (channel, record))?;
                dispatch.post_return(store)
            }
            (Event::Action { id, index }, Some((action, _))) => {
                let action = action.typed::<(&str, Option<u32>), ()>(&*store)?;
                action.call(&mut *store, (id, *index))?;
                action.post_return(store)
            }
            (Event::Epoch, Some((_, epoch))) => {
                let epoch = epoch.typed::<(), ()>(&*store)?;
                epoch.call(&mut *store, ())?;
                epoch.post_return(store)
            }
            (Event::Action { .. } | Event::Epoch, None) => Ok(()),
            (Event::Resized { size }, _) => match &self.exports.editing {
                Some((resized, _, _)) => {
                    let resized = resized.typed::<(GuiSize,), ()>(&*store)?;
                    let size = GuiSize {
                        width: size.width,
                        height: size.height,
                        scale: size.scale,
                    };
                    resized.call(&mut *store, (size,))?;
                    resized.post_return(store)
                }
                None => Ok(()),
            },
            (Event::SecondaryAction { id, index }, _) => match &self.exports.editing {
                Some((_, _, secondary)) => {
                    let secondary = secondary.typed::<(&str, Option<u32>), ()>(&*store)?;
                    secondary.call(&mut *store, (id, *index))?;
                    secondary.post_return(store)
                }
                None => Ok(()),
            },
            (Event::Scrolled { view, range }, _) => match &self.exports.scrolled {
                Some(scrolled) => {
                    let scrolled = scrolled.typed::<(&str, ScrollRange), ()>(&*store)?;
                    let range = ScrollRange {
                        offset: range.offset,
                        viewport: range.viewport,
                        content: range.content,
                    };
                    scrolled.call(&mut *store, (view, range))?;
                    scrolled.post_return(store)
                }
                None => Ok(()),
            },
            (Event::Text { control, text }, _) => match &self.exports.editing {
                Some((_, changed, _)) => {
                    let changed = changed.typed::<(&str, &str), ()>(&*store)?;
                    changed.call(&mut *store, (control, text))?;
                    changed.post_return(store)
                }
                None => Ok(()),
            },
        }
    }

    /// Takes only successfully returned initialization or callback output.
    pub fn take_transaction(&mut self) -> Transaction {
        let state = self.store.data_mut();
        Transaction {
            owner: state.owner.clone(),
            epoch: state.epoch,
            commands: std::mem::take(&mut state.commands),
        }
    }
}

#[cfg(test)]
mod tests;
