//! Modal editing shares the launcher's bounded text editor and focus ownership.

use super::*;
use launcher::dressing_room::{SkinEditor, SkinEditorMode, SkinEditorTarget};

impl MenuRuntime {
    pub(in crate::menu) fn skin_editor_blocks(&self, action: MenuAction) -> bool {
        if self.dressing_room.busy && action == MenuAction::EditSkinName {
            return true;
        }
        self.dressing_room.editor.is_some()
            && !matches!(
                action,
                MenuAction::EditSkinName
                    | MenuAction::AddBack
                    | MenuAction::DressingRoom(
                        Action::SaveRename | Action::ConfirmDelete | Action::Cancel
                    )
            )
    }

    pub(super) fn begin_skin_editor(
        &mut self,
        index: usize,
        mode: SkinEditorMode,
        target: SkinEditorTarget,
    ) {
        let name = match target {
            SkinEditorTarget::Skin => self
                .dressing_room
                .skins
                .get(index)
                .filter(|entry| entry.imported)
                .map(|entry| entry.name.clone()),
            SkinEditorTarget::Cape => self
                .dressing_room
                .capes
                .get(index)
                .filter(|entry| entry.imported)
                .map(|entry| entry.name.clone()),
        };
        let Some(name) = name else {
            return;
        };
        self.skin_name.set_text(&name);
        let draft = self.skin_name.as_str().to_owned();
        let view = Arc::make_mut(&mut self.dressing_room);
        view.editor = Some(SkinEditor {
            index,
            mode,
            draft,
            target,
        });
        view.message = None;
        self.hovered = None;
        self.pressed = None;
        self.focused = usize::from(mode == SkinEditorMode::Rename);
        if mode == SkinEditorMode::Rename {
            self.focus_field(MenuField::SkinName);
        } else {
            self.field = None;
        }
    }

    pub(in crate::menu) fn sync_skin_name_draft(&mut self) {
        if let Some(editor) = Arc::make_mut(&mut self.dressing_room).editor.as_mut()
            && editor.mode == SkinEditorMode::Rename
        {
            editor.draft = self.skin_name.as_str().to_owned();
        }
    }

    pub(super) fn save_skin_name(&mut self) {
        let Some(editor) = self
            .dressing_room
            .editor
            .as_ref()
            .filter(|editor| editor.mode == SkinEditorMode::Rename)
        else {
            return;
        };
        let name = self.skin_name.as_str().to_owned();
        let job = match editor.target {
            SkinEditorTarget::Skin => Job::Rename(editor.index, name),
            SkinEditorTarget::Cape => Job::RenameCape(editor.index, name),
        };
        self.queue_skin_job(job);
    }

    pub(super) fn confirm_skin_delete(&mut self) {
        let Some(editor) = self
            .dressing_room
            .editor
            .as_ref()
            .filter(|editor| editor.mode == SkinEditorMode::Delete)
        else {
            return;
        };
        self.queue_skin_job(match editor.target {
            SkinEditorTarget::Skin => Job::Delete(editor.index),
            SkinEditorTarget::Cape => Job::DeleteCape(editor.index),
        });
    }

    pub(super) fn cancel_skin_editor(&mut self) {
        let Some(editor) = Arc::make_mut(&mut self.dressing_room).editor.take() else {
            return;
        };
        self.field = None;
        self.hovered = None;
        self.pressed = None;
        let action = MenuAction::DressingRoom(match (editor.target, editor.mode) {
            (SkinEditorTarget::Skin, SkinEditorMode::Rename) => Action::BeginRename(editor.index),
            (SkinEditorTarget::Skin, SkinEditorMode::Delete) => Action::BeginDelete(editor.index),
            (SkinEditorTarget::Cape, SkinEditorMode::Rename) => {
                Action::BeginRenameCape(editor.index)
            }
            (SkinEditorTarget::Cape, SkinEditorMode::Delete) => {
                Action::BeginDeleteCape(editor.index)
            }
        });
        self.focused = self
            .focus_actions()
            .iter()
            .position(|candidate| *candidate == action)
            .unwrap_or(0);
    }
}
