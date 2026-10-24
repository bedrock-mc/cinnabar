//! Callback admission pauses independently of completion polling and isolates each helper.

use super::*;

impl<H: Worker> Instance<H> {
    /// Charges and dispatches one callback, stopping only this bundle if its helper refuses.
    pub(super) fn admit(
        &mut self,
        budget: &mut Budget,
        session: Option<&SessionData>,
        event: Event,
        epoch: u64,
        gui: Option<GuiSize>,
        now_ms: u64,
    ) {
        let result = budget
            .dispatch(&self.owner)
            .and_then(|_| self.dispatch(session, event, epoch, gui));
        if let Err(error) = result {
            self.unavailable(budget, &error, now_ms);
        }
    }
}

impl<H> Instance<H> {
    /// Routes unusable helper output and transport through the callback-failure policy.
    pub(super) fn unavailable(&mut self, budget: &mut Budget, error: &anyhow::Error, now_ms: u64) {
        let failure = CallFailure::unavailable(&self.owner.bundle, self.callback, error);
        failed(self, budget, &failure, now_ms);
    }
}

impl<H: Worker> Live<H> {
    /// Starts pending initializers within the shared slice, isolating failed starts.
    pub(super) fn initialize(&mut self, now_ms: u64) {
        for instance in self.instances.values_mut() {
            if instance.component.is_none() || !self.budget.can_dispatch(&instance.owner) {
                continue;
            }
            let result = (|| {
                instance.drawing_context.check(self.epoch)?;
                self.budget.dispatch(&instance.owner)?;
                H::spawn(
                    &self.executable,
                    instance.component.take().expect("component checked"),
                    instance.owner.clone(),
                    instance.capabilities.clone(),
                    instance.drawing_context.clone(),
                    std::mem::take(&mut instance.assets),
                )
            })();
            match result {
                Ok(helper) => {
                    instance.helper = Some(helper);
                    instance.pending_drawing_context = instance.drawing_context.clone();
                    instance.epoch = self.epoch;
                    instance.callback = "init";
                }
                Err(error) => {
                    let failure = CallFailure::startup(&instance.owner.bundle, &error);
                    failed(instance, &mut self.budget, &failure, now_ms);
                }
            }
        }
    }

    /// Delivers waiting input and messages before rotating through frame callbacks.
    pub(super) fn dispatch_pending(&mut self, epoch: u64, now_ms: u64) -> Result<()> {
        self.deliver_events(epoch, now_ms);
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
            instance.admit(
                &mut self.budget,
                self.session.as_deref(),
                Event::Message {
                    channel: super::super::media::EVENT_CHANNEL.into(),
                    record,
                },
                epoch,
                modal::size_for(&self.gui, &instance.owner.bundle),
                now_ms,
            );
        }
        while let Some(message) = self.ingress.peek(u64::MAX, epoch) {
            // Ingress checks the signed route before queuing; a missing owner is session corruption.
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
            match serde_json::to_vec(&message.payload) {
                Ok(record) => instance.admit(
                    &mut self.budget,
                    self.session.as_deref(),
                    Event::Message {
                        channel: message.channel,
                        record,
                    },
                    epoch,
                    modal::size_for(&self.gui, &instance.owner.bundle),
                    now_ms,
                ),
                Err(error) => instance.unavailable(&mut self.budget, &error.into(), now_ms),
            }
        }
        let mut ids: Vec<_> = self.instances.keys().cloned().collect();
        let start =
            ids.partition_point(|id| self.frame_cursor.as_ref().is_some_and(|last| id <= last));
        ids.rotate_left(start);
        for id in ids {
            let instance = self.instances.get_mut(&id).expect("current bundle");
            if instance.stopped.is_none()
                && instance.helper.is_some()
                && !instance.busy
                && instance.capabilities.has_presentation()
                && self.budget.can_dispatch(&instance.owner)
            {
                instance.admit(
                    &mut self.budget,
                    self.session.as_deref(),
                    Event::Frame,
                    epoch,
                    modal::size_for(&self.gui, &instance.owner.bundle),
                    now_ms,
                );
                self.frame_cursor = Some(id);
            }
        }
        Ok(())
    }
}
