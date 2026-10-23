//! Transactional server-bundle host. Production callers must use a restricted helper.

use crate::{helper::Event, runtime::cinnabar::session::items};
use anyhow::{Result, ensure};
use server_experience::{
    manifest::Permission,
    policy::*,
    runtime::{CALLBACK_FUEL, Capabilities, Command, MediaOperation, Principal, Transaction},
    screen,
    session_data::{ItemKey, SessionData},
};
use std::sync::Arc;
use wasmtime::{
    Store, StoreLimits, StoreLimitsBuilder,
    component::{Component, Func, HasSelf, Instance, Linker},
};

wasmtime::component::bindgen!({
    path: ["../experience-sdk/wit/client/deps/server-experience", "../experience-sdk/wit/session", "../mod-api/wit"],
    world: "cinnabar:extension/server-bundle@0.1.0",
    imports: { default: trappable },
    with: {
        "cinnabar:session/items@0.1.0": crate::runtime::cinnabar::session::items,
        "cinnabar:world-draw": crate::runtime::cinnabar::world_draw,
        "cinnabar:extension/render@0.1.0": crate::runtime::cinnabar::extension::render,
        "cinnabar:extension/environment@0.1.0": crate::runtime::cinnabar::extension::environment,
    },
});

struct State {
    presentation: crate::runtime::State,
    /// The session's items as the client last sent them; empty until it does.
    session: Arc<SessionData>,
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

impl State {
    /// Charges the call and refuses it without the `items` permission.
    fn read_items(&mut self) -> Result<Result<(), String>> {
        self.charge()?;
        Ok(
            if self
                .capabilities
                .scope
                .permissions
                .contains(&Permission::Items)
            {
                Ok(())
            } else {
                Err("items permission denied".into())
            },
        )
    }
}

impl items::Host for State {
    fn revision(&mut self) -> Result<Result<u64, String>> {
        Ok(self.read_items()?.map(|()| self.session.item_revision))
    }

    fn count(&mut self) -> Result<Result<u32, String>> {
        let count = u32::try_from(self.session.items.len()).unwrap_or(u32::MAX);
        Ok(self.read_items()?.map(|()| count))
    }

    fn page(&mut self, start: u32, count: u32) -> Result<Result<Vec<items::Item>, String>> {
        Ok(self.read_items()?.map(|()| {
            let page = self.session.item_page(start, count);
            page.iter().map(crate::runtime::player_mod::item).collect()
        }))
    }

    fn lookup(&mut self, key: items::ItemKey) -> Result<Result<Option<items::Item>, String>> {
        let key = ItemKey {
            identifier: key.identifier,
            aux: key.aux,
        };
        Ok(self.read_items()?.map(|()| {
            self.session
                .lookup(&key)
                .map(crate::runtime::player_mod::item)
        }))
    }

    fn tag_members(&mut self, tag: String) -> Result<Result<Vec<String>, String>> {
        Ok(self
            .read_items()?
            .map(|()| self.session.tag_members(&tag).to_vec()))
    }
}

/// The guest's callbacks. A component built against 1.0 exports only `init` and `dispatch`;
/// 1.1 adds `action` and `epoch`, both or neither; 1.2 its three editing callbacks, all or none;
/// 1.3 `scroll-changed`; then `shift-changed`.
struct Exports {
    dispatch: Func,
    events: Option<(Func, Func)>,
    /// 1.2's `modal-resized`, `text-changed` and `secondary-action`.
    editing: Option<(Func, Func, Func)>,
    /// 1.3's `scroll-changed`.
    scrolled: Option<Func>,
    shift: Option<Func>,
    frame: Option<Func>,
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
        let shift = find("shift-changed");
        let frame = find("frame");
        if let Some(frame) = &frame {
            frame.typed::<(), ()>(&*store)?;
        }
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
        if let Some(shift) = &shift {
            shift.typed::<(bool,), ()>(&*store)?;
        }
        Ok((
            init,
            Self {
                dispatch,
                events,
                editing,
                scrolled,
                shift,
                frame,
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
    presentation_sent: Option<(u64, Option<u32>, Option<[f32; 2]>)>,
    presentation_pending: Option<crate::presentation::Pending>,
    _ticker: crate::execution::EpochTicker,
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
        Self::instantiate_with_assets(bytes, owner, capabilities, epoch, Default::default())
    }

    /// Admits host-verified assets before guest initialization can read them.
    pub(crate) fn instantiate_with_assets(
        bytes: &[u8],
        owner: Principal,
        capabilities: Capabilities,
        epoch: u64,
        assets: std::collections::BTreeMap<String, Vec<u8>>,
    ) -> Result<Self> {
        ensure!(
            bytes.len() <= MAX_COMPONENT_BYTES && bytes.starts_with(b"\0asm"),
            "invalid component bytes"
        );
        ensure!(
            assets.values().map(Vec::len).sum::<usize>() as u64 <= capabilities.scope.memory_bytes,
            "drawing asset memory budget exceeded"
        );
        ensure!(
            assets.keys().all(|path| capabilities.assets.contains(path)),
            "unowned drawing asset"
        );
        Self::launch(bytes, owner, capabilities, epoch, assets)
    }

    /// Links the 1.1 imports, which also satisfy a 1.0 component's semver-compatible ones, and
    /// runs `init`.
    fn launch(
        source: &[u8],
        owner: Principal,
        capabilities: Capabilities,
        epoch: u64,
        assets: std::collections::BTreeMap<String, Vec<u8>>,
    ) -> Result<Self> {
        capabilities.scope.validate()?;
        ensure!(
            capabilities.scope.memory_bytes > 0
                && capabilities.scope.memory_bytes <= MAX_GUEST_MEMORY,
            "invalid guest memory limit"
        );
        let (engine, ticker) = crate::execution::engine()?;
        let component = Component::new(&engine, source)?;
        let mut linker = Linker::new(&engine);
        cinnabar::server_experience::ui::add_to_linker::<_, HasSelf<_>>(
            &mut linker,
            |state: &mut State| state,
        )?;
        cinnabar::server_experience::input::add_to_linker::<_, HasSelf<_>>(
            &mut linker,
            |state: &mut State| state,
        )?;
        cinnabar::server_experience::messaging::add_to_linker::<_, HasSelf<_>>(
            &mut linker,
            |state: &mut State| state,
        )?;
        cinnabar::server_experience::scene::add_to_linker::<_, HasSelf<_>>(
            &mut linker,
            |state: &mut State| state,
        )?;
        cinnabar::server_experience::media::add_to_linker::<_, HasSelf<_>>(
            &mut linker,
            |state: &mut State| state,
        )?;
        items::add_to_linker::<_, HasSelf<_>>(&mut linker, |state: &mut State| state)?;
        crate::runtime::State::link_presentation(&mut linker, |state: &mut State| {
            &mut state.presentation
        })?;
        let (store, exports) =
            Self::start(&linker, &component, owner, capabilities, epoch, assets)?;
        let mut host = Self {
            store,
            exports,
            linker,
            component,
            active: true,
            last_fuel: 0,
            presentation_sent: None,
            presentation_pending: None,
            _ticker: ticker,
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
        assets: std::collections::BTreeMap<String, Vec<u8>>,
    ) -> Result<(Store<State>, Exports)> {
        let mut state = State {
            presentation: crate::runtime::State::new(
                drawing_grants(&capabilities),
                String::new(),
                Default::default(),
            ),
            session: Arc::default(),
            limits: StoreLimitsBuilder::new()
                .memory_size(
                    (capabilities.scope.memory_bytes as usize)
                        .saturating_sub(assets.values().map(Vec::len).sum::<usize>()),
                )
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
        state.presentation.asset_files = assets;
        state.begin_output()?;
        let mut store = Store::new(linker.engine(), state);
        store.limiter(|state| &mut state.limits);
        crate::execution::arm(&mut store, CALLBACK_FUEL, true)?;
        let instance = linker.instantiate(&mut store, component)?;
        let (init, exports) = Exports::find(&mut store, &instance)?;
        let init = init.typed::<(), ()>(&store)?;
        init.call(&mut store, ())?;
        init.post_return(&mut store)?;
        store.data_mut().presentation.commit_presentation();
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
        if self.store.data().epoch != epoch || matches!(event, Event::Epoch) {
            self.presentation_pending = None;
            self.presentation_sent = None;
        }
        let state = self.store.data_mut();
        if let Event::Action { id, .. }
        | Event::SecondaryAction { id, .. }
        | Event::Text { control: id, .. }
        | Event::Scrolled { view: id, .. } = event
        {
            ensure!(state.capabilities.may_deliver(id), "action not granted");
        }
        if let Event::Shift { .. } = event {
            ensure!(state.capabilities.may_read_input(), "input not granted");
        }
        if let Event::Resized { size } = event {
            state.gui = Some(*size);
        }
        state.action = match event {
            Event::Action { id, .. } | Event::SecondaryAction { id, .. } => Some(id.clone()),
            _ => None,
        };
        if state.epoch != epoch || matches!(event, Event::Epoch) {
            state.presentation.reset_presentation();
        }
        state.epoch = epoch;
        state.begin_output()?;
        state.presentation.begin_presentation();
        crate::execution::arm(&mut self.store, CALLBACK_FUEL, false)?;
        let called = self.call(event);
        self.last_fuel = self.fuel_used();
        if let Err(error) = called {
            self.store.data_mut().commands.clear();
            self.restart();
            return Err(error);
        }
        self.store.data_mut().presentation.commit_presentation();
        Ok(self.take_transaction())
    }

    /// Sets the open modal's size that `ui.modal-size` returns from the next callback on; none
    /// while the modal is closed.
    pub fn set_modal_size(&mut self, size: Option<screen::GuiSize>) {
        self.store.data_mut().gui = size;
    }

    /// Sets the session's items that `items` reads from the next callback on.
    pub fn set_session(&mut self, session: Arc<SessionData>) {
        self.store.data_mut().session = session;
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
    #[expect(
        clippy::print_stderr,
        reason = "the server helper's stderr lines are drained into the client's log"
    )]
    fn restart(&mut self) {
        self.presentation_pending = None;
        self.presentation_sent = None;
        let state = self.store.data();
        let (owner, capabilities, epoch) =
            (state.owner.clone(), state.capabilities.clone(), state.epoch);
        let session = Arc::clone(&state.session);
        let bundle = owner.bundle.clone();
        let assets = state.presentation.asset_files.clone();
        match Self::start(
            &self.linker,
            &self.component,
            owner,
            capabilities,
            epoch,
            assets,
        ) {
            Ok((store, exports)) => {
                (self.store, self.exports) = (store, exports);
                self.store.data_mut().commands.clear();
                self.set_session(session);
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
            (Event::Frame, _) => match &self.exports.frame {
                Some(frame) => {
                    let frame = frame.typed::<(), ()>(&*store)?;
                    frame.call(&mut *store, ())?;
                    frame.post_return(store)
                }
                None => Ok(()),
            },
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
            (Event::Shift { held }, _) => match &self.exports.shift {
                Some(shift) => {
                    let shift = shift.typed::<(bool,), ()>(&*store)?;
                    shift.call(&mut *store, (*held,))?;
                    shift.post_return(store)
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

    /// Supplies the native frame fence and admitted actor references before a callback.
    pub fn set_drawing_context(&mut self, context: &crate::presentation::Context) -> Result<()> {
        ensure!(
            context.candidates.len() <= mod_api::MAX_DRAW_ENTITIES
                && context.live.len() <= mod_api::MAX_DRAW_ENTITIES * 2
                && context.targets.len() <= mod_api::drawing::MAX_MASK_ENTITIES,
            "drawing context budget exceeded"
        );
        self.store
            .data_mut()
            .presentation
            .presentation_context(context)
    }

    /// Returns changed drawing only; bundles without drawing authority publish no presentation.
    pub(crate) fn take_presentation(
        &mut self,
        byte_allowance: usize,
    ) -> Result<Option<Box<crate::presentation::Chunk>>> {
        let state = self.store.data();
        if !(state.presentation.grants.world_draw
            || state.presentation.grants.render_post
            || state.presentation.grants.entity_highlight)
        {
            return Ok(None);
        }
        let key = state.presentation.presentation_key();
        if self.presentation_pending.is_none() && self.presentation_sent != Some(key) {
            self.presentation_pending = Some(crate::presentation::Pending::new(
                &state.presentation.presentation_output(),
                state.capabilities.scope.memory_bytes,
            )?);
            self.presentation_sent = Some(key);
        }
        let Some(pending) = &mut self.presentation_pending else {
            return Ok(None);
        };
        let chunk = pending.take(byte_allowance).map(Box::new);
        if pending.complete() {
            self.presentation_pending = None;
        }
        Ok(chunk)
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

/// Narrows the canonical personal drawing host to this bundle's consented capabilities.
pub fn drawing_grants(capabilities: &Capabilities) -> crate::ModGrants {
    let has = |permission| capabilities.scope.permissions.contains(&permission);
    crate::ModGrants {
        world_draw: has(Permission::WorldDraw),
        entity_observe: has(Permission::EntityObserve),
        entity_highlight: has(Permission::EntityHighlight),
        render_through_walls: has(Permission::RenderThroughWalls),
        render_material: has(Permission::RenderMaterial),
        render_post: has(Permission::RenderPost),
        environment: has(Permission::WorldDraw),
        ..Default::default()
    }
}
