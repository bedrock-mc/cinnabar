//! One retained FIFO per recipient, with a single replay after registry confirmation.

use super::*;

#[derive(Default)]
pub(super) struct CallbackQueue {
    pub(super) entries: VecDeque<Callback>,
    bytes: usize,
    next: u64,
}

pub(super) struct Callback {
    id: u64,
    epoch: u64,
    pub(super) event: Event,
    bytes: usize,
}

impl CallbackQueue {
    /// Retains a callback, coalescing only an adjacent, unsent update of the same state.
    fn push(&mut self, event: Event, epoch: u64, pending: Option<u64>, limit: usize) -> bool {
        let bytes = retained_bytes(&event);
        let replace = self.entries.back().is_some_and(|last| {
            last.epoch == epoch && pending != Some(last.id) && same_state(&last.event, &event)
        });
        let replaced_bytes = if replace {
            self.entries.back().unwrap().bytes
        } else {
            0
        };
        if self.entries.len() - usize::from(replace) >= MAX_QUEUE_MESSAGES
            || bytes > limit.saturating_sub(self.bytes - replaced_bytes)
        {
            return false;
        }
        if replace {
            self.entries.pop_back();
            self.bytes -= replaced_bytes;
        }
        self.next = self
            .next
            .checked_add(1)
            .expect("callback identity exhausted");
        self.entries.push_back(Callback {
            id: self.next,
            epoch,
            event,
            bytes,
        });
        self.bytes += bytes;
        true
    }

    /// Removes only the acknowledged callback, even if newer state has identical contents.
    pub(super) fn acknowledge(&mut self, id: u64) {
        self.retain(|entry| entry.id != id);
    }

    /// Discards callbacks from previous worlds and counts stale messages for diagnostics.
    pub(super) fn filter_epoch(&mut self, epoch: u64, pending: Option<u64>) -> u64 {
        let mut stale = 0;
        self.retain(|entry| {
            // Its reply decides whether this callback ran or must be counted as stale.
            if pending == Some(entry.id) {
                return true;
            }
            let current = wire::is_current_epoch(entry.epoch, epoch);
            stale += u64::from(!current && matches!(entry.event, Event::Message { .. }));
            current
        });
        stale
    }

    /// Collapses queued state once at confirmation, ahead of the remaining ordered callbacks.
    pub(super) fn replay(&mut self) {
        let mut state = VecDeque::<Callback>::new();
        let mut ordered = VecDeque::new();
        for entry in self.entries.drain(..) {
            if is_state(&entry.event) {
                if let Some(previous) = state
                    .iter_mut()
                    .find(|old| same_state(&old.event, &entry.event))
                {
                    *previous = entry;
                } else {
                    state.push_back(entry);
                }
            } else {
                ordered.push_back(entry);
            }
        }
        // The world notification must precede UI state in the initialization snapshot.
        if let Some(index) = state
            .iter()
            .position(|entry| matches!(entry.event, Event::Epoch))
        {
            let epoch = state.remove(index).unwrap();
            state.push_front(epoch);
        }
        state.append(&mut ordered);
        self.entries = state;
        self.bytes = self.entries.iter().map(|entry| entry.bytes).sum();
    }

    /// Retains matching callbacks and keeps the byte accounting in sync.
    fn retain(&mut self, mut keep: impl FnMut(&Callback) -> bool) {
        self.entries.retain(|entry| {
            if keep(entry) {
                return true;
            }
            self.bytes -= entry.bytes;
            false
        });
    }

    /// Releases all retained callbacks without reusing callback identities.
    pub(super) fn clear(&mut self) {
        self.entries.clear();
        self.bytes = 0;
    }
}

/// Counts the retained event and its owned buffers, without serializing another copy.
fn retained_bytes(event: &Event) -> usize {
    std::mem::size_of::<Callback>()
        + match event {
            Event::Message { channel, record } => channel.len() + record.len(),
            Event::Action { id, .. } | Event::SecondaryAction { id, .. } => id.len(),
            Event::Text { control, text } => control.len() + text.len(),
            Event::Scrolled { view, .. } => view.len(),
            Event::Epoch | Event::Frame | Event::Resized { .. } | Event::Shift { .. } => 0,
        }
}

/// Identifies state updates that initialization may collapse to one latest value.
fn same_state(a: &Event, b: &Event) -> bool {
    match (a, b) {
        (Event::Epoch, Event::Epoch)
        | (Event::Resized { .. }, Event::Resized { .. })
        | (Event::Shift { .. }, Event::Shift { .. }) => true,
        (Event::Text { control: a, .. }, Event::Text { control: b, .. }) => a == b,
        (Event::Scrolled { view: a, .. }, Event::Scrolled { view: b, .. }) => a == b,
        _ => false,
    }
}

/// Distinguishes replayable state from ordered messages and actions.
fn is_state(event: &Event) -> bool {
    same_state(event, event)
}

impl<H: Worker> Live<H> {
    /// Admits every callback through the same recipient bounds, epoch and quarantine checks.
    pub(super) fn enqueue(&mut self, bundle: &str, event: Event, epoch: u64) -> bool {
        let Some(instance) = self.instances.get_mut(bundle) else {
            return false;
        };
        if instance.stopped.is_some() {
            return false;
        }
        if !wire::is_current_epoch(epoch, self.epoch) {
            if matches!(event, Event::Message { .. }) {
                self.ingress.stale = self.ingress.stale.saturating_add(1);
            }
            return false;
        }
        // Input admission can be retried by the caller; accepted host state and messages
        // must stop their recipient if reliable retention is no longer possible.
        let required = matches!(
            event,
            Event::Message { .. } | Event::Epoch | Event::Resized { .. }
        );
        if instance.queue.push(
            event,
            epoch,
            instance.pending,
            self.grant.wire.limits.max_reassembly_bytes as usize,
        ) {
            return true;
        }
        if required {
            instance.stop(
                &mut self.budget,
                &mut self.scene_revision,
                STOPPED_AT_OVERFLOW,
            );
        }
        false
    }

    /// Moves media transitions into recipient queues as soon as the media service produces them.
    pub(in crate::server_experiences) fn service_media(
        &mut self,
        now_unix: u64,
        local_us: u64,
        autoplay: bool,
    ) {
        for (bundle, record) in self.media.service(now_unix, local_us, autoplay) {
            self.enqueue(
                &bundle,
                Event::Message {
                    channel: super::super::media::EVENT_CHANNEL.into(),
                    record,
                },
                self.epoch,
            );
        }
    }

    /// Visits recipients after the last dispatch, skipping busy and negotiating helpers.
    pub(super) fn schedule(&mut self, now_ms: u64) {
        let mut recipients: Vec<_> = self.instances.keys().cloned().collect();
        if let Some(last) = &self.last_recipient {
            let next = recipients.partition_point(|bundle| bundle <= last);
            recipients.rotate_left(next);
        }
        for bundle in recipients {
            let instance = self.instances.get_mut(&bundle).unwrap();
            self.ingress.stale = self
                .ingress
                .stale
                .saturating_add(instance.queue.filter_epoch(self.epoch, instance.pending));
            if instance.busy
                || instance.helper.is_none()
                || !self.budget.can_dispatch(&instance.owner)
            {
                continue;
            }
            let index = if let Some(confirmation) = &instance.waiting_for {
                instance.queue.entries.iter().position(|entry| {
                    matches!(&entry.event, Event::Message { channel, .. } if channel == confirmation)
                        || (matches!(entry.event, Event::Epoch) && instance.epoch != self.epoch)
                })
            } else {
                (!instance.queue.entries.is_empty()).then_some(0)
            };
            let Some(index) = index else {
                continue;
            };
            let entry = &instance.queue.entries[index];
            let (id, event) = (entry.id, entry.event.clone());
            let gui = modal::size_for(&self.gui, &bundle);
            let result = self.budget.dispatch(&instance.owner).and_then(|_| {
                instance.dispatch(self.session.as_deref(), event, self.epoch, gui, Some(id))
            });
            if let Err(error) = result {
                instance.unavailable(&mut self.budget, &mut self.scene_revision, &error, now_ms);
            }
            self.last_recipient = Some(bundle);
        }
    }
}
