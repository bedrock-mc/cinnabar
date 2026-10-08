//! Developer helper supervision; only implemented adapters receive grants.

use super::worker::Worker;
use anyhow::{Result, ensure};
use mod_host::helper::{CallFailure, Dispatch, Event, FailureKind, Helper, Reply};
use server_experience::{
    bundle::VerifiedBundle,
    manifest::{Manifest, developer_permissions},
    negotiation::Grant,
    policy::*,
    runtime::{Budget, CALLBACK_INTERVAL_MS, Capabilities, Command, Contributions, Principal},
    screen::{self, GuiSize},
    session::Control,
    wire::{self, Envelope, Ingress, RateLimit},
};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    path::{Path, PathBuf},
    sync::Arc,
};

mod modal;

struct Instance<H> {
    helper: Option<H>,
    component: Option<Vec<u8>>,
    capabilities: Capabilities,
    owner: Principal,
    contributions: Contributions,
    busy: bool,
    /// The bundle's verified templates and textures, shared with the modal presenter.
    files: Arc<screen::Files>,
    /// When its modal last opened, so the most recent one draws on top.
    opened: u64,
    /// Host callbacks (modal actions) waiting for the helper, oldest first.
    events: VecDeque<Event>,
    /// The world epoch of its pending callback, which that callback's transaction carries.
    epoch: u64,
    /// When its recent callbacks failed, in milliseconds, oldest first.
    strikes: VecDeque<u64>,
    /// Why the client part was stopped, which the trusted status says; it runs no more.
    stopped: Option<&'static str>,
    /// The guest export of its pending callback, for the log.
    callback: &'static str,
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
    /// and so does its budget.
    fn stop(&mut self, budget: &mut Budget, why: &'static str) {
        self.helper = None;
        self.component = None;
        self.busy = false;
        self.events.clear();
        self.contributions = Contributions::default();
        budget.quarantine(&self.owner);
        self.stopped = Some(why);
    }
}

/// What the trusted status says of a client part stopped after `MAX_GUEST_STRIKES` failures.
const STOPPED_AFTER_ERRORS: &str = "after repeated errors";
/// What the trusted status says of a client part that failed to start.
const STOPPED_AT_START: &str = "because it failed to start";

pub(super) struct Live<H = Helper> {
    grant: Grant,
    media: super::media::Media,
    screens: Vec<render::MediaScreen>,
    /// Scene and frame revisions `screens` was built from.
    screens_built: Option<(u64, u64)>,
    scene_revision: u64,
    instances: BTreeMap<String, Instance<H>>,
    executable: PathBuf,
    pending_sends: VecDeque<Vec<u8>>,
    pending_send_bytes: usize,
    budget: Budget,
    ingress: Ingress,
    egress: RateLimit,
    sequence: u64,
    slice_ms: u64,
    ready: bool,
    epoch: u64,
    /// Counts modal openings across bundles.
    modal_order: u64,
    /// The drawn open modal's size and its bundle, which dispatches to that bundle carry.
    gui: Option<(GuiSize, String)>,
}

impl<H: Worker> Live<H> {
    /// Launches only developer helpers; unsupported required presentation remains denied.
    pub(super) fn start(
        grant: Grant,
        bundles: Vec<VerifiedBundle>,
        epoch: u64,
        now_ms: u64,
        executable: &Path,
        media_helper: &Path,
    ) -> Result<Self> {
        ensure!(
            bundles
                .iter()
                .map(|bundle| bundle.manifest.channels.len())
                .sum::<usize>()
                <= MAX_CHANNELS,
            "aggregate channel limit exceeded"
        );
        let mut budget = Budget::default();
        budget.begin_slice();
        let mut instances = BTreeMap::new();
        let mut media = super::media::Media::new(grant.clone(), epoch, media_helper.to_owned());
        for mut bundle in bundles {
            let owner = Principal {
                session: grant.session.clone(),
                bundle: bundle.manifest.id.clone(),
                generation: INITIAL_BUNDLE_GENERATION,
            };
            let assets = bundle.paths().map(str::to_owned).collect();
            let capabilities = capabilities(&grant, &bundle.manifest, assets);
            budget.reserve(
                owner.clone(),
                capabilities.scope.memory_bytes,
                capabilities.scope.gpu_bytes,
            )?;
            let files = bundle.take_screen_files();
            let component = bundle.take_component();
            media.register(bundle);
            let busy = component.is_some();
            instances.insert(
                owner.bundle.clone(),
                Instance {
                    helper: None,
                    component,
                    capabilities,
                    owner,
                    contributions: Contributions::default(),
                    busy,
                    files: Arc::new(files),
                    opened: 0,
                    events: VecDeque::new(),
                    epoch,
                    strikes: VecDeque::new(),
                    stopped: None,
                    callback: "init",
                },
            );
        }
        let mut live = Self {
            grant,
            media,
            screens: Vec::new(),
            screens_built: None,
            scene_revision: 0,
            instances,
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
            modal_order: 0,
            gui: None,
        };
        live.initialize()?;
        Ok(live)
    }

    /// Publishes complete transactions only; failure revokes every contribution in this preview.
    pub(super) fn poll(&mut self, epoch: u64, now_ms: u64) -> Result<Vec<Vec<u8>>> {
        let mut packets = Vec::new();
        if epoch != self.epoch {
            self.change_epoch(epoch, now_ms, &mut packets)?;
        }
        if now_ms.saturating_sub(self.slice_ms) >= CALLBACK_INTERVAL_MS {
            self.slice_ms = now_ms;
            self.budget.begin_slice();
        }
        self.initialize()?;

        for instance in self.instances.values_mut() {
            let Some(helper) = &mut instance.helper else {
                continue;
            };
            let result = helper.poll();
            for line in helper.drain_log() {
                bevy::log::warn!(bundle = %instance.owner.bundle, "client part helper: {line}");
            }
            let Some(result) = result else {
                continue;
            };
            instance.busy = false;
            let transaction = match result {
                Ok(Reply::Committed { transaction, fuel }) => {
                    bevy::log::debug!(
                        bundle = %instance.owner.bundle,
                        callback = instance.callback,
                        fuel,
                        "client part callback committed"
                    );
                    transaction
                }
                Ok(Reply::Failed(failure)) => {
                    failed(instance, &mut self.budget, &failure, now_ms);
                    self.scene_revision += 1;
                    continue;
                }
                Err(error) => {
                    self.budget.quarantine(&instance.owner);
                    instance.contributions = Contributions::default();
                    self.scene_revision += 1;
                    return Err(error);
                }
            };
            // A callback that began before an epoch change still publishes; its sends carry the
            // epoch it began in, which the server drops and counts.
            self.scene_revision += 1;
            instance.contributions.apply(
                &transaction,
                &instance.owner,
                instance.epoch,
                &instance.capabilities,
            )?;
            modal::note_opened(instance, &transaction, &mut self.modal_order);
            for command in transaction.commands {
                if let Command::Media {
                    id,
                    operation,
                    position_ms,
                } = command
                {
                    self.media
                        .queue(&instance.owner, id, operation, position_ms)?;
                } else if let Command::Send {
                    channel,
                    schema,
                    record,
                } = command
                {
                    let send = Envelope {
                        version: self.grant.wire.version,
                        session: self.grant.session.clone(),
                        connection: self.grant.connection.clone(),
                        subclient: self.grant.subclient,
                        bundle: instance.owner.bundle.clone(),
                        generation: instance.owner.generation,
                        channel,
                        schema,
                        sequence: self.sequence,
                        world_epoch: instance.epoch,
                        payload: record,
                    };
                    for bytes in wire::encode(&send, &self.grant.wire)? {
                        ensure!(
                            self.pending_sends.len() < MAX_QUEUE_MESSAGES
                                && bytes.len() <= MAX_QUEUE_BYTES - self.pending_send_bytes,
                            "outbound initialization queue overflow"
                        );
                        self.pending_send_bytes += bytes.len();
                        self.pending_sends.push_back(bytes);
                    }
                    self.sequence = self
                        .sequence
                        .checked_add(1)
                        .ok_or_else(|| anyhow::anyhow!("outbound sequence exhausted"))?;
                }
            }
        }
        if !self.ready && self.instances.values().all(|instance| !instance.busy) {
            self.ready = true;
            packets.push(serde_json::to_vec(&Control::Ready {
                session: self.grant.session.clone(),
                packages: self
                    .grant
                    .offer
                    .offer
                    .packages
                    .iter()
                    .map(|p| p.digest.clone())
                    .collect(),
                generation: INITIAL_BUNDLE_GENERATION,
                permissions: self
                    .instances
                    .iter()
                    .map(|(id, instance)| {
                        (id.clone(), instance.capabilities.scope.permissions.clone())
                    })
                    .collect(),
                world_epoch: self.epoch,
            })?);
        }
        if self.ready {
            while let Some(bytes) = self.pending_sends.pop_front() {
                self.pending_send_bytes -= bytes.len();
                self.egress.charge(bytes.len(), now_ms)?;
                packets.push(bytes);
            }
            self.deliver_events(epoch)?;
            while let Some((bundle, record)) = self.media.next_event() {
                let Some(instance) = self.instances.get_mut(&bundle) else {
                    continue;
                };
                if instance.stopped.is_some() {
                    continue;
                }
                if instance.busy || !self.budget.can_dispatch(&instance.owner) {
                    self.media.defer_event((bundle, record));
                    break;
                }
                self.budget.dispatch(&instance.owner)?;
                if let Some(helper) = &mut instance.helper {
                    let event = Event::Message {
                        channel: super::media::EVENT_CHANNEL.into(),
                        record,
                    };
                    instance.callback = event.callback();
                    let gui = modal::size_for(&self.gui, &instance.owner.bundle);
                    helper.dispatch(Dispatch { event, epoch, gui })?;
                    instance.busy = true;
                    instance.epoch = epoch;
                }
            }
            while let Some(message) = self.ingress.peek(u64::MAX, epoch) {
                let instance = self
                    .instances
                    .get_mut(&message.bundle)
                    .ok_or_else(|| anyhow::anyhow!("unknown bundle"))?;
                if instance.stopped.is_some() {
                    self.ingress.pop(u64::MAX, epoch);
                    continue;
                }
                if instance.busy || !self.budget.can_dispatch(&instance.owner) {
                    break;
                }
                let message = self.ingress.pop(u64::MAX, epoch).expect("front checked");
                self.budget.dispatch(&instance.owner)?;
                if let Some(helper) = &mut instance.helper {
                    let event = Event::Message {
                        channel: message.channel,
                        record: serde_json::to_vec(&message.payload)?,
                    };
                    instance.callback = event.callback();
                    let gui = modal::size_for(&self.gui, &instance.owner.bundle);
                    helper.dispatch(Dispatch { event, epoch, gui })?;
                    instance.busy = true;
                    instance.epoch = epoch;
                }
            }
        }
        Ok(packets)
    }

    /// Keeps a wire v2 runtime through a world epoch change: once Ready has named the old epoch,
    /// an `epoch` control names the new one ahead of every later send, and each guest is called
    /// back to resend its state. A v1 session cannot continue.
    fn change_epoch(&mut self, epoch: u64, now_ms: u64, packets: &mut Vec<Vec<u8>>) -> Result<()> {
        ensure!(
            self.grant.wire.version != WIRE_VERSION,
            "world epoch changed; extension snapshot required"
        );
        self.epoch = epoch;
        if self.ready {
            let control = serde_json::to_vec(&Control::Epoch {
                session: self.grant.session.clone(),
                world_epoch: epoch,
            })?;
            self.egress.charge(control.len(), now_ms)?;
            packets.push(control);
        }
        for instance in self.instances.values_mut() {
            let guest = instance.helper.is_some() || instance.component.is_some();
            if guest
                && !instance
                    .events
                    .iter()
                    .any(|event| matches!(event, Event::Epoch))
            {
                instance.events.push_back(Event::Epoch);
            }
        }
        Ok(())
    }

    /// Starts pending initializers only when the aggregate callback slice has room.
    fn initialize(&mut self) -> Result<()> {
        for instance in self.instances.values_mut() {
            if instance.component.is_none() || !self.budget.can_dispatch(&instance.owner) {
                continue;
            }
            self.budget.dispatch(&instance.owner)?;
            let component = instance.component.take().expect("component checked");
            instance.helper = Some(H::spawn(
                &self.executable,
                component,
                instance.owner.clone(),
                instance.capabilities.clone(),
                self.epoch,
            )?);
            instance.epoch = self.epoch;
            instance.callback = "init";
        }
        Ok(())
    }

    /// Applies aggregate limits and signed schemas before guest dispatch.
    pub(super) fn receive(&mut self, bytes: &[u8], now_ms: u64) -> Result<()> {
        ensure!(self.ready, "runtime message before readiness");
        let instances = &self.instances;
        self.ingress.receive(bytes, now_ms, 0, &self.grant, |id| {
            instances.get(id).map(|instance| &instance.capabilities)
        })
    }

    pub(super) fn media_mut(&mut self) -> &mut super::media::Media {
        &mut self.media
    }

    /// Media screens (each bundle's own scene quads whose texture names a playing descriptor),
    /// rebuilt only when the scene or a frame changed; None when unchanged since the last call.
    pub(super) fn changed_screens(&mut self) -> Option<&[render::MediaScreen]> {
        let built = (self.scene_revision, self.media.frames_revision());
        if self.screens_built == Some(built) {
            return None;
        }
        self.screens_built = Some(built);
        self.screens.clear();
        'instances: for (index, instance) in self.instances.values().enumerate() {
            for (id, object) in &instance.contributions.scene {
                if self.screens.len() == render::MAX_MEDIA_SCREENS {
                    break 'instances;
                }
                let Some((texture, mut screen)) =
                    super::media::screen((index as u64) << 32 | u64::from(*id), object)
                else {
                    continue;
                };
                if let Some(frame) = self.media.frame(&instance.owner.bundle, texture) {
                    screen.frame = frame;
                    self.screens.push(screen);
                }
            }
        }
        Some(&self.screens)
    }

    /// Texture bytes the signed scope lets media screens allocate.
    pub(super) fn gpu_budget_bytes(&self) -> u64 {
        self.grant.offer.offer.scope.gpu_bytes.min(MAX_GPU_BYTES)
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
/// reaches the limit, stops the client part.
fn failed<H>(instance: &mut Instance<H>, budget: &mut Budget, failure: &CallFailure, now_ms: u64) {
    bevy::log::warn!(
        bundle = %failure.bundle,
        callback = %failure.callback,
        kind = ?failure.kind,
        fuel = ?failure.fuel,
        reason = %failure.reason,
        "client part callback failed; its output was dropped"
    );
    let why = if failure.kind == FailureKind::Startup {
        STOPPED_AT_START
    } else if instance.strike(now_ms) {
        STOPPED_AFTER_ERRORS
    } else {
        return;
    };
    bevy::log::warn!(bundle = %instance.owner.bundle, "client part stopped {why}");
    instance.stop(budget, why);
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
