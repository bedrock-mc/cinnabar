//! A browser's video preference writer. Browsers start no threads, and a local storage write is
//! quick, so each snapshot saves when it is submitted, keeping the desktop writer's interface.
use super::SavedVideoSettings;
type Completion = (SavedVideoSettings, Result<(), String>);

type Saver = Box<dyn FnMut(SavedVideoSettings) -> Result<(), String> + Send>;

pub struct Writer {
    /// Never contended: only `submit` reaches it, through `&mut self`.
    saver: std::sync::Mutex<Saver>,
    completed: Vec<Completion>,
    requested: Option<SavedVideoSettings>,
}

impl std::fmt::Debug for Writer {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Writer")
            .field("requested", &self.requested)
            .finish_non_exhaustive()
    }
}

impl Writer {
    /// Keeps `saver`, which each submitted snapshot runs.
    pub fn new(
        saver: impl FnMut(SavedVideoSettings) -> Result<(), String> + Send + 'static,
    ) -> std::io::Result<Self> {
        Ok(Self {
            saver: std::sync::Mutex::new(Box::new(saver)),
            completed: Vec::new(),
            requested: None,
        })
    }

    /// Saves `settings` unless they are the snapshot saved last.
    pub fn submit(&mut self, settings: SavedVideoSettings) -> Result<(), String> {
        if self.requested == Some(settings) {
            return Ok(());
        }
        self.requested = Some(settings);
        let saver = self
            .saver
            .get_mut()
            .unwrap_or_else(|poison| poison.into_inner());
        let result = saver(settings);
        self.completed.push((settings, result));
        Ok(())
    }

    /// Takes the saves finished since the last call.
    pub fn poll(&mut self) -> Vec<Completion> {
        std::mem::take(&mut self.completed)
    }
}
