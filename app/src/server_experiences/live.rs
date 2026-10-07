//! Developer helper supervision; only implemented adapters receive grants.

use super::worker::Worker;
use anyhow::{Result, ensure};
use mod_host::helper::{Dispatch, Helper};
use server_experience::{
    bundle::VerifiedBundle,
    manifest::developer_permissions,
    negotiation::Grant,
    policy::*,
    runtime::{Budget, CALLBACK_INTERVAL_MS, Capabilities, Command, Contributions, Principal},
    session::Control,
    wire::{Envelope, Ingress, RateLimit},
};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    path::{Path, PathBuf},
};

struct Instance<H> {
    helper: Option<H>,
    component: Option<Vec<u8>>,
    capabilities: Capabilities,
    owner: Principal,
    contributions: Contributions,
    busy: bool,
}

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
            let mut scope = grant.offer.offer.scope.clone();
            scope.permissions = bundle.manifest.permissions.clone();
            scope
                .permissions
                .retain(|permission| developer_permissions().contains(permission));
            let count = grant.offer.offer.packages.len() as u64;
            scope.memory_bytes = (scope.memory_bytes / count).min(MAX_GUEST_MEMORY);
            scope.gpu_bytes /= count;
            let capabilities = Capabilities {
                scope,
                assets: bundle.paths().map(str::to_owned).collect(),
                channels: bundle.manifest.channels.clone(),
                actions: bundle.manifest.actions.clone(),
            };
            budget.reserve(
                owner.clone(),
                capabilities.scope.memory_bytes,
                capabilities.scope.gpu_bytes,
            )?;
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
        };
        live.initialize()?;
        Ok(live)
    }

    /// Publishes complete transactions only; failure revokes every contribution in this preview.
    pub(super) fn poll(&mut self, epoch: u64, now_ms: u64) -> Result<Vec<Vec<u8>>> {
        ensure!(
            epoch == self.epoch,
            "world epoch changed; extension snapshot required"
        );
        if now_ms.saturating_sub(self.slice_ms) >= CALLBACK_INTERVAL_MS {
            self.slice_ms = now_ms;
            self.budget.begin_slice();
        }
        self.initialize()?;

        for instance in self.instances.values_mut() {
            let Some(helper) = &mut instance.helper else {
                continue;
            };
            let Some(result) = helper.poll() else {
                continue;
            };
            instance.busy = false;
            let transaction = match result {
                Ok(transaction) => transaction,
                Err(error) => {
                    self.budget.quarantine(&instance.owner);
                    instance.contributions = Contributions::default();
                    self.scene_revision += 1;
                    return Err(error);
                }
            };
            self.scene_revision += 1;
            instance.contributions.apply(
                &transaction,
                &instance.owner,
                epoch,
                &instance.capabilities,
            )?;
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
                        version: WIRE_VERSION,
                        session: self.grant.session.clone(),
                        connection: self.grant.connection.clone(),
                        subclient: self.grant.subclient,
                        bundle: instance.owner.bundle.clone(),
                        generation: instance.owner.generation,
                        channel,
                        schema,
                        sequence: self.sequence,
                        world_epoch: epoch,
                        payload: record,
                    };
                    let bytes = serde_json::to_vec(&send)?;
                    ensure!(
                        self.pending_sends.len() < MAX_QUEUE_MESSAGES
                            && bytes.len() <= MAX_QUEUE_BYTES - self.pending_send_bytes,
                        "outbound initialization queue overflow"
                    );
                    self.pending_send_bytes += bytes.len();
                    self.pending_sends.push_back(bytes);
                    self.sequence = self
                        .sequence
                        .checked_add(1)
                        .ok_or_else(|| anyhow::anyhow!("outbound sequence exhausted"))?;
                }
            }
        }
        let mut packets = Vec::new();
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
            while let Some((bundle, record)) = self.media.next_event() {
                let Some(instance) = self.instances.get_mut(&bundle) else {
                    continue;
                };
                if instance.busy || !self.budget.can_dispatch(&instance.owner) {
                    self.media.defer_event((bundle, record));
                    break;
                }
                self.budget.dispatch(&instance.owner)?;
                if let Some(helper) = &mut instance.helper {
                    helper.dispatch(Dispatch {
                        channel: super::media::EVENT_CHANNEL.into(),
                        record,
                        actions: BTreeSet::new(),
                        epoch,
                    })?;
                    instance.busy = true;
                }
            }
            while let Some(message) = self.ingress.peek(u64::MAX, epoch) {
                let instance = self
                    .instances
                    .get_mut(&message.bundle)
                    .ok_or_else(|| anyhow::anyhow!("unknown bundle"))?;
                if instance.busy || !self.budget.can_dispatch(&instance.owner) {
                    break;
                }
                let message = self.ingress.pop(u64::MAX, epoch).expect("front checked");
                self.budget.dispatch(&instance.owner)?;
                if let Some(helper) = &mut instance.helper {
                    helper.dispatch(Dispatch {
                        channel: message.channel,
                        record: serde_json::to_vec(&message.payload)?,
                        actions: BTreeSet::new(),
                        epoch,
                    })?;
                    instance.busy = true;
                }
            }
        }
        Ok(packets)
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

    /// Uses only host-owned status text in the persistent execution indicator.
    pub(super) fn text(&self) -> String {
        "Cinnabar: server code running (developer helper). F9: disable".into()
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

#[cfg(test)]
mod tests;
