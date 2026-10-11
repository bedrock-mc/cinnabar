//! An ordered worker coalesces pending applied video preferences without blocking frames.
use super::SavedVideoSettings;
use std::sync::{Arc, Mutex, mpsc};
type Completion = (SavedVideoSettings, Result<(), String>);

#[derive(Debug)]
pub struct Writer {
    latest: Arc<Mutex<Option<SavedVideoSettings>>>,
    wake: Option<mpsc::SyncSender<()>>,
    completed: Mutex<mpsc::Receiver<Completion>>,
    requested: Option<SavedVideoSettings>,
    worker: Option<std::thread::JoinHandle<()>>,
}
impl Writer {
    /// Starts a worker owning the durable save operation.
    pub fn new(
        mut saver: impl FnMut(SavedVideoSettings) -> Result<(), String> + Send + 'static,
    ) -> std::io::Result<Self> {
        let latest = Arc::new(Mutex::new(None));
        let pending = Arc::clone(&latest);
        let (wake, jobs) = mpsc::sync_channel(1);
        let (done, completed) = mpsc::channel();
        let worker = std::thread::Builder::new()
            .name("video-settings".into())
            .spawn(move || {
                while jobs.recv().is_ok() {
                    loop {
                        let settings = pending
                            .lock()
                            .unwrap_or_else(|poison| poison.into_inner())
                            .take();
                        let Some(settings) = settings else {
                            break;
                        };
                        let result = saver(settings);
                        if done.send((settings, result)).is_err() {
                            return;
                        }
                    }
                }
            })?;
        Ok(Self {
            latest,
            wake: Some(wake),
            completed: Mutex::new(completed),
            requested: None,
            worker: Some(worker),
        })
    }
    /// Replaces pending work with the newest applied snapshot and wakes the worker.
    pub fn submit(&mut self, settings: SavedVideoSettings) -> Result<(), String> {
        if self.requested == Some(settings) {
            return Ok(());
        }
        *self
            .latest
            .lock()
            .unwrap_or_else(|poison| poison.into_inner()) = Some(settings);
        match self
            .wake
            .as_ref()
            .ok_or("video settings writer stopped")?
            .try_send(())
        {
            Ok(()) | Err(mpsc::TrySendError::Full(())) => {
                self.requested = Some(settings);
                Ok(())
            }
            Err(mpsc::TrySendError::Disconnected(())) => {
                Err("video settings writer stopped".into())
            }
        }
    }
    /// Takes completed writes without waiting for storage.
    pub fn poll(&mut self) -> Vec<Completion> {
        self.completed
            .get_mut()
            .unwrap_or_else(|poison| poison.into_inner())
            .try_iter()
            .collect()
    }
}
impl Drop for Writer {
    /// Drains accepted snapshots before releasing the persistence owner.
    fn drop(&mut self) {
        self.wake.take();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn video_writer_coalesces_pending_snapshots_and_reports_failures() {
        use std::sync::mpsc;
        use std::time::Duration;
        let (seen_tx, seen_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let mut first = true;
        let mut writer = Writer::new(move |settings| {
            seen_tx.send(settings).unwrap();
            if first {
                first = false;
                release_rx.recv().unwrap();
                Ok(())
            } else {
                Err("read only".into())
            }
        })
        .unwrap();
        let snapshot = |gui_scale_offset| SavedVideoSettings {
            fullscreen: false,
            gui_scale_offset,
        };
        writer.submit(snapshot(0)).unwrap();
        assert_eq!(
            seen_rx.recv_timeout(Duration::from_secs(2)).unwrap(),
            snapshot(0)
        );
        writer.submit(snapshot(1)).unwrap();
        writer.submit(snapshot(2)).unwrap();
        release_tx.send(()).unwrap();
        assert_eq!(
            seen_rx.recv_timeout(Duration::from_secs(2)).unwrap(),
            snapshot(2)
        );
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        let mut completed = Vec::new();
        while completed.len() < 2 {
            completed.extend(writer.poll());
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
        assert_eq!(
            completed,
            vec![
                (snapshot(0), Ok(())),
                (snapshot(2), Err("read only".into()))
            ]
        );
    }
    #[test]
    fn review_slow_video_save_does_not_block_submission() {
        use std::sync::mpsc;
        use std::time::Duration;
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let (returned_tx, returned_rx) = mpsc::channel();
        let thread = std::thread::spawn(move || {
            let mut writer = Writer::new(move |_| {
                started_tx.send(()).unwrap();
                release_rx.recv().unwrap();
                Ok(())
            })
            .unwrap();
            writer.submit(SavedVideoSettings::default()).unwrap();
            returned_tx.send(()).unwrap();
        });
        started_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        // Submission must finish while storage is still held, regardless of scheduling delay.
        let responsive = returned_rx.recv_timeout(Duration::from_secs(2)).is_ok();
        release_tx.send(()).unwrap();
        thread.join().unwrap();
        assert!(responsive, "a flush blocked the frame's submission");
    }
}
