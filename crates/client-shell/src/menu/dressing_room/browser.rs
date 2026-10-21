//! Browser-local execution yields between commands and individual imported skins.

use super::*;
use crate::player_skin::catalog::memory as catalog;
use launcher::skin_import::picked::PickedFile;
use wasm_bindgen::prelude::*;

#[wasm_bindgen(module = "/../../web/site/dressing.mjs")]
extern "C" {
    #[wasm_bindgen(js_name = nextDressingFrame)]
    fn next_frame() -> js_sys::Promise;
}

/// Ends one browser task slice without blocking the animation frame that requested it.
async fn yield_frame() {
    let _ = wasm_bindgen_futures::JsFuture::from(next_frame()).await;
}

/// A single reserved import; dropping it completes the operation as a cancellation.
pub struct DressingRoomImport {
    sender: crossbeam_channel::Sender<Result<Vec<PickedFile>, String>>,
    cape: bool,
}

impl DressingRoomImport {
    /// Returns the canonical count before the page enumerates any selected file metadata.
    pub fn file_limit(&self) -> usize {
        launcher::skin_import::picked::MAX_PICKED_FILES
    }

    /// Admits metadata synchronously before the page reads or copies any binary bytes.
    pub fn admit_files(&self, metadata: &str) -> Result<(), JsError> {
        use launcher::skin_import::picked;
        if metadata.len() > picked::MAX_PICKED_METADATA_BYTES {
            return Err(JsError::new("The selected file metadata is too large."));
        }
        let files: Vec<(String, u64)> = serde_json::from_str(metadata)
            .map_err(|_| JsError::new("The selected file metadata is invalid."))?;
        picked::admit(
            &files
                .iter()
                .map(|(name, size)| (name.as_str(), *size))
                .collect::<Vec<_>>(),
            self.cape,
        )
        .map_err(|error| JsError::new(&error.to_string()))
    }

    /// Sends validated picker bytes, an empty cancelled selection, or a read failure once.
    pub fn complete(self, files: Result<Vec<PickedFile>, String>) {
        let _ = self.sender.try_send(files);
    }

    /// Rechecks every JS byte view before copying any selected file into Rust memory.
    pub fn complete_files(self, files: js_sys::Array, error: Option<String>) {
        let cape = self.cape;
        let read = || -> Result<Vec<PickedFile>, String> {
            if let Some(error) = error {
                return Err(error);
            }
            if files.length() == 0 {
                return Ok(Vec::new());
            }
            if files.length() as usize > launcher::skin_import::picked::MAX_PICKED_FILES {
                return Err("Too many selected skin files.".into());
            }
            let mut selected = Vec::new();
            for value in files.iter() {
                let pair = value
                    .dyn_into::<js_sys::Array>()
                    .map_err(|_| "Invalid selected file.")?;
                let name = pair
                    .get(0)
                    .as_string()
                    .ok_or("Invalid selected filename.")?;
                let bytes = pair
                    .get(1)
                    .dyn_into::<js_sys::Uint8Array>()
                    .map_err(|_| "Invalid selected bytes.")?;
                selected.push((name, bytes));
            }
            launcher::skin_import::picked::admit(
                &selected
                    .iter()
                    .map(|(name, bytes)| (name.as_str(), u64::from(bytes.length())))
                    .collect::<Vec<_>>(),
                cape,
            )
            .map_err(|error| error.to_string())?;
            Ok(selected
                .into_iter()
                .map(|(name, bytes)| PickedFile {
                    name,
                    bytes: bytes.to_vec(),
                })
                .collect())
        };
        self.complete(read());
    }
}

impl MenuRuntime {
    /// Reserves an import from the gesture's current pointer hit or focused keyboard action.
    /// The host opens its file picker before returning from that gesture and completes this ticket.
    pub fn begin_dressing_room_import(
        &mut self,
        presentation: &client_ui::ui_runtime::presentation::UiPresentationRuntime,
        position: Option<ui::UiPoint>,
    ) -> Option<DressingRoomImport> {
        if self.screen() != launcher::menu::MenuScreen::DressingRoom
            || self.dressing_room.busy
            || self.dressing_room.editor.is_some()
        {
            return None;
        }
        let action = match position {
            Some(point) => presentation.hit_test_menu(point)?,
            None => *self.focus_actions().get(self.focused)?,
        };
        let cape = match (action, self.dressing_room.section) {
            (
                MenuAction::DressingRoom(Action::Import),
                launcher::dressing_room::DressingRoomSection::Skins,
            ) => false,
            (
                MenuAction::DressingRoom(Action::ImportCape),
                launcher::dressing_room::DressingRoomSection::Capes,
            ) => true,
            _ => return None,
        };
        self.ensure_dressing_room();
        let worker = self.dressing_room_worker.as_mut()?;
        let (sender, receiver) = crossbeam_channel::bounded(1);
        worker.commands.try_send(Job::Pick(cape, receiver)).ok()?;
        worker.pending += 1;
        let view = Arc::make_mut(&mut self.dressing_room);
        view.busy = true;
        view.message = None;
        Some(DressingRoomImport { sender, cape })
    }
}

impl Worker {
    /// Starts a local async catalog owner; Bevy retains only bounded channel endpoints.
    fn start(
        _layout: InstallLayout,
        fallback: crate::player_skin::LocalPlayerSkin,
    ) -> Result<Self, String> {
        let (commands, incoming) = crossbeam_channel::bounded(MAX_SKIN_COMMANDS);
        let (outgoing, results) = crossbeam_channel::bounded(1);
        let alive = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let running = alive.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let mut view = catalog::load(&fallback);
            let mut result = Outcome::new(
                &view,
                Some(fallback.standard_skin()),
                Some((
                    fallback.model(),
                    fallback.engine_version.clone(),
                    fallback.skin_id.clone(),
                )),
                fallback.local_uuid,
                true,
            );
            loop {
                yield_frame().await;
                if !running.load(std::sync::atomic::Ordering::Relaxed) {
                    break;
                }
                result = match outgoing.try_send(result) {
                    Ok(()) => break,
                    Err(crossbeam_channel::TrySendError::Full(result)) => result,
                    Err(crossbeam_channel::TrySendError::Disconnected(_)) => return,
                };
            }
            loop {
                yield_frame().await;
                if !running.load(std::sync::atomic::Ordering::Relaxed) {
                    return;
                }
                let job = match incoming.try_recv() {
                    Ok(job) => job,
                    Err(crossbeam_channel::TryRecvError::Empty) => continue,
                    Err(crossbeam_channel::TryRecvError::Disconnected) => return,
                };
                let before = view.active_skin();
                let previous = view.selected_skin().map(selection_key);
                view.message = match job {
                    Job::Pick(cape, picked) => {
                        import(&mut view, cape, &picked, &running).await.err()
                    }
                    job => memory::apply(&mut view, job).err(),
                };
                let mut result = Outcome::new(&view, before, previous, fallback.local_uuid, true);
                loop {
                    if !running.load(std::sync::atomic::Ordering::Relaxed) {
                        return;
                    }
                    match outgoing.try_send(result) {
                        Ok(()) => break,
                        Err(crossbeam_channel::TrySendError::Full(returned)) => result = returned,
                        Err(crossbeam_channel::TrySendError::Disconnected(_)) => return,
                    }
                    yield_frame().await;
                }
            }
        });
        Ok(Self {
            commands,
            results,
            pending: 1,
            alive,
        })
    }
}

/// Waits without blocking for a picker, then decodes one skin between animation frames.
async fn import(
    view: &mut DressingRoomView,
    cape: bool,
    picked: &crossbeam_channel::Receiver<Result<Vec<PickedFile>, String>>,
    running: &std::sync::atomic::AtomicBool,
) -> Result<(), String> {
    let files = loop {
        if !running.load(std::sync::atomic::Ordering::Relaxed) {
            return Ok(());
        }
        match picked.try_recv() {
            Ok(files) => break files?,
            Err(crossbeam_channel::TryRecvError::Disconnected) => return Ok(()),
            Err(crossbeam_channel::TryRecvError::Empty) => yield_frame().await,
        }
    };
    if files.is_empty() {
        return Ok(());
    }
    yield_frame().await;
    if cape {
        return catalog::import_cape(view, files);
    }
    let mut import = catalog::Import::begin(view, files)?;
    loop {
        yield_frame().await;
        if !running.load(std::sync::atomic::Ordering::Relaxed) {
            return Ok(());
        }
        if import.step()? {
            break;
        }
    }
    *view = import.finish();
    Ok(())
}

impl Drop for Worker {
    /// Cancels frame work when the owning browser runtime is dropped.
    fn drop(&mut self) {
        self.alive
            .store(false, std::sync::atomic::Ordering::Relaxed);
    }
}
