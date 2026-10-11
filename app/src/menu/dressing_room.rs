//! Skin selection work stays outside the frame loop; results publish atomically.

use launcher::dressing_room::{Action, DressingRoomView, SkinModel};
use std::sync::Arc;
use {super::*, launcher::install_layout::InstallLayout, launcher::menu::MenuAction};

mod editor;
mod picker;
#[cfg(test)]
mod tests;

#[derive(Debug)]
pub(super) struct Worker {
    commands: crossbeam_channel::Sender<Job>,
    results: crossbeam_channel::Receiver<Outcome>,
    pending: usize,
}

#[derive(Debug)]
enum Job {
    Select(usize),
    Model(launcher::dressing_room::SkinModel),
    Import(Option<PathBuf>),
    Rename(usize, String),
    Delete(usize),
    SelectCape(Option<usize>),
    ImportCape(Option<PathBuf>),
    RenameCape(usize, String),
    DeleteCape(usize),
}

#[derive(Debug)]
struct Outcome {
    view: DressingRoomView,
    changed: bool,
    packet: Option<protocol::Packet>,
    completed_command: bool,
}

impl Outcome {
    fn new(
        view: &DressingRoomView,
        before: Option<render_api::StandardSkin>,
        previous_model: Option<(SkinModel, Arc<str>)>,
        uuid: [u8; 16],
        completed_command: bool,
    ) -> Self {
        let active = view.active_skin();
        let selected = view.selected_skin();
        let changed = before != active
            || previous_model != selected.map(|entry| (entry.model, entry.engine_version.clone()));
        let mut packet = changed
            .then(|| {
                active.as_ref().zip(selected).map(|(skin, entry)| {
                    protocol::player_skin_packet(
                        uuid,
                        skin,
                        entry.model.arm_size(),
                        &entry.id,
                        &entry.name,
                    )
                })
            })
            .flatten();
        if let Some(packet) = &mut packet
            && let Some(selected) = selected
        {
            protocol::set_skin_packet_engine_version(packet, &selected.engine_version);
        }
        Self {
            view: view.clone(),
            changed,
            packet,
            completed_command,
        }
    }
}

impl Worker {
    fn start(
        layout: InstallLayout,
        fallback: crate::player_skin::LocalPlayerSkin,
    ) -> Result<Self, String> {
        Self::start_with_refresh(layout, fallback, |layout| {
            #[cfg(not(test))]
            crate::player_skin::catalog::refresh_default_capes(layout);
            #[cfg(test)]
            let _ = layout;
        })
    }

    fn start_with_refresh(
        layout: InstallLayout,
        fallback: crate::player_skin::LocalPlayerSkin,
        refresh: impl FnOnce(&InstallLayout) + Send + 'static,
    ) -> Result<Self, String> {
        let (commands, incoming) = crossbeam_channel::bounded(4);
        let (outgoing, results) = crossbeam_channel::unbounded();
        std::thread::Builder::new()
            .name("skin-selection".to_owned())
            .spawn(move || {
                let mut view = crate::player_skin::catalog::load(&layout, &fallback);
                if outgoing
                    .send(Outcome::new(
                        &view,
                        Some(fallback.standard_skin()),
                        Some((fallback.model(), fallback.engine_version.clone())),
                        fallback.local_uuid,
                        true,
                    ))
                    .is_err()
                {
                    return;
                }
                let (finished, mut cape_done) = crossbeam_channel::bounded(1);
                let cape_layout = layout.clone();
                if std::thread::Builder::new()
                    .name("cape-textures".to_owned())
                    .spawn(move || {
                        refresh(&cape_layout);
                        let _ = finished.send(());
                    })
                    .is_err()
                {
                    cape_done = crossbeam_channel::never();
                }
                loop {
                    let before = view.active_skin();
                    let previous_model = view
                        .selected_skin()
                        .map(|entry| (entry.model, entry.engine_version.clone()));
                    let completed_command = crossbeam_channel::select! {
                        recv(incoming)->job=>{
                            let Ok(job)=job else {break;};
                            view.message=apply_job(&layout,&mut view,job).err();
                            true
                        },
                        recv(cape_done)->done=>{
                            cape_done=crossbeam_channel::never();
                            if done.is_err() {continue;}
                            crate::player_skin::catalog::merge_default_capes(&layout,&mut view);
                            false
                        }
                    };
                    if outgoing
                        .send(Outcome::new(
                            &view,
                            before,
                            previous_model,
                            fallback.local_uuid,
                            completed_command,
                        ))
                        .is_err()
                    {
                        break;
                    }
                }
            })
            .map_err(|error| error.to_string())?;
        Ok(Self {
            commands,
            results,
            pending: 1,
        })
    }
}

fn apply_job(layout: &InstallLayout, view: &mut DressingRoomView, job: Job) -> Result<(), String> {
    use crate::player_skin::catalog;
    match job {
        Job::Select(index) => catalog::select(layout, view, index),
        Job::Model(model) => catalog::set_model(layout, view, model),
        Job::Import(None) => picker::pick(false)
            .and_then(|path| path.map_or(Ok(()), |path| catalog::import(layout, view, &path))),
        Job::Import(Some(path)) => catalog::import(layout, view, &path),
        Job::Rename(index, name) => catalog::rename(layout, view, index, &name),
        Job::Delete(index) => catalog::delete(layout, view, index),
        Job::SelectCape(index) => catalog::select_cape(layout, view, index),
        Job::ImportCape(None) => picker::pick(true)
            .and_then(|path| path.map_or(Ok(()), |path| catalog::import_cape(layout, view, &path))),
        Job::ImportCape(Some(path)) => catalog::import_cape(layout, view, &path),
        Job::RenameCape(index, name) => catalog::rename_cape(layout, view, index, &name),
        Job::DeleteCape(index) => catalog::delete_cape(layout, view, index),
    }
}

impl MenuRuntime {
    pub(super) fn ensure_dressing_room(&mut self) {
        if self.dressing_room_worker.is_some() {
            return;
        }
        match Worker::start(self.layout.clone(), self.player_skin.clone()) {
            Ok(worker) => {
                self.dressing_room_worker = Some(worker);
                Arc::make_mut(&mut self.dressing_room).busy = true;
            }
            Err(error) => Arc::make_mut(&mut self.dressing_room).message = Some(error),
        }
    }

    fn queue_skin_job(&mut self, job: Job) {
        self.ensure_dressing_room();
        let Some(worker) = self.dressing_room_worker.as_mut() else {
            return;
        };
        match worker.commands.try_send(job) {
            Ok(()) => {
                worker.pending += 1;
                let view = Arc::make_mut(&mut self.dressing_room);
                view.busy = true;
                view.message = None;
            }
            Err(_) => {
                Arc::make_mut(&mut self.dressing_room).message =
                    Some("Please wait for the current skin change to finish.".to_owned())
            }
        }
    }

    pub(super) fn activate_dressing_room(&mut self, action: Action) {
        if self.dressing_room.busy && action != Action::Cancel {
            return;
        }
        match action {
            Action::SetSection(section) => {
                Arc::make_mut(&mut self.dressing_room).section = section;
                self.field = None;
                self.hovered = None;
                self.pressed = None;
                self.focused = 0;
            }
            Action::Select(index) => self.queue_skin_job(Job::Select(index)),
            Action::Import => self.queue_skin_job(Job::Import(None)),
            Action::ImportCape => self.queue_skin_job(Job::ImportCape(None)),
            Action::SelectCape(index) => self.queue_skin_job(Job::SelectCape(index)),
            Action::SetModel(model) => self.queue_skin_job(Job::Model(model)),
            Action::BeginRename(index) => self.begin_skin_editor(
                index,
                launcher::dressing_room::SkinEditorMode::Rename,
                launcher::dressing_room::SkinEditorTarget::Skin,
            ),
            Action::BeginDelete(index) => self.begin_skin_editor(
                index,
                launcher::dressing_room::SkinEditorMode::Delete,
                launcher::dressing_room::SkinEditorTarget::Skin,
            ),
            Action::BeginRenameCape(index) => self.begin_skin_editor(
                index,
                launcher::dressing_room::SkinEditorMode::Rename,
                launcher::dressing_room::SkinEditorTarget::Cape,
            ),
            Action::BeginDeleteCape(index) => self.begin_skin_editor(
                index,
                launcher::dressing_room::SkinEditorMode::Delete,
                launcher::dressing_room::SkinEditorTarget::Cape,
            ),
            Action::SaveRename => self.save_skin_name(),
            Action::ConfirmDelete => self.confirm_skin_delete(),
            Action::Cancel => self.cancel_skin_editor(),
        }
    }

    /// Uses the same skin importer as the Dressing Room file picker.
    #[cfg(any(test, feature = "developer-control"))]
    pub(crate) fn import_skin_path(&mut self, path: PathBuf) {
        if self.dressing_room.editor.is_some() {
            return;
        }
        self.queue_skin_job(Job::Import(Some(path)));
    }

    #[cfg(any(test, feature = "developer-control"))]
    pub(crate) fn import_cape_path(&mut self, path: PathBuf) {
        if self.dressing_room.editor.is_none() {
            self.queue_skin_job(Job::ImportCape(Some(path)));
        }
    }

    pub(super) fn poll_dressing_room(
        &mut self,
        local: Option<&mut crate::player_skin::LocalPlayerSkin>,
        world: &mut ClientWorld,
        network: Option<&crate::runtime::network::NetworkHandle>,
        session: u64,
    ) {
        if let Some(worker) = self.dressing_room_worker.as_mut() {
            while let Ok(mut result) = worker.results.try_recv() {
                if result.completed_command {
                    worker.pending = worker.pending.saturating_sub(1);
                }
                result.view.busy = worker.pending != 0;
                result.view.section = self.dressing_room.section;
                if !result.completed_command {
                    result.view.editor = self.dressing_room.editor.clone();
                    result.view.message = self.dressing_room.message.clone();
                } else if result.view.message.is_some() {
                    result.view.editor = self.dressing_room.editor.clone();
                } else if self.dressing_room.editor.is_some() {
                    self.field = None;
                    self.focused = 0;
                }
                if result.changed
                    && let Some(selected) = result.view.selected_skin()
                {
                    if let Some(active) = result.view.active_skin() {
                        self.player_skin.set_selection(&active, selected.model);
                        self.player_skin.engine_version = selected.engine_version.clone();
                    }
                    self.skin_update_pending = true;
                    self.skin_outbound = None;
                    self.skin_packet_pending = result.packet;
                }
                self.dressing_room = Arc::new(result.view);
            }
        }
        if !self.skin_update_pending {
            self.flush_skin_packet(network);
            return;
        }
        if let Some(local) = local {
            *local = self.player_skin.clone();
        }
        let Some(stream) = world.stream.as_mut() else {
            self.skin_update_pending = self.is_connecting();
            if !self.skin_update_pending {
                self.skin_packet_pending = None;
            }
            return;
        };
        let authority = stream.authority();
        let uuid = authority
            .actor(authority.local_player_runtime_id())
            .and_then(|actor| match &actor.kind {
                protocol::ActorKind::Player { uuid, .. } => Some(*uuid),
                _ => None,
            })
            .unwrap_or(self.player_skin.local_uuid);
        if let Some(mut profile) = stream
            .authority()
            .actor_player_profile(stream.local_player_runtime_id())
            .cloned()
        {
            profile.skin = self.player_skin.player_skin();
            stream.update_local_player_skin(&profile);
        }
        if session == 0 {
            return;
        }
        let Some(mut packet) = self.skin_packet_pending.take() else {
            return;
        };
        protocol::set_skin_packet_uuid(&mut packet, uuid);
        self.skin_update_pending = false;
        self.skin_outbound = Some((session, packet));
        self.flush_skin_packet(network);
    }

    fn flush_skin_packet(&mut self, network: Option<&crate::runtime::network::NetworkHandle>) {
        let Some(network) = network else { return };
        let Some((session, packet)) = self.skin_outbound.take() else {
            return;
        };
        match network.send_settings_packet(session, packet) {
            Ok(()) | Err(crate::runtime::network::PacketSendError::Closed(_)) => {}
            Err(crate::runtime::network::PacketSendError::Full(packet)) => {
                self.skin_outbound = Some((session, packet))
            }
        }
    }

    pub(super) fn dressing_room_focus(&self) -> Vec<MenuAction> {
        if let Some(editor) = &self.dressing_room.editor {
            if self.dressing_room.busy {
                return vec![MenuAction::DressingRoom(Action::Cancel)];
            }
            return match editor.mode {
                launcher::dressing_room::SkinEditorMode::Rename => {
                    let mut actions = vec![
                        MenuAction::DressingRoom(Action::Cancel),
                        MenuAction::EditSkinName,
                    ];
                    if !editor.draft.trim().is_empty() {
                        actions.push(MenuAction::DressingRoom(Action::SaveRename));
                    }
                    actions
                }
                launcher::dressing_room::SkinEditorMode::Delete => vec![
                    MenuAction::DressingRoom(Action::Cancel),
                    MenuAction::DressingRoom(Action::ConfirmDelete),
                ],
            };
        }
        let mut actions = vec![MenuAction::AddBack];
        if self.dressing_room.busy {
            return actions;
        }
        actions.extend(
            [
                launcher::dressing_room::DressingRoomSection::Skins,
                launcher::dressing_room::DressingRoomSection::Capes,
            ]
            .into_iter()
            .filter(|section| *section != self.dressing_room.section)
            .map(|section| MenuAction::DressingRoom(Action::SetSection(section))),
        );
        if self.dressing_room.section == launcher::dressing_room::DressingRoomSection::Capes {
            actions.extend([
                MenuAction::DressingRoom(Action::ImportCape),
                MenuAction::DressingRoom(Action::SelectCape(None)),
            ]);
            actions.extend(
                (0..self.dressing_room.capes.len())
                    .map(|index| MenuAction::DressingRoom(Action::SelectCape(Some(index)))),
            );
            if let Some(index) = self.dressing_room.selected_cape.filter(|index| {
                self.dressing_room
                    .capes
                    .get(*index)
                    .is_some_and(|entry| entry.imported)
            }) {
                actions.extend([
                    MenuAction::DressingRoom(Action::BeginRenameCape(index)),
                    MenuAction::DressingRoom(Action::BeginDeleteCape(index)),
                ]);
            }
            return actions;
        }
        actions.push(MenuAction::DressingRoom(Action::Import));
        actions.extend(
            (0..self.dressing_room.skins.len())
                .map(|index| MenuAction::DressingRoom(Action::Select(index))),
        );
        if self
            .dressing_room
            .selected_skin()
            .is_some_and(|skin| skin.imported)
        {
            actions.extend(
                [
                    launcher::dressing_room::SkinModel::Classic,
                    launcher::dressing_room::SkinModel::Slim,
                ]
                .into_iter()
                .filter(|model| {
                    self.dressing_room.selected_skin().is_some_and(|selected| {
                        selected.model != SkinModel::Custom && selected.model != *model
                    })
                })
                .map(|model| MenuAction::DressingRoom(Action::SetModel(model))),
            );
            if let Some(index) = self.dressing_room.selected {
                actions.extend([
                    MenuAction::DressingRoom(Action::BeginRename(index)),
                    MenuAction::DressingRoom(Action::BeginDelete(index)),
                ]);
            }
        }
        actions
    }
}
