//! The modal screen the client parts draw, and the actions its controls deliver.

use super::{Live, Worker};
use mod_host::helper::Event;
use mod_ui::experience_modal::ExperienceModal;
use server_experience::{
    runtime::{Command, Transaction},
    screen::{self, GuiSize},
};

/// Raises a bundle's modal above the others when its transaction opened a screen.
pub(super) fn note_opened<H>(
    instance: &mut super::Instance<H>,
    transaction: &Transaction,
    order: &mut u64,
) {
    if transaction
        .commands
        .iter()
        .any(|command| matches!(command, Command::Screen { template: Some(_) }))
    {
        *order += 1;
        instance.opened = *order;
    }
}

impl<H: Worker> Live<H> {
    /// The modal to draw: the most recently opened screen still open, else the last opened
    /// bundle's closed one, so its catalog and textures stay ready for reopening.
    pub(in crate::server_experiences) fn modal(&self) -> Option<ExperienceModal<'_>> {
        let (bundle, instance) = self
            .instances
            .iter()
            .filter(|(_, instance)| instance.opened > 0)
            .max_by_key(|(_, instance)| {
                (
                    instance.contributions.modal.template.is_some(),
                    instance.opened,
                )
            })?;
        Some(ExperienceModal {
            bundle,
            files: &instance.files,
            modal: &instance.contributions.modal,
        })
    }

    /// Escape closes the open modal on the host side; its bound data stays for the next open.
    pub(in crate::server_experiences) fn close_modal(&mut self) {
        self.shift_changed(false);
        if let Some(bundle) = self.open_bundle() {
            self.instances
                .get_mut(&bundle)
                .unwrap()
                .contributions
                .modal
                .open(None);
        }
    }

    /// Queues a declared action for the focused recipient in callback arrival order.
    pub(in crate::server_experiences) fn press(&mut self, id: &str, index: Option<usize>) -> bool {
        self.press_button(id, index, false)
    }

    /// Follows the open modal's drawn size: each change queues one `modal-resized` for its
    /// bundle, coalescing adjacent unsent sizes. Later dispatches carry the size. A closed
    /// or undrawn modal (`None`) has no size and reports nothing.
    pub(in crate::server_experiences) fn set_modal_size(&mut self, size: Option<GuiSize>) {
        let bundle = self
            .modal()
            .filter(|modal| modal.modal.template.is_some())
            .map(|modal| modal.bundle.to_owned());
        let current = size.zip(bundle);
        if current == self.gui {
            return;
        }
        self.gui.clone_from(&current);
        let Some((size, bundle)) = current else {
            return;
        };
        self.enqueue(&bundle, Event::Resized { size }, self.epoch);
    }

    /// Queues an edit of the open modal's edit box `control` for its bundle, only when the
    /// manifest declares `control` as an action and `input` is granted. Adjacent unsent edits
    /// of the same box coalesce without crossing other callbacks.
    pub(in crate::server_experiences) fn text_changed(
        &mut self,
        control: &str,
        text: &str,
    ) -> bool {
        let Some(bundle) = self.open_bundle() else {
            return false;
        };
        if !self.ready
            || !self.instances[&bundle].capabilities.may_deliver(control)
            || !screen::edit_text(text)
        {
            return false;
        }
        self.enqueue(
            &bundle,
            Event::Text {
                control: control.to_owned(),
                text: text.to_owned(),
            },
            self.epoch,
        )
    }

    /// Updates the focused bundle and releases bundles that lost focus, subject to `input`.
    /// Adjacent unsent transitions coalesce; a full queue retains its state for retry.
    pub(in crate::server_experiences) fn shift_changed(&mut self, held: bool) -> bool {
        if !self.ready {
            return false;
        }
        let focused = self
            .modal()
            .filter(|modal| modal.modal.template.is_some())
            .map(|modal| modal.bundle.to_owned());
        let mut queued = false;
        let bundles: Vec<_> = self.instances.keys().cloned().collect();
        for bundle in bundles {
            let held = held && focused.as_deref() == Some(bundle.as_str());
            let instance = &self.instances[&bundle];
            if instance.capabilities.may_read_input()
                && instance.shift != Some(held)
                && self.enqueue(&bundle, Event::Shift { held }, self.epoch)
            {
                self.instances.get_mut(&bundle).unwrap().shift = Some(held);
                queued = true;
            }
        }
        queued
    }

    /// Queues the open modal's scroll view `view` showing `range` for its bundle, only when the
    /// manifest declares `view` as an action and `input` is granted. Adjacent unsent ranges
    /// for the same view coalesce without crossing actions or other callbacks.
    pub(in crate::server_experiences) fn scroll_changed(
        &mut self,
        view: &str,
        range: screen::ScrollRange,
    ) -> bool {
        let Some(bundle) = self.open_bundle() else {
            return false;
        };
        if !self.ready || !self.instances[&bundle].capabilities.may_deliver(view) || !range.valid()
        {
            return false;
        }
        self.enqueue(
            &bundle,
            Event::Scrolled {
                view: view.to_owned(),
                range,
            },
            self.epoch,
        )
    }

    /// Queues a secondary press (a right click) of the open modal's control `id`, under the same
    /// rules as [`Live::press`].
    pub(in crate::server_experiences) fn press_secondary(
        &mut self,
        id: &str,
        index: Option<usize>,
    ) -> bool {
        self.press_button(id, index, true)
    }

    /// Admits primary and secondary buttons using the focused bundle's input grant.
    fn press_button(&mut self, id: &str, index: Option<usize>, secondary: bool) -> bool {
        let Some(bundle) = self.open_bundle() else {
            return false;
        };
        if !self.ready || !self.instances[&bundle].capabilities.may_deliver(id) {
            return false;
        }
        let id = id.to_owned();
        let index = index.and_then(|index| u32::try_from(index).ok());
        let event = if secondary {
            Event::SecondaryAction { id, index }
        } else {
            Event::Action { id, index }
        };
        self.enqueue(&bundle, event, self.epoch)
    }

    /// Identifies the recipient whose modal currently owns input focus.
    fn open_bundle(&self) -> Option<String> {
        Some(
            self.modal()
                .filter(|modal| modal.modal.template.is_some())?
                .bundle
                .to_owned(),
        )
    }
}

/// The modal size a dispatch to `bundle` carries: the drawn open modal's, if it is `bundle`'s.
pub(super) fn size_for(gui: &Option<(GuiSize, String)>, bundle: &str) -> Option<GuiSize> {
    gui.as_ref()
        .filter(|(_, open)| open == bundle)
        .map(|(size, _)| *size)
}
