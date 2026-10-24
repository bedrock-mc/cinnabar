//! Commits completed bundle work without letting a rejected producer revoke its neighbours.

use super::*;
use server_experience::runtime::Transaction;

impl<H: Worker> Live<H> {
    /// Polls every existing helper even while native drawing pauses new callback dispatch.
    /// `valid` checks current native actor liveness, never callback or render snapshots.
    pub(in crate::server_experiences) fn poll(
        &mut self,
        epoch: u64,
        now_ms: u64,
        dispatch: bool,
        valid: impl Fn(mod_render::retained::EntityKey) -> bool,
    ) -> Result<Vec<Vec<u8>>> {
        let mut packets = Vec::new();
        self.discard_stopped();
        self.set_epoch(epoch)?;
        if let Some(control) = &self.pending_control {
            let bytes = serde_json::to_vec(control)?;
            let class = control.rate_class(Direction::ToServer, true);
            match self.egress.charge_class(class, bytes.len(), now_ms) {
                Ok(()) => {
                    self.pending_control = None;
                    packets.push(bytes);
                }
                Err(error) if error.is::<RateExceeded>() => {}
                Err(error) => return Err(error),
            }
        }
        if now_ms.saturating_sub(self.slice_ms) >= CALLBACK_INTERVAL_MS {
            self.slice_ms = now_ms;
            self.budget.begin_slice();
        }
        if dispatch {
            self.initialize(now_ms);
        }
        for instance in self.instances.values_mut() {
            let Some(helper) = &mut instance.helper else {
                continue;
            };
            let result = helper.poll();
            for line in helper.drain_log() {
                bevy::log::warn!(bundle = %instance.owner.bundle, "client part helper: {line}");
            }
            let Some(result) = result else { continue };
            let stale_references = instance.presentation.stale_references;
            instance.busy = false;
            if matches!(&result, Ok(Reply::NotReady { .. })) {
                instance.pending = None;
            } else if let Some(id) = instance.pending.take() {
                instance.queue.acknowledge(id);
            }
            instance.initialized |= instance.callback == "init";
            self.scene_revision += 1;
            let result = match result {
                Ok(Reply::Committed {
                    transaction,
                    fuel,
                    presentation,
                    references,
                }) => {
                    if instance.waiting_for.take().is_some() {
                        instance.queue.replay();
                    }
                    bevy::log::debug!(bundle = %instance.owner.bundle, callback = instance.callback,
                        fuel, "client part callback committed");
                    instance.commit(
                        transaction,
                        presentation,
                        references,
                        self.epoch,
                        self.drawing_fence,
                        &valid,
                        &mut self.modal_order,
                    )
                }
                Ok(Reply::NotReady {
                    transaction,
                    confirmation,
                }) => {
                    instance.waiting_for = Some(confirmation);
                    instance.commit(
                        transaction,
                        None,
                        instance.drawing_references.clone(),
                        self.epoch,
                        self.drawing_fence,
                        &valid,
                        &mut self.modal_order,
                    )
                }
                Ok(Reply::Recovered {
                    failure,
                    transaction,
                    confirmation,
                    references,
                }) => {
                    instance.waiting_for = confirmation;
                    failed(
                        instance,
                        &mut self.budget,
                        &mut self.scene_revision,
                        &failure,
                        now_ms,
                    );
                    if instance.stopped.is_some() {
                        Ok(Vec::new())
                    } else {
                        instance.commit(
                            transaction,
                            None,
                            references,
                            self.epoch,
                            self.drawing_fence,
                            &valid,
                            &mut self.modal_order,
                        )
                    }
                }
                Ok(Reply::Failed {
                    failure,
                    references,
                }) => {
                    let result = if instance.epoch == self.epoch
                        && Some(instance.pending_drawing_context.fence()) == self.drawing_fence
                    {
                        instance.accept_references(references, &valid)
                    } else {
                        Ok(())
                    };
                    if result.is_ok() {
                        failed(
                            instance,
                            &mut self.budget,
                            &mut self.scene_revision,
                            &failure,
                            now_ms,
                        );
                    }
                    result.map(|()| Vec::new())
                }
                Err(error) => Err(error),
            };
            let result = result.and_then(|commands| {
                for command in commands {
                    match command {
                        Command::Media {
                            id,
                            operation,
                            position_ms,
                        } => {
                            self.media
                                .queue(&instance.owner, id, operation, position_ms)?;
                        }
                        Command::Send {
                            channel,
                            schema,
                            record,
                        } => {
                            let send = Envelope {
                                version: self.grant.wire.version,
                                session: self.grant.session.clone(),
                                connection: self.grant.connection.clone(),
                                subclient: self.grant.subclient,
                                bundle: instance.owner.bundle.clone(),
                                generation: instance.owner.generation,
                                channel,
                                schema,
                                sequence: u64::MAX,
                                world_epoch: instance.epoch,
                                payload: record,
                            };
                            let frames = wire::encode(&send, &self.grant.wire)?;
                            let size =
                                RateClass::Data.cost(frames.iter().map(Vec::len))?.bytes as usize;
                            ensure!(
                                self.pending_sends.len() < MAX_QUEUE_MESSAGES
                                    && size <= MAX_QUEUE_BYTES - self.pending_send_bytes,
                                "outbound initialization queue overflow"
                            );
                            self.pending_send_bytes += size;
                            self.pending_sends.push_back((send, size));
                        }
                        _ => {}
                    }
                }
                Ok(())
            });
            if let Err(error) = result {
                instance.unavailable(&mut self.budget, &mut self.scene_revision, &error, now_ms);
            }
            let discarded = instance.presentation.stale_references - stale_references;
            if discarded != 0 {
                bevy::log::debug!(bundle = %instance.owner.bundle, discarded,
                    "discarded stale drawing references");
            }
            instance.discard_outputs(
                &mut self.pending_sends,
                &mut self.pending_send_bytes,
                &mut self.media,
                &mut self.ingress,
            );
        }
        if !self.ready && self.instances.values().all(|instance| instance.initialized) {
            self.ready = true;
            self.pending_control = None;
            // Trusted readiness and epoch controls belong to the whole session.
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
        if self.ready && self.pending_control.is_none() {
            self.send_pending(now_ms, &mut packets)?;
            if dispatch {
                self.dispatch_pending(now_ms);
            }
        }
        self.discard_stopped();
        Ok(packets)
    }

    /// Releases stopped producers before another callback can need their queue capacity.
    pub(super) fn discard_stopped(&mut self) {
        for instance in self.instances.values() {
            instance.discard_outputs(
                &mut self.pending_sends,
                &mut self.pending_send_bytes,
                &mut self.media,
                &mut self.ingress,
            );
        }
    }

    /// Assigns sequence numbers only to admitted output, keeping stopped bundles out of the wire.
    fn send_pending(&mut self, now_ms: u64, packets: &mut Vec<Vec<u8>>) -> Result<()> {
        while let Some((mut send, size)) = self.pending_sends.pop_front() {
            self.pending_send_bytes -= size;
            let instance = self.instances.get_mut(&send.bundle).expect("queued bundle");
            if instance.stopped.is_some() {
                continue;
            }
            let next = self
                .sequence
                .checked_add(1)
                .ok_or_else(|| anyhow::anyhow!("outbound sequence exhausted"))?;
            send.sequence = self.sequence;
            let result = (|| {
                let bytes = wire::encode(&send, &self.grant.wire)?;
                let mut candidate = self.egress.clone();
                candidate.charge_all(bytes.iter().map(Vec::len), now_ms)?;
                self.egress = candidate;
                Ok::<_, anyhow::Error>(bytes)
            })();
            match result {
                Ok(bytes) => {
                    self.sequence = next;
                    packets.extend(bytes);
                }
                Err(error) if error.is::<RateExceeded>() => {
                    self.pending_send_bytes += size;
                    self.pending_sends.push_front((send, size));
                    break;
                }
                Err(error) => {
                    instance.unavailable(
                        &mut self.budget,
                        &mut self.scene_revision,
                        &error,
                        now_ms,
                    );
                    self.scene_revision += 1;
                }
            }
        }
        Ok(())
    }
}

impl<H> Instance<H> {
    /// Records the guest's bounded held references without waiting for scene publication.
    fn accept_references(
        &mut self,
        mut references: Vec<mod_render::retained::EntityKey>,
        valid: impl Fn(mod_render::retained::EntityKey) -> bool,
    ) -> Result<()> {
        ensure!(
            references.len() <= mod_api::MAX_DRAW_ENTITIES
                && references
                    .iter()
                    .all(|key| key.session == self.pending_drawing_context.connection
                        && key.dimension == self.pending_drawing_context.dimension),
            "unadmitted drawing references"
        );
        let count = references.len();
        references.retain(|key| valid(*key));
        self.presentation.stale_references = self
            .presentation
            .stale_references
            .saturating_add((count - references.len()) as u64);
        self.drawing_references = references;
        Ok(())
    }

    /// Revokes this stopped producer's queued side effects without touching another bundle.
    fn discard_outputs(
        &self,
        sends: &mut VecDeque<(Envelope, usize)>,
        bytes: &mut usize,
        media: &mut super::super::media::Media,
        ingress: &mut Ingress,
    ) {
        if self.stopped.is_none() {
            return;
        }
        media.stop(&self.owner.bundle);
        ingress.discard_bundle(&self.owner.bundle);
        sends.retain(|(send, size)| {
            let keep = send.bundle != self.owner.bundle;
            if !keep {
                *bytes -= size;
            }
            keep
        });
    }

    /// Revalidates one producer's transaction before returning its owned side effects.
    #[allow(
        clippy::too_many_arguments,
        reason = "callback authority and output admission"
    )]
    fn commit(
        &mut self,
        transaction: Transaction,
        presentation: Option<Box<mod_host::presentation::Chunk>>,
        references: Vec<mod_render::retained::EntityKey>,
        epoch: u64,
        fence: Option<(u64, u64, i32)>,
        valid: impl Fn(mod_render::retained::EntityKey) -> bool,
        modal_order: &mut u64,
    ) -> Result<Vec<Command>> {
        ensure!(
            !transaction
                .commands
                .iter()
                .any(|command| matches!(command, Command::Scene { .. })),
            "legacy objects require a presentation snapshot"
        );
        self.contributions
            .apply(&transaction, &self.owner, self.epoch, &self.capabilities)?;
        if transaction.epoch == epoch && Some(self.pending_drawing_context.fence()) == fence {
            self.accept_references(references, &valid)?;
            if let Some(presentation) = presentation {
                self.presentation.receive(
                    *presentation,
                    &self.capabilities,
                    &self.drawing_context,
                    self.capabilities.scope.memory_bytes,
                    self.capabilities.scope.gpu_bytes,
                    valid,
                )?;
            }
        }
        modal::note_opened(self, &transaction, modal_order);
        Ok(transaction.commands)
    }
}
