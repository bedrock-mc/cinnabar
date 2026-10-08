use crate::{
    CameraDelta, FRAME_FUEL, GameplayCameraRig, GameplayMob, GameplaySnapshot, MAX_LABEL_BYTES,
    MEMORY_BYTES, ModCue, ModEvent, ModGrants, ModScreens,
};
use anyhow::{Result, bail};
use server_experience::{
    runtime::{CALLBACK_FUEL, LOAD_FUEL},
    screen::ScreenLayout as HostLayout,
    session_data::SessionData,
};
use std::{collections::BTreeSet, sync::Arc};
use wasmtime::{
    Engine, Store, StoreLimits, StoreLimitsBuilder,
    component::{Component, HasSelf, Linker},
};

// `player-mod` includes `extension`, so one linker serves both worlds' components.
wasmtime::component::bindgen!({
    path: [
        "../experience-sdk/wit/client/deps/server-experience",
        "../experience-sdk/wit/session",
        "../mod-api/wit",
    ],
    world: "cinnabar:extension/player-mod@0.1.0",
    imports: { default: trappable },
    additional_derives: [PartialEq],
});

const MAX_IMPORT_WRITES: u32 = 8;
#[path = "block_highlights.rs"]
mod block_highlights;
#[path = "camera.rs"]
mod camera;
#[path = "controls.rs"]
mod controls;
mod exports;
#[path = "gameplay.rs"]
mod gameplay;
mod player_mod;
#[path = "item_use.rs"]
mod item_use;
#[path = "render.rs"]
mod render;

/// What a package declares that the host checks guest output and events against.
#[derive(Clone, Debug, Default)]
pub(crate) struct Declared {
    pub(crate) templates: BTreeSet<String>,
    pub(crate) actions: BTreeSet<String>,
    pub(crate) keys: BTreeSet<String>,
}

struct State {
    limits: StoreLimits,
    pressed: bool,
    label: Option<String>,
    pending: Option<String>,
    writes: u32,
    grants: ModGrants,
    time_override: Option<u32>,
    pending_time: Option<Option<u32>>,
    fullbright: bool,
    pending_fullbright: Option<bool>,
    environment_writes: u32,
    snapshot: Option<GameplaySnapshot>,
    gameplay_reads: u32,
    camera_writes: u32,
    pending_camera: Option<CameraDelta>,
    camera_delta: Option<CameraDelta>,
    packet_delay_ms: u32,
    pending_packet_delay: Option<u32>,
    packet_delay_writes: u32,
    show_real_position: bool,
    pending_show_real_position: Option<bool>,
    controls: controls::ControlState,
    world: gameplay::WorldState,
    camera_policy: camera::CameraPolicy,
    item_use_policy: item_use::ItemUsePolicy,
    render: render::RenderState,
    declared: Declared,
    layout: Option<HostLayout>,
    session: Arc<SessionData>,
    screens: ModScreens,
    /// This callback's screen output, applied to a copy of `screens`; committed on return.
    pending_screens: Option<ModScreens>,
    /// Host calls and screen output bytes of the running callback.
    calls: usize,
    output: usize,
    block_highlights: block_highlights::HighlightState,
}

impl State {
    fn new(grants: ModGrants, settings: String, declared: Declared) -> Self {
        Self {
            limits: StoreLimitsBuilder::new()
                .memory_size(MEMORY_BYTES)
                .table_elements(4096)
                .instances(16)
                .memories(1)
                .tables(2)
                .trap_on_grow_failure(true)
                .build(),
            pressed: false,
            label: None,
            pending: None,
            writes: 0,
            grants,
            time_override: None,
            pending_time: None,
            fullbright: false,
            pending_fullbright: None,
            environment_writes: 0,
            snapshot: None,
            gameplay_reads: 0,
            camera_writes: 0,
            pending_camera: None,
            camera_delta: None,
            packet_delay_ms: 0,
            pending_packet_delay: None,
            packet_delay_writes: 0,
            show_real_position: false,
            pending_show_real_position: None,
            controls: controls::ControlState::new(settings),
            world: gameplay::WorldState::default(),
            camera_policy: camera::CameraPolicy::default(),
            item_use_policy: item_use::ItemUsePolicy::default(),
            render: render::RenderState::new(),
            declared,
            layout: None,
            session: Arc::default(),
            screens: ModScreens::default(),
            pending_screens: None,
            calls: 0,
            output: 0,
            block_highlights: block_highlights::HighlightState::default(),
        }
    }
}

impl cinnabar::extension::hud::Host for State {
    /// Stages bounded plain text; nothing is published until the guest returns.
    fn set_label(&mut self, text: String) -> Result<Result<(), String>> {
        self.writes += 1;
        if self.writes > MAX_IMPORT_WRITES {
            bail!("HUD import budget exhausted");
        }
        if text.len() > MAX_LABEL_BYTES || text.chars().any(|c| c.is_control() || c == '§') {
            return Ok(Err("label must be short plain text".into()));
        }
        self.pending = Some(text);
        Ok(Ok(()))
    }
}

impl cinnabar::extension::environment::Host for State {
    fn set_fullbright(&mut self, enabled: bool) -> Result<Result<(), String>> {
        self.environment_writes += 1;
        if self.environment_writes > MAX_IMPORT_WRITES {
            bail!("environment import budget exhausted");
        }
        if !self.grants.fullbright {
            return Ok(Err("fullbright capability denied".into()));
        }
        self.pending_fullbright = Some(enabled);
        Ok(Ok(()))
    }

    /// Stages a fixed visual clock only with explicit authority and a valid tick.
    fn set_time_override(&mut self, ticks: Option<u32>) -> Result<Result<(), String>> {
        self.environment_writes += 1;
        if self.environment_writes > MAX_IMPORT_WRITES {
            bail!("environment import budget exhausted");
        }
        if !self.grants.environment {
            return Ok(Err("environment capability denied".into()));
        }
        if ticks.is_some_and(|tick| tick >= mod_api::BEDROCK_DAY_TICKS) {
            return Ok(Err("time override must be within one Bedrock day".into()));
        }
        self.pending_time = Some(ticks);
        Ok(Ok(()))
    }
}

impl cinnabar::extension::input::Host for State {
    /// Reads only the host's one-frame action edge, never raw keyboard state.
    fn demo_pressed(&mut self) -> Result<bool> {
        Ok(self.pressed)
    }

    fn read_controls(&mut self) -> Result<Result<crate::ControlFrame, String>> {
        controls::read(self)
    }

    fn reserve_keys(&mut self, keys: Vec<String>) -> Result<Result<(), String>> {
        controls::reserve(self, keys)
    }
}

pub(super) struct Instance {
    store: Store<State>,
    exports: exports::Exports,
    pub(super) active: bool,
    /// Fuel the last `init` or callback consumed.
    last_fuel: u64,
}

impl Instance {
    /// Initializes a candidate store without changing the published instance. An `extension`
    /// component's instantiation and `init` share `FRAME_FUEL`; a `player-mod` component's
    /// `init`, which may read the whole session, gets `LOAD_FUEL`.
    pub(super) fn new(
        engine: &Engine,
        bytes: &[u8],
        grants: ModGrants,
        settings: String,
        declared: Declared,
    ) -> Result<Self> {
        let component = Component::new(engine, bytes)?;
        let mut linker = Linker::new(engine);
        PlayerMod::add_to_linker::<_, HasSelf<_>>(&mut linker, |state: &mut State| state)?;
        let state = State::new(grants, settings, declared);
        let mut store = Store::new(engine, state);
        store.limiter(|state| &mut state.limits);
        store.set_fuel(FRAME_FUEL)?;
        let instance = linker.instantiate(&mut store, &component)?;
        let (init, exports) = exports::Exports::find(&mut store, &instance)?;
        let budget = if exports.loads_session() {
            store.set_fuel(LOAD_FUEL)?;
            LOAD_FUEL
        } else {
            FRAME_FUEL
        };
        init.call(&mut store, ())?;
        init.post_return(&mut store)?;
        commit(&mut store);
        let last_fuel = budget - store.get_fuel().unwrap_or(0).min(budget);
        Ok(Self {
            store,
            exports,
            active: true,
            last_fuel,
        })
    }

    /// Restores the call budget and commits output only on successful return.
    pub(super) fn frame(
        &mut self,
        pressed: bool,
        snapshot: Option<GameplaySnapshot>,
        mobs: Vec<GameplayMob>,
        controls: crate::ControlFrame,
    ) -> Result<()> {
        let state = self.store.data_mut();
        state.snapshot = None;
        state.pending_camera = None;
        state.camera_delta = None;
        state.pending_packet_delay = None;
        state.packet_delay_writes = 0;
        state.pending_show_real_position = None;
        state.pending_fullbright = None;
        state.controls.begin_frame();
        state.render.begin_frame();
        state.block_highlights.begin_frame();
        state.world.begin_frame();
        state.camera_policy = camera::CameraPolicy::default();
        state.item_use_policy = item_use::ItemUsePolicy::default();
        if !self.active {
            return Ok(());
        }
        gameplay::validate_snapshot(snapshot.as_ref())?;
        gameplay::validate_mobs(snapshot.as_ref(), &mobs)?;
        controls::validate_frame(&controls)?;
        let snapshot_seconds = snapshot.as_ref().map_or(0.0, |frame| frame.frame_seconds);
        let state = self.store.data_mut();
        state.pressed = pressed;
        state.snapshot = snapshot;
        state.world.advance_command_window(snapshot_seconds);
        state.world.mobs = mobs;
        state.controls.frame = controls;
        let result = self.run(FRAME_FUEL, commit, |exports, store| exports.frame(store));
        self.store.data_mut().snapshot = None;
        self.store.data_mut().world.mobs = Vec::new();
        self.store.data_mut().world.incoming = Vec::new();
        self.store.data_mut().controls.frame = crate::empty_controls();
        result
    }

    /// Delivers one event callback, `data-changed` with `LOAD_FUEL` and every other event with
    /// `CALLBACK_FUEL`. An `extension` component has no event exports and receives nothing.
    pub(super) fn dispatch(&mut self, event: &ModEvent) -> Result<()> {
        if !self.active {
            return Ok(());
        }
        self.store.data().check_event(event)?;
        let fuel = match event {
            ModEvent::DataChanged(_) => LOAD_FUEL,
            _ => CALLBACK_FUEL,
        };
        self.run(fuel, commit_event, |exports, store| {
            exports.event(store, event)
        })
    }

    /// Runs one callback with `fuel` and commits its output with `publish`; a trap discards
    /// it and quarantines the guest, removing everything it presented.
    fn run(
        &mut self,
        fuel: u64,
        publish: fn(&mut Store<State>),
        call: impl FnOnce(&exports::Exports, &mut Store<State>) -> Result<()>,
    ) -> Result<()> {
        let state = self.store.data_mut();
        state.writes = 0;
        state.environment_writes = 0;
        state.gameplay_reads = 0;
        state.camera_writes = 0;
        state.calls = 0;
        state.output = 0;
        state.pending_screens = None;
        self.store.set_fuel(fuel)?;
        let called = call(&self.exports, &mut self.store);
        self.last_fuel = fuel - self.store.get_fuel().unwrap_or(0).min(fuel);
        if let Err(error) = called {
            self.quarantine();
            bail!("mod quarantined after a guest trap: {error:#}");
        }
        publish(&mut self.store);
        Ok(())
    }

    /// Disables callbacks and drops everything the guest presented or staged.
    pub(super) fn quarantine(&mut self) {
        self.active = false;
        let state = self.store.data_mut();
        state.pending = None;
        state.label = None;
        state.pending_time = None;
        state.time_override = None;
        state.fullbright = false;
        state.pending_fullbright = None;
        state.snapshot = None;
        state.pending_camera = None;
        state.camera_delta = None;
        state.controls.revoke();
        state.render.revoke();
        state.block_highlights.revoke();
        state.world = gameplay::WorldState::default();
        state.camera_policy = camera::CameraPolicy::default();
        state.item_use_policy = item_use::ItemUsePolicy::default();
        state.packet_delay_ms = 0;
        state.pending_packet_delay = None;
        state.show_real_position = false;
        state.pending_show_real_position = None;
        state.pending_screens = None;
        state.screens = ModScreens::default();
    }

    pub(super) fn item_use_delay_fix(&self) -> Option<(u64, i32)> {
        self.store.data().item_use_policy.committed
    }

    pub(super) fn preserves_teleport_rotation(&self) -> bool {
        self.store.data().camera_policy.committed
    }

    pub(super) fn camera_rig(&self) -> Option<GameplayCameraRig> {
        self.store.data().world.rig
    }

    pub(super) fn take_commands(&mut self) -> Vec<String> {
        std::mem::take(&mut self.store.data_mut().world.commands)
    }

    /// Cues the next callback may poll; they last exactly one callback.
    pub(super) fn deliver_cues(&mut self, cues: Vec<ModCue>) {
        self.store.data_mut().world.incoming = gameplay::incoming(cues);
    }

    pub(super) fn take_cues(&mut self) -> Vec<ModCue> {
        std::mem::take(&mut self.store.data_mut().world.cues)
    }

    pub(super) fn take_camera_delta(&mut self) -> Option<CameraDelta> {
        self.store.data_mut().camera_delta.take()
    }

    pub(super) fn packet_delay_ms(&self) -> u32 {
        self.store.data().packet_delay_ms
    }
    pub(super) fn fullbright(&self) -> bool {
        self.store.data().fullbright
    }
    pub(super) fn block_highlights(&self) -> Option<&mod_api::BlockHighlightSpec> {
        self.store.data().block_highlights.committed.as_ref()
    }
    pub(super) fn show_real_position(&self) -> bool {
        self.store.data().show_real_position
    }

    /// Reads the committed presentation clock without entering the component.
    pub(super) fn time_override(&self) -> Option<u32> {
        self.store.data().time_override
    }

    /// Reads retained UI without entering the component.
    pub(super) fn label(&self) -> Option<&str> {
        self.store.data().label.as_deref()
    }

    pub(super) fn panel(&self) -> Option<&ui::mod_panel::Panel> {
        self.store.data().controls.panel.as_ref()
    }
    pub(super) fn panel_open(&self) -> bool {
        self.store.data().controls.open
    }
    pub(super) fn set_panel_open(&mut self, open: bool) {
        let state = self.store.data_mut();
        state.controls.open =
            open && self.active && state.grants.controls && state.controls.panel.is_some();
    }
    pub(super) fn reserved_keys(&self) -> &[String] {
        &self.store.data().controls.keys
    }
    pub(super) fn take_interaction(&mut self) -> crate::InteractionOutput {
        std::mem::take(&mut self.store.data_mut().controls.interaction)
    }
    pub(super) fn settings_write(&self) -> Option<&str> {
        self.store.data().controls.dirty_settings.as_deref()
    }
    pub(super) fn settings_written(&mut self) {
        self.store.data_mut().controls.dirty_settings = None;
    }

    pub(super) fn render(&self) -> (&mod_render::RenderOutput, u64) {
        let render = &self.store.data().render;
        (render.output(), render.generation())
    }

    pub(super) fn settings(&self) -> &str {
        self.store.data().controls.settings()
    }

    pub(super) fn screens(&self) -> &ModScreens {
        &self.store.data().screens
    }

    /// Whether the component exports `player-mod`'s event callbacks.
    pub(super) fn has_events(&self) -> bool {
        self.exports.has_events()
    }

    /// The open container screen's layout that `screen.layout` returns; none also closes the
    /// view, as closing the container returns from it.
    pub(super) fn set_layout(&mut self, layout: Option<HostLayout>) {
        let state = self.store.data_mut();
        if layout.is_none() && state.screens.view.is_some() {
            state.screens.view = None;
            state.screens.data.revision += 1;
        }
        state.layout = layout;
    }

    /// Closes the view without the guest, as Escape does; `true` when one was open.
    pub(super) fn close_view(&mut self) -> bool {
        let screens = &mut self.store.data_mut().screens;
        let open = screens.view.take().is_some();
        if open {
            screens.data.revision += 1;
        }
        open
    }

    pub(super) fn view_open(&self) -> bool {
        self.store.data().screens.view.is_some()
    }

    pub(super) fn last_fuel(&self) -> u64 {
        self.last_fuel
    }

    pub(super) fn set_session(&mut self, session: Arc<SessionData>) {
        self.store.data_mut().session = session;
    }
}

/// Publishes retained presentation changes after the entire callback succeeds.
fn commit(store: &mut Store<State>) {
    let state = store.data_mut();
    state.render.commit();
    state.block_highlights.commit();
    state.world.commit();
    state.camera_policy.committed = state.camera_policy.pending && state.snapshot.is_some();
    state.item_use_policy.commit(state.snapshot.as_ref());
    state.camera_delta = state.pending_camera.take();
    if let Some(delay) = state.pending_packet_delay.take() {
        state.packet_delay_ms = delay;
    }
    if let Some(show) = state.pending_show_real_position.take() {
        state.show_real_position = show;
    }
    commit_event(store);
}

/// Publishes what an event callback may retain: the label, visual time, panel, settings and
/// screens. Render, camera and command output belongs to `frame` alone, so an event never
/// replaces the frame's drawn set; what an event stages there is dropped by the next frame.
fn commit_event(store: &mut Store<State>) {
    let state = store.data_mut();
    state.controls.commit();
    if let Some(enabled) = state.pending_fullbright.take() {
        state.fullbright = enabled;
    }
    if let Some(ticks) = state.pending_time.take() {
        state.time_override = ticks;
    }
    if let Some(text) = state.pending.take() {
        state.label = (!text.is_empty()).then_some(text);
    }
    if let Some(screens) = state.pending_screens.take() {
        state.screens = screens;
    }
}
