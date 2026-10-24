//! Keeps callbacks in their owning queues until the helper acknowledges that they ran.

use super::*;

pub(super) enum Pending {
    Message(u64),
    Event(Event),
    Media((String, Vec<u8>)),
}

impl<H> Instance<H> {
    /// Removes only the acknowledged event; coalesced updates and later events stay queued.
    pub(super) fn acknowledge(
        &mut self,
        ingress: &mut Ingress,
        media: &mut super::super::media::Media,
        epoch: u64,
    ) {
        match self.pending.take() {
            Some(Pending::Message(sequence)) => {
                ingress.acknowledge(sequence);
            }
            Some(Pending::Event(event)) => {
                if matches!(event, Event::Epoch) && self.epoch != epoch {
                    return;
                }
                if let Some(index) = self.events.iter().position(|pending| pending == &event) {
                    self.events.remove(index);
                }
            }
            Some(Pending::Media(event)) => media.acknowledge_event(&event),
            None => {}
        }
    }
}

impl<H: Worker> Live<H> {
    /// Delivers media only after registry confirmation, retaining events until they run.
    pub(super) fn deliver_media(&mut self, epoch: u64) -> Result<()> {
        while let Some((bundle, record)) = self.media.peek_event().cloned() {
            let Some(instance) = self.instances.get_mut(&bundle) else {
                self.media.next_event();
                continue;
            };
            if instance.stopped.is_some() {
                self.media.next_event();
                continue;
            }
            if instance.busy
                || instance.waiting_for.is_some()
                || !self.budget.can_dispatch(&instance.owner)
            {
                break;
            }
            self.budget.dispatch(&instance.owner)?;
            let event = Event::Message {
                channel: super::super::media::EVENT_CHANNEL.into(),
                record: record.clone(),
            };
            let gui = modal::size_for(&self.gui, &instance.owner.bundle);
            instance.dispatch(
                self.session.as_deref(),
                event,
                epoch,
                gui,
                Pending::Media((bundle, record)),
            )?;
        }
        Ok(())
    }

    /// Lets confirmation pass waiting messages, then drains them through ordinary dispatch.
    pub(super) fn deliver_messages(&mut self, epoch: u64) -> Result<()> {
        loop {
            let instances = &self.instances;
            let budget = &self.budget;
            let Some(message) = self.ingress.peek_matching(u64::MAX, epoch, |message| {
                instances.get(&message.bundle).is_none_or(|instance| {
                    instance.stopped.is_some()
                        || (!instance.busy
                            && instance.helper.is_some()
                            && budget.can_dispatch(&instance.owner)
                            && instance
                                .waiting_for
                                .as_ref()
                                .is_none_or(|confirmation| confirmation == &message.channel))
                })
            }) else {
                break;
            };
            let instance = self
                .instances
                .get_mut(&message.bundle)
                .ok_or_else(|| anyhow::anyhow!("unknown bundle"))?;
            if instance.stopped.is_some() {
                let sequence = message.sequence;
                self.ingress.acknowledge(sequence);
                continue;
            }
            self.budget.dispatch(&instance.owner)?;
            let pending = Pending::Message(message.sequence);
            let event = Event::Message {
                channel: message.channel.clone(),
                record: serde_json::to_vec(&message.payload)?,
            };
            let gui = modal::size_for(&self.gui, &instance.owner.bundle);
            instance.dispatch(self.session.as_deref(), event, epoch, gui, pending)?;
        }
        Ok(())
    }
}
