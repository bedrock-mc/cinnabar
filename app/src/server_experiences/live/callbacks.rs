//! Callback admission pauses independently of completion polling and isolates each helper.

use super::*;

impl<H> Instance<H> {
    /// Routes unusable helper output and transport through the callback-failure policy.
    pub(super) fn unavailable(
        &mut self,
        budget: &mut Budget,
        scene_revision: &mut u64,
        error: &anyhow::Error,
        now_ms: u64,
    ) {
        let failure = CallFailure::unavailable(&self.owner.bundle, self.callback, error);
        failed(self, budget, scene_revision, &failure, now_ms);
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
                    failed(
                        instance,
                        &mut self.budget,
                        &mut self.scene_revision,
                        &failure,
                        now_ms,
                    );
                }
            }
        }
    }

    /// Delivers queued callbacks before rotating through idle presentation producers.
    pub(super) fn dispatch_pending(&mut self, now_ms: u64) {
        self.schedule(now_ms);
        let mut ids: Vec<_> = self.instances.keys().cloned().collect();
        let start =
            ids.partition_point(|id| self.frame_cursor.as_ref().is_some_and(|last| id <= last));
        ids.rotate_left(start);
        for id in ids {
            let instance = self.instances.get_mut(&id).expect("current bundle");
            if instance.stopped.is_none()
                && instance.waiting_for.is_none()
                && instance.helper.is_some()
                && !instance.busy
                && instance.capabilities.has_presentation()
                && self.budget.can_dispatch(&instance.owner)
            {
                let result = self.budget.dispatch(&instance.owner).and_then(|_| {
                    instance.dispatch(
                        self.session.as_deref(),
                        Event::Frame,
                        self.epoch,
                        modal::size_for(&self.gui, &instance.owner.bundle),
                        None,
                    )
                });
                if let Err(error) = result {
                    instance.unavailable(
                        &mut self.budget,
                        &mut self.scene_revision,
                        &error,
                        now_ms,
                    );
                }
                self.frame_cursor = Some(id);
            }
        }
    }
}
