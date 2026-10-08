//! The modal screen the client parts draw, and the actions its controls deliver.

use super::{Instance, Live, Worker};
use anyhow::Result;
use client_ui::ui_runtime::presentation::ExperienceModal;
use mod_host::helper::{Dispatch, Event};
use server_experience::{
    runtime::{Command, Transaction},
    screen::{self, GuiSize},
};

/// Presses waiting for one bundle's helper; more before it answers are dropped.
const MAX_PENDING_EVENTS: usize = 16;

/// Raises a bundle's modal above the others when its transaction opened a screen.
pub(super) fn note_opened<H>(
    instance: &mut Instance<H>,
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
        if let Some(instance) = self.open_instance() {
            instance.contributions.modal.open(None);
        }
    }

    /// Queues a press of the open modal's control `id` (the control's `$pressed_button_name`)
    /// for its bundle, only when the manifest declares it as an action and `input` is granted.
    pub(in crate::server_experiences) fn press(&mut self, id: &str, index: Option<usize>) -> bool {
        let ready = self.ready;
        let Some(instance) = self.open_instance() else {
            return false;
        };
        if !ready
            || !instance.capabilities.may_deliver(id)
            || instance.events.len() >= MAX_PENDING_EVENTS
        {
            return false;
        }
        instance.events.push_back(Event::Action {
            id: id.to_owned(),
            index: index.and_then(|index| u32::try_from(index).ok()),
        });
        true
    }

    /// Follows the open modal's drawn size: each change queues one `modal-resized` for its
    /// bundle, replacing one still waiting, and later dispatches to it carry the size. A closed
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
        let Some(instance) = self.instances.get_mut(&bundle) else {
            return;
        };
        instance
            .events
            .retain(|event| !matches!(event, Event::Resized { .. }));
        if instance.events.len() < MAX_PENDING_EVENTS {
            instance.events.push_back(Event::Resized { size });
        }
    }

    /// Queues an edit of the open modal's edit box `control` for its bundle, only when the
    /// manifest declares `control` as an action and `input` is granted; a later edit of the same
    /// box replaces one still waiting.
    pub(in crate::server_experiences) fn text_changed(
        &mut self,
        control: &str,
        text: &str,
    ) -> bool {
        let ready = self.ready;
        let Some(instance) = self.open_instance() else {
            return false;
        };
        if !ready || !instance.capabilities.may_deliver(control) || !screen::edit_text(text) {
            return false;
        }
        let waiting = instance.events.iter_mut().find_map(|event| match event {
            Event::Text {
                control: pending,
                text,
            } if pending == control => Some(text),
            _ => None,
        });
        if let Some(waiting) = waiting {
            text.clone_into(waiting);
            return true;
        }
        if instance.events.len() >= MAX_PENDING_EVENTS {
            return false;
        }
        instance.events.push_back(Event::Text {
            control: control.to_owned(),
            text: text.to_owned(),
        });
        true
    }

    /// Queues a secondary press (a right click) of the open modal's control `id`, under the same
    /// rules as [`Live::press`].
    pub(in crate::server_experiences) fn press_secondary(
        &mut self,
        id: &str,
        index: Option<usize>,
    ) -> bool {
        let ready = self.ready;
        let Some(instance) = self.open_instance() else {
            return false;
        };
        if !ready
            || !instance.capabilities.may_deliver(id)
            || instance.events.len() >= MAX_PENDING_EVENTS
        {
            return false;
        }
        instance.events.push_back(Event::SecondaryAction {
            id: id.to_owned(),
            index: index.and_then(|index| u32::try_from(index).ok()),
        });
        true
    }

    fn open_instance(&mut self) -> Option<&mut Instance<H>> {
        let bundle = self
            .modal()
            .filter(|modal| modal.modal.template.is_some())?
            .bundle
            .to_owned();
        self.instances.get_mut(&bundle)
    }

    /// Hands each idle helper its oldest waiting event within the aggregate callback budget.
    pub(super) fn deliver_events(&mut self, epoch: u64) -> Result<()> {
        for instance in self.instances.values_mut() {
            if instance.busy
                || instance.events.is_empty()
                || !self.budget.can_dispatch(&instance.owner)
            {
                continue;
            }
            let Some(helper) = &mut instance.helper else {
                continue;
            };
            let event = instance.events.pop_front().expect("events checked");
            self.budget.dispatch(&instance.owner)?;
            instance.callback = event.callback();
            let gui = size_for(&self.gui, &instance.owner.bundle);
            helper.dispatch(Dispatch { event, epoch, gui })?;
            instance.busy = true;
            instance.epoch = epoch;
        }
        Ok(())
    }
}

/// The modal size a dispatch to `bundle` carries: the drawn open modal's, if it is `bundle`'s.
pub(super) fn size_for(gui: &Option<(GuiSize, String)>, bundle: &str) -> Option<GuiSize> {
    gui.as_ref()
        .filter(|(_, open)| open == bundle)
        .map(|(size, _)| *size)
}
