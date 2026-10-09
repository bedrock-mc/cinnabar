use crate::{
    CameraDelta, FRAME_FUEL, GameplayCameraRig, GameplayMob, GameplaySnapshot, MAX_LABEL_BYTES,
    MEMORY_BYTES, ModCue, ModGrants, PlayerStateSnapshot,
};
use anyhow::{Result, bail};
use wasmtime::{
    Engine, Store, StoreLimits, StoreLimitsBuilder,
    component::{Component, HasSelf, Linker},
};

wasmtime::component::bindgen!({
    path: "../mod-api/wit", world: "extension", imports: { default: trappable },
    additional_derives: [PartialEq],
});

const MAX_IMPORT_WRITES: u32 = 8;
#[path = "block_highlights.rs"]
mod block_highlights;
#[path = "camera.rs"]
mod camera;
#[path = "controls.rs"]
mod controls;
#[path = "gameplay.rs"]
mod gameplay;
#[path = "hud.rs"]
mod hud;
#[path = "item_use.rs"]
mod item_use;
#[path = "player_state.rs"]
mod player_state;
#[path = "render.rs"]
mod render;

struct State {
    limits: StoreLimits,
    pressed: bool,
    label: Option<String>,
    hud: hud::HudState,
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
    player_state: player_state::PlayerState,
    render: render::RenderState,
    block_highlights: block_highlights::HighlightState,
}

impl State {
    fn new(grants: ModGrants, settings: String) -> Self {
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
            hud: hud::HudState::default(),
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
            player_state: player_state::PlayerState::default(),
            render: render::RenderState::new(),
            block_highlights: block_highlights::HighlightState::default(),
        }
    }
}

impl cinnabar::extension::hud::Host for State {
    fn open_editor(&mut self, json: String) -> Result<Result<(), String>> {
        hud::open_editor(self, json)
    }

    fn read_editor_result(
        &mut self,
    ) -> Result<Result<Option<cinnabar::extension::hud::EditorResult>, String>> {
        hud::read_editor_result(self)
    }
    fn set_content(&mut self, json: String) -> Result<Result<(), String>> {
        hud::set_content(self, json)
    }

    fn set_crosshair(&mut self, json: String) -> Result<Result<(), String>> {
        hud::set_crosshair(self, json)
    }

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
    guest: Extension,
    pub(super) active: bool,
}

impl Instance {
    /// Initializes a candidate store without changing the published instance.
    pub(super) fn new(
        engine: &Engine,
        bytes: &[u8],
        grants: ModGrants,
        settings: String,
    ) -> Result<Self> {
        let component = Component::new(engine, bytes)?;
        let mut linker = Linker::new(engine);
        Extension::add_to_linker::<_, HasSelf<_>>(&mut linker, |state: &mut State| state)?;
        let state = State::new(grants, settings);
        let mut store = Store::new(engine, state);
        store.limiter(|state| &mut state.limits);
        store.set_fuel(FRAME_FUEL)?;
        let guest = Extension::instantiate(&mut store, &component, &linker)?;
        guest.call_init(&mut store)?;
        commit(&mut store);
        Ok(Self {
            store,
            guest,
            active: true,
        })
    }

    /// Restores the call budget and commits output only on successful return.
    pub(super) fn frame(
        &mut self,
        pressed: bool,
        snapshot: Option<GameplaySnapshot>,
        mobs: Vec<GameplayMob>,
        player_state: Option<PlayerStateSnapshot>,
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
        state.player_state.begin_frame();
        state.camera_policy = camera::CameraPolicy::default();
        state.item_use_policy = item_use::ItemUsePolicy::default();
        if !self.active {
            state.player_state.revoke();
            return Ok(());
        }
        let validation = gameplay::validate_snapshot(snapshot.as_ref())
            .and_then(|()| gameplay::validate_mobs(snapshot.as_ref(), &mobs))
            .and_then(|()| player_state::validate(player_state.as_ref()))
            .and_then(|()| controls::validate_frame(&controls));
        if let Err(error) = validation {
            state.player_state.revoke();
            return Err(error);
        }
        let snapshot_seconds = snapshot.as_ref().map_or(0.0, |frame| frame.frame_seconds);
        let state = self.store.data_mut();
        state.pressed = pressed;
        state.writes = 0;
        state.environment_writes = 0;
        state.gameplay_reads = 0;
        state.camera_writes = 0;
        state.snapshot = snapshot;
        state.world.advance_command_window(snapshot_seconds);
        state.world.mobs = mobs;
        state.player_state.set_snapshot(player_state)?;
        state.controls.frame = controls;
        if let Err(error) = self.store.set_fuel(FRAME_FUEL) {
            self.store.data_mut().player_state.revoke();
            return Err(error);
        }
        if let Err(error) = self.guest.call_frame(&mut self.store) {
            self.active = false;
            self.store.data_mut().pending = None;
            self.store.data_mut().label = None;
            self.store.data_mut().hud = hud::HudState::default();
            self.store.data_mut().pending_time = None;
            self.store.data_mut().time_override = None;
            self.store.data_mut().fullbright = false;
            self.store.data_mut().pending_fullbright = None;
            self.store.data_mut().snapshot = None;
            self.store.data_mut().pending_camera = None;
            self.store.data_mut().camera_delta = None;
            self.store.data_mut().controls.revoke();
            self.store.data_mut().render.revoke();
            self.store.data_mut().block_highlights.revoke();
            self.store.data_mut().world = gameplay::WorldState::default();
            self.store.data_mut().player_state.revoke();
            self.store.data_mut().camera_policy = camera::CameraPolicy::default();
            self.store.data_mut().item_use_policy = item_use::ItemUsePolicy::default();
            self.store.data_mut().packet_delay_ms = 0;
            self.store.data_mut().pending_packet_delay = None;
            self.store.data_mut().show_real_position = false;
            self.store.data_mut().pending_show_real_position = None;
            bail!("mod quarantined after a guest trap: {error:#}");
        }
        commit(&mut self.store);
        self.store.data_mut().snapshot = None;
        self.store.data_mut().player_state.begin_frame();
        self.store.data_mut().world.mobs = Vec::new();
        self.store.data_mut().world.incoming = Vec::new();
        self.store.data_mut().controls.frame = crate::empty_controls();
        Ok(())
    }

    pub(super) fn item_use_delay_fix(&self) -> Option<(u64, i32)> {
        self.store.data().item_use_policy.committed
    }

    pub(super) fn preserves_teleport_rotation(&self) -> bool {
        self.store.data().camera_policy.committed
    }

    /// Only a successful, focused gameplay callback publishes a view multiplier.
    pub(super) fn camera_view_scale(&self) -> Option<[f32; 2]> {
        self.store.data().camera_policy.committed_view_scale
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

    pub(super) fn hud(&self) -> Option<&ui::mod_hud::Hud> {
        self.store.data().hud.content.as_ref()
    }

    /// Moves a committed preview into the native editor without a guest call.
    pub(super) fn take_hud_editor_request(&mut self) -> Option<ui::mod_hud::Hud> {
        self.store.data_mut().hud.editor_request.take()
    }

    /// Makes one host result available to this instance's next callback.
    pub(super) fn deliver_hud_editor_result(&mut self, result: ui::mod_hud::EditorResult) {
        self.store.data_mut().hud.editor_result = Some(result);
    }

    pub(super) fn crosshair(&self) -> Option<&ui::mod_hud::Crosshair> {
        self.store.data().hud.crosshair.as_ref()
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
}

/// Publishes retained presentation changes after the entire callback succeeds.
fn commit(store: &mut Store<State>) {
    let state = store.data_mut();
    state.hud.commit();
    state.controls.commit();
    state.render.commit();
    state.block_highlights.commit();
    state.world.commit();
    state.camera_policy.committed = state.camera_policy.pending && state.snapshot.is_some();
    state.camera_policy.committed_view_scale = (state.snapshot.is_some()
        && state.controls.frame.focused
        && !state.controls.frame.panel_open)
        .then_some(state.camera_policy.pending_view_scale)
        .flatten();
    state.item_use_policy.commit(state.snapshot.as_ref());
    state.camera_delta = state.pending_camera.take();
    if let Some(delay) = state.pending_packet_delay.take() {
        state.packet_delay_ms = delay;
    }
    if let Some(show) = state.pending_show_real_position.take() {
        state.show_real_position = show;
    }
    if let Some(enabled) = state.pending_fullbright.take() {
        state.fullbright = enabled;
    }
    if let Some(ticks) = state.pending_time.take() {
        state.time_override = ticks;
    }
    if let Some(text) = state.pending.take() {
        state.label = (!text.is_empty()).then_some(text);
    }
}
