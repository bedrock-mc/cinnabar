//! Developer helper supervision; only implemented adapters receive grants.

use super::worker::Worker;
use anyhow::{Result, ensure};
use mod_host::helper::{CallFailure, Dispatch, Event, FailureKind, Helper, Reply};
use server_experience::{
    bundle::VerifiedBundle,
    manifest::{Manifest, Permission, developer_permissions},
    negotiation::Grant,
    policy::*,
    rate_limit::{RateClass, RateExceeded, RateLimit},
    runtime::{Budget, CALLBACK_INTERVAL_MS, Capabilities, Command, Contributions, Principal},
    screen::{self, GuiSize},
    session::Control,
    session_data::{SessionData, SessionItems},
    wire::{self, Direction, Envelope, Ingress},
};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    path::{Path, PathBuf},
    sync::Arc,
};

mod callbacks;
mod completion;
mod modal;
mod scheduling;
use scheduling::CallbackQueue;

struct Instance<H> {
    helper: Option<H>,
    assets: BTreeMap<String, Vec<u8>>,
    presentation: mod_host::presentation::Receiver,
    component: Option<Vec<u8>>,
    capabilities: Capabilities,
    owner: Principal,
    contributions: Contributions,
    busy: bool,
    /// Whether the one-time initializer finished, independently of later callbacks.
    initialized: bool,
    /// The caller-owned event awaiting an acknowledgement from this helper.
    pending: Option<u64>,
    /// While set, only registry confirmation may run guest code.
    waiting_for: Option<String>,
    /// The bundle's verified templates and textures, shared with the modal presenter.
    files: Arc<screen::Files>,
    /// When its modal last opened, so the most recent one draws on top.
    opened: u64,
    /// All callbacks retained for this recipient until acknowledged.
    queue: CallbackQueue,
    /// The world epoch of its pending callback, which that callback's transaction carries.
    epoch: u64,
    /// When its recent callbacks failed, in milliseconds, oldest first.
    strikes: VecDeque<u64>,
    /// Why the client part was stopped, which the trusted status says; it runs no more.
    stopped: Option<&'static str>,
    /// The guest export of its pending callback, for the log.
    callback: &'static str,
    /// The revision of the session's items its helper last received.
    items_sent: Option<u64>,
    /// The delivered or queued Shift state; None means the guest needs synchronization.
    shift: Option<bool>,
    drawing_context: mod_host::presentation::Context,
    pending_drawing_context: mod_host::presentation::Context,
    /// The helper's latest held references, independent of snapshot transfer progress.
    drawing_references: Vec<mod_render::retained::EntityKey>,
}

impl<H: Worker> Instance<H> {
    /// Hands `event` to the helper, with the session's items when the guest may read them and
    /// they changed since the helper last received them.
    fn dispatch(
        &mut self,
        session: Option<&SessionData>,
        event: Event,
        epoch: u64,
        gui: Option<GuiSize>,
        pending: Option<u64>,
    ) -> Result<()> {
        let Some(helper) = &mut self.helper else {
            return Ok(());
        };
        let items = session
            .filter(|session| {
                self.capabilities
                    .scope
                    .permissions
                    .contains(&Permission::Items)
                    && self.items_sent != Some(session.item_revision)
            })
            .map(SessionItems::from);
        let revision = items.as_ref().map(|items| items.revision);
        self.drawing_context.check(epoch)?;
        self.callback = event.callback();
        self.pending_drawing_context = self.drawing_context.clone();
        helper.dispatch(Dispatch {
            drawing: self.drawing_context.clone(),
            event,
            epoch,
            gui,
            items,
        })?;
        self.items_sent = revision.or(self.items_sent);
        self.busy = true;
        self.pending = pending;
        self.epoch = epoch;
        Ok(())
    }
}

impl<H> Instance<H> {
    /// Counts the failed callback at `now_ms` and reports whether `MAX_GUEST_STRIKES` failed
    /// within `GUEST_STRIKE_WINDOW_MS`, as the server adapter counts its strikes.
    fn strike(&mut self, now_ms: u64) -> bool {
        self.strikes.push_back(now_ms);
        while self
            .strikes
            .front()
            .is_some_and(|&at| now_ms.saturating_sub(at) >= GUEST_STRIKE_WINDOW_MS)
        {
            self.strikes.pop_front();
        }
        self.strikes.len() >= MAX_GUEST_STRIKES
    }

    /// Ends the client part for `why`: its helper, its contributions and its waiting events go,
    /// and so does its budget. Invalidates the cached scene when contributions are cleared.
    fn stop(&mut self, budget: &mut Budget, scene_revision: &mut u64, why: &'static str) {
        self.helper = None;
        self.component = None;
        self.busy = false;
        self.initialized = true;
        self.pending = None;
        self.waiting_for = None;
        self.queue.clear();
        self.contributions = Contributions::default();
        self.presentation.clear();
        self.drawing_references.clear();
        self.assets.clear();
        *scene_revision += 1;
        budget.quarantine(&self.owner);
        self.stopped = Some(why);
    }
}

/// What the trusted status says of a client part stopped after `MAX_GUEST_STRIKES` failures.
const STOPPED_AFTER_ERRORS: &str = "after repeated errors";
/// What the trusted status says of a client part that failed to start.
const STOPPED_AT_START: &str = "because it failed to start";
/// What the trusted status says when a helper cannot continue.
const STOPPED_AT_HELPER_FAILURE: &str = "because its helper failed";
/// What the trusted status says when reliable callback retention cannot continue.
const STOPPED_AT_OVERFLOW: &str = "because its callback queue overflowed";

pub(super) struct Live<H = Helper> {
    grant: Grant,
    media: super::media::Media,
    screens: Vec<render_scene::MediaScreen>,
    /// Scene and frame revisions `screens` was built from.
    screens_built: Option<(u64, u64)>,
    scene_revision: u64,
    instances: BTreeMap<String, Instance<H>>,
    /// The last dispatched recipient, so the next slice starts with its successor.
    last_recipient: Option<String>,
    executable: PathBuf,
    pending_sends: VecDeque<(Envelope, usize)>,
    pending_send_bytes: usize,
    budget: Budget,
    ingress: Ingress,
    egress: RateLimit,
    sequence: u64,
    slice_ms: u64,
    ready: bool,
    epoch: u64,
    /// The latest epoch notification, retained until its encoded cost is admitted.
    pending_control: Option<Control>,
    /// Counts modal openings across bundles.
    modal_order: u64,
    /// Last native world epoch, tracked separately from server-requested logical epochs.
    native_epoch: u64,
    /// Minimum logical epoch requested by the server since the previous poll.
    requested_epoch: Option<u64>,
    /// The drawn open modal's size and its bundle, which dispatches to that bundle carry.
    gui: Option<(GuiSize, String)>,
    drawing_fence: Option<(u64, u64, i32)>,
    drawing: mod_render::RenderOutput,
    drawing_generation: u64,
    drawing_sources: Vec<u64>,
    drawing_merge: mod_render::RenderMerge,
    /// The last bundle given a frame callback, rotating each presentation slice.
    frame_cursor: Option<String>,
    /// The session's items, for guests granted `items`.
    session: Option<Arc<SessionData>>,
}

impl<H: Worker> Live<H> {
    /// Launches only developer helpers; unsupported required presentation remains denied.
    pub(super) fn start(
        grant: Grant,
        bundles: Vec<VerifiedBundle>,
        context: &mod_host::presentation::Context,
        now_ms: u64,
        executable: &Path,
        media_helper: &Path,
    ) -> Result<Self> {
        context.check(context.epoch)?;
        let epoch = context.epoch;
        let mut budget = Budget::default();
        budget.begin_slice();
        let mut instances = BTreeMap::new();
        let mut media = super::media::Media::new(grant.clone(), epoch, media_helper.to_owned());
        let mut channels = 0;
        let mut scene_revision = 0;
        for mut bundle in bundles {
            let owner = Principal {
                session: grant.session.clone(),
                bundle: bundle.manifest.id.clone(),
                generation: INITIAL_BUNDLE_GENERATION,
            };
            let assets = bundle.paths().map(str::to_owned).collect();
            let capabilities = capabilities(&grant, &bundle.manifest, assets);
            let reservation = budget.reserve(
                owner.clone(),
                capabilities.scope.memory_bytes,
                capabilities.scope.gpu_bytes,
            );
            let assets = if capabilities
                .scope
                .permissions
                .contains(&Permission::WorldDraw)
            {
                bundle
                    .paths()
                    .filter(|path| Some(*path) != bundle.manifest.component.as_deref())
                    .filter_map(|path| Some((path.to_owned(), bundle.file(path)?.to_vec())))
                    .collect()
            } else {
                BTreeMap::new()
            };
            let admission = reservation.and_then(|()| {
                ensure!(
                    assets.values().map(Vec::len).sum::<usize>() as u64
                        <= capabilities.scope.memory_bytes,
                    "drawing asset memory budget exceeded"
                );
                ensure!(
                    channels + capabilities.channels.len() <= MAX_CHANNELS,
                    "aggregate channel limit exceeded"
                );
                channels += capabilities.channels.len();
                Ok(())
            });
            let files = bundle.take_screen_files();
            let component = bundle.take_component();
            media.register(bundle);
            let busy = component.is_some();
            let mut instance = Instance {
                helper: None,
                assets,
                presentation: Default::default(),
                component,
                capabilities,
                owner,
                contributions: Contributions::default(),
                busy,
                initialized: !busy,
                pending: None,
                waiting_for: None,
                files: Arc::new(files),
                opened: 0,
                queue: CallbackQueue::default(),
                epoch,
                strikes: VecDeque::new(),
                stopped: None,
                callback: "init",
                items_sent: None,
                shift: Some(false),
                drawing_context: Default::default(),
                pending_drawing_context: Default::default(),
                drawing_references: Vec::new(),
            };
            if let Err(error) = admission {
                let failure = CallFailure::startup(&instance.owner.bundle, &error);
                failed(
                    &mut instance,
                    &mut budget,
                    &mut scene_revision,
                    &failure,
                    now_ms,
                );
            }
            instances.insert(instance.owner.bundle.clone(), instance);
        }
        let mut live = Self {
            grant,
            media,
            screens: Vec::new(),
            screens_built: None,
            scene_revision,
            instances,
            last_recipient: None,
            executable: executable.to_owned(),
            pending_sends: VecDeque::new(),
            pending_send_bytes: 0,
            budget,
            ingress: Ingress::new(now_ms),
            egress: RateLimit::new(now_ms),
            sequence: 1,
            slice_ms: now_ms,
            ready: false,
            epoch,
            pending_control: None,
            modal_order: 0,
            native_epoch: epoch,
            requested_epoch: None,
            gui: None,
            drawing_fence: None,
            drawing: Default::default(),
            drawing_generation: mod_render::next_owner(),
            drawing_sources: Vec::new(),
            drawing_merge: Default::default(),
            frame_cursor: None,
            session: None,
        };
        live.bind_drawing(context, |_| false)?;
        live.initialize(now_ms);
        Ok(live)
    }

    /// Applies native world changes and coalesced server notices before admitting callbacks.
    pub(super) fn set_epoch(&mut self, native_epoch: u64) -> Result<()> {
        let requested = self.requested_epoch.take();
        if native_epoch == self.native_epoch && requested.is_none() {
            return Ok(());
        }
        let epoch = self
            .epoch
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("world epoch exhausted"))?
            .max(requested.unwrap_or(0));
        self.native_epoch = native_epoch;
        ensure!(
            self.grant.wire.version != WIRE_VERSION,
            "world epoch changed; extension snapshot required"
        );
        self.epoch = epoch;
        if self.ready {
            let session = self.grant.session.clone();
            let acknowledgment = requested.or(match &self.pending_control {
                Some(Control::WorldAck {
                    requested_epoch, ..
                }) => Some(*requested_epoch),
                _ => None,
            });
            self.pending_control = Some(match acknowledgment {
                Some(requested_epoch) => Control::WorldAck {
                    session,
                    world_epoch: epoch,
                    requested_epoch,
                },
                None => Control::Epoch {
                    session,
                    world_epoch: epoch,
                },
            });
        }
        self.gui = None;
        let recipients: Vec<_> = self
            .instances
            .iter()
            .filter(|(_, instance)| instance.helper.is_some() || instance.component.is_some())
            .map(|(bundle, _)| bundle.clone())
            .collect();
        for bundle in recipients {
            let instance = self.instances.get_mut(&bundle).unwrap();
            instance.shift = None;
            self.ingress.stale = self
                .ingress
                .stale
                .saturating_add(instance.queue.filter_epoch(epoch, instance.pending));
            self.enqueue(&bundle, Event::Epoch, epoch);
        }
        Ok(())
    }

    /// Applies aggregate limits and signed schemas before guest dispatch.
    pub(super) fn receive(
        &mut self,
        message: &protocol::ExperienceMessage,
        now_ms: u64,
        epoch: u64,
    ) -> Result<()> {
        ensure!(self.ready, "runtime message before readiness");
        if let Some(requested) = self
            .ingress
            .receive_epoch_request(message, &self.grant, now_ms)?
        {
            self.requested_epoch = Some(self.requested_epoch.unwrap_or(0).max(requested));
            return Ok(());
        }
        self.set_epoch(epoch)?;
        let bytes = message.as_bytes();
        let instances = &self.instances;
        let result = self.ingress.receive(bytes, now_ms, 0, &self.grant, |id| {
            instances.get(id).map(|instance| &instance.capabilities)
        });
        if let Err(error) = result {
            if let Some(overflow) = error.downcast_ref::<wire::QueueOverflow>() {
                if let Some(instance) = self.instances.get_mut(&overflow.bundle) {
                    instance.stop(
                        &mut self.budget,
                        &mut self.scene_revision,
                        STOPPED_AT_OVERFLOW,
                    );
                }
            } else if let Some(failure) = error.downcast_ref::<wire::BundleFailure>()
                && let Some(instance) = self.instances.get_mut(&failure.bundle)
            {
                if instance.stopped.is_none() {
                    instance.unavailable(
                        &mut self.budget,
                        &mut self.scene_revision,
                        &error,
                        now_ms,
                    );
                }
                self.ingress.discard_bundle(&instance.owner.bundle);
            } else {
                return Err(error);
            }
        }
        while let Some(message) = self.ingress.take_committed(u64::MAX) {
            let event = Event::Message {
                channel: message.channel,
                record: serde_json::to_vec(&message.payload)?,
            };
            self.enqueue(&message.bundle, event, message.world_epoch);
        }
        Ok(())
    }

    /// Whether a guest may read the session's items, which [`Live::set_session`] then supplies.
    pub(super) fn reads_items(&self) -> bool {
        self.instances.values().any(|instance| {
            instance
                .capabilities
                .scope
                .permissions
                .contains(&Permission::Items)
        })
    }

    /// The session's items that later dispatches carry to guests granted `items`.
    pub(super) fn set_session(&mut self, session: Arc<SessionData>) {
        self.session = Some(session);
    }

    pub(super) fn media_mut(&mut self) -> &mut super::media::Media {
        &mut self.media
    }

    /// The logical epoch every drawing callback must bind.
    pub(super) fn drawing_epoch(&self) -> u64 {
        self.epoch
    }

    /// Replaces authority and actor identities together before initialization or any dispatch.
    pub(super) fn bind_drawing(
        &mut self,
        context: &mod_host::presentation::Context,
        valid: impl Fn(mod_render::retained::EntityKey) -> bool,
    ) -> Result<()> {
        context.check(context.epoch)?;
        if self.drawing_fence != Some(context.fence()) {
            for instance in self.instances.values_mut() {
                instance.presentation.clear();
                instance.drawing_references.clear();
            }
            self.drawing_sources.clear();
            self.drawing_fence = Some(context.fence());
        }
        for instance in self.instances.values_mut() {
            let mut current = context.clone();
            for key in instance
                .drawing_references
                .iter()
                .chain(
                    instance
                        .pending_drawing_context
                        .candidates
                        .iter()
                        .filter(|_| {
                            instance.busy
                                && instance.pending_drawing_context.fence() == context.fence()
                        }),
                )
                .copied()
                .filter(|key| valid(*key))
            {
                if !current.live.contains(&key) {
                    current.live.push(key);
                }
            }
            current.check(context.epoch)?;
            instance.drawing_context = current;
        }
        Ok(())
    }

    /// Merges only changed client-part output; stopped parts release their renderer resources.
    pub(super) fn drawing(&mut self) -> (&mod_render::RenderOutput, u64) {
        if !self.drawing_sources.iter().copied().eq(self
            .instances
            .values()
            .map(|instance| instance.presentation.output().1))
        {
            self.drawing = self.drawing_merge.merge(
                self.instances
                    .values()
                    .map(|instance| instance.presentation.output().0),
            );
            self.drawing_sources = self
                .instances
                .values()
                .map(|instance| instance.presentation.output().1)
                .collect();
            self.drawing_generation = mod_render::next_owner();
        }
        (&self.drawing, self.drawing_generation)
    }

    /// The first active bundle's local environment contribution in manifest order.
    pub(super) fn environment(&self) -> (Option<u32>, Option<[f32; 2]>) {
        (
            self.instances
                .values()
                .find_map(|instance| instance.presentation.time),
            self.instances
                .values()
                .find_map(|instance| instance.presentation.weather),
        )
    }

    /// Media screens (each bundle's own scene quads whose texture names a playing descriptor),
    /// rebuilt only when the scene or a frame changed; None when unchanged since the last call.
    pub(super) fn changed_screens(&mut self) -> Option<&[render_scene::MediaScreen]> {
        let built = (self.scene_revision, self.media.frames_revision());
        if self.screens_built == Some(built) {
            return None;
        }
        self.screens_built = Some(built);
        self.screens.clear();
        'instances: for (index, instance) in self.instances.values_mut().enumerate() {
            if instance.presentation.scene().is_empty() {
                continue;
            }
            let ledger = instance
                .presentation
                .ledger(instance.capabilities.scope.gpu_bytes)
                .clone();
            for (id, object) in instance.presentation.scene() {
                if self.screens.len() == render_scene::MAX_MEDIA_SCREENS {
                    break 'instances;
                }
                let Some((texture, mut screen)) =
                    super::media::screen((index as u64) << 32 | u64::from(*id), object)
                else {
                    continue;
                };
                if let Some(frame) = self.media.frame(&instance.owner.bundle, texture) {
                    screen.ledger = Some(ledger.clone());
                    screen.frame = frame;
                    self.screens.push(screen);
                }
            }
        }
        Some(&self.screens)
    }

    /// Uses only host-owned status text in the persistent execution indicator; a stopped client
    /// part is named by its bundle id, which the signed manifest bounds.
    pub(super) fn text(&self) -> String {
        let stopped: Vec<String> = self
            .instances
            .iter()
            .filter_map(|(id, instance)| {
                instance
                    .stopped
                    .map(|why| format!("Cinnabar: {id} client part stopped {why}."))
            })
            .collect();
        if stopped.is_empty() {
            return "Cinnabar: server code running (developer helper). F9: disable".into();
        }
        format!("{} F9: disable server code", stopped.join(" "))
    }

    /// Limits remote text separately from the trusted execution indicator.
    pub(super) fn labels(&self) -> String {
        let labels = self
            .instances
            .values()
            .flat_map(|instance| instance.contributions.widgets.values())
            .take(8)
            .cloned()
            .collect::<Vec<_>>()
            .join(" | ");
        if labels.is_empty() {
            return String::new();
        }
        format!(
            "Server widgets: {}",
            labels.chars().take(256).collect::<String>()
        )
    }
}

/// Drops the failed callback's transaction and logs why; a failed start, or the strike that
/// reaches the limit stops the client part.
fn failed<H>(
    instance: &mut Instance<H>,
    budget: &mut Budget,
    scene_revision: &mut u64,
    failure: &CallFailure,
    now_ms: u64,
) {
    bevy::log::warn!(
        bundle = %failure.bundle,
        callback = %failure.callback,
        kind = ?failure.kind,
        fuel = ?failure.fuel,
        reason = %failure.reason,
        "client part callback failed; its output was dropped"
    );
    let why = if failure.kind == FailureKind::RegistryMismatch {
        experience_sdk::REGISTRY_MISMATCH
    } else if failure.kind == FailureKind::Startup {
        STOPPED_AT_START
    } else if failure.kind == FailureKind::Unavailable {
        "because its helper failed"
    } else if instance.strike(now_ms) {
        STOPPED_AFTER_ERRORS
    } else {
        // A restarted guest must refresh retained UI even when Shift is released.
        instance.shift = instance
            .queue
            .entries
            .iter()
            .rev()
            .find_map(|entry| match &entry.event {
                Event::Shift { held } => Some(*held),
                _ => None,
            });
        return;
    };
    bevy::log::warn!(bundle = %instance.owner.bundle, "client part stopped {why}");
    instance.stop(budget, scene_revision, why);
}

/// What the guest of `manifest` may do in this session: the offer's scope narrowed to the
/// permissions the manifest asks for and this build implements, with an equal share of the
/// offer's memory, over its own `assets` and the templates, channels and actions it declares,
/// sending messages no larger than the session's wire carries.
fn capabilities(grant: &Grant, manifest: &Manifest, assets: BTreeSet<String>) -> Capabilities {
    let mut scope = grant.offer.offer.scope.clone();
    scope.permissions = manifest.permissions.clone();
    scope
        .permissions
        .retain(|permission| developer_permissions().contains(permission));
    let count = grant.offer.offer.packages.len() as u64;
    scope.memory_bytes = (scope.memory_bytes / count).min(MAX_GUEST_MEMORY);
    scope.gpu_bytes /= count;
    Capabilities {
        scope,
        assets,
        templates: manifest.templates.clone(),
        channels: manifest.channels.clone(),
        actions: manifest.actions.clone(),
        max_message_bytes: grant.wire.limits.max_message_bytes,
    }
}

#[cfg(test)]
mod tests;
