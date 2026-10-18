//! Arboard-backed bounded clipboard adapter for chat paste and copy.

use std::sync::{Arc, Mutex, PoisonError, TryLockError};

#[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
use arboard::{Clipboard as SystemClipboard, Error as SystemClipboardError};
use ui::ChatClipboard;
#[cfg(any(target_os = "android", target_arch = "wasm32"))]
use unavailable::{Clipboard as SystemClipboard, Error as SystemClipboardError};

#[cfg(any(target_os = "android", target_arch = "wasm32"))]
mod unavailable;

/// Handle to the process-wide platform clipboard.
///
/// Every handle shares one long-lived arboard clipboard. On X11 the copied
/// selection is served by that clipboard object, so it must outlive each copy
/// for other applications to paste the text when no clipboard manager runs.
/// The application releases it during shutdown.
#[derive(Default)]
pub struct PlatformClipboard;

#[derive(Debug, thiserror::Error)]
pub enum PlatformClipboardError {
    #[error("platform clipboard failed: {0}")]
    Platform(#[from] SystemClipboardError),
    #[error("clipboard text exceeds the {maximum}-byte chat insertion bound")]
    TooLong { maximum: usize },
    #[error("platform clipboard is shutting down")]
    ShuttingDown,
}

static PLATFORM_CLIPBOARD: ClipboardOwner<SystemClipboard> = ClipboardOwner::new();

impl ChatClipboard for PlatformClipboard {
    type Error = PlatformClipboardError;

    fn read_text_bounded(&mut self, maximum_bytes: usize) -> Result<Option<Arc<str>>, Self::Error> {
        PLATFORM_CLIPBOARD.read_text_bounded(maximum_bytes)
    }
}

impl PlatformClipboard {
    /// Places `text` on the platform clipboard and keeps serving it after
    /// this call returns.
    pub fn write_text(&mut self, text: String) -> Result<(), PlatformClipboardError> {
        PLATFORM_CLIPBOARD.write_text(text)
    }

    /// Releases the retained connection on graceful exit so its destructor can
    /// hand copied text to the clipboard manager, and prevents further access.
    pub fn shutdown() {
        PLATFORM_CLIPBOARD.shutdown();
    }

    /// Abandons the connection on forced exit unless clipboard I/O holds its lock.
    /// Skips destruction and handoff so process termination cannot wait on them.
    pub fn shutdown_if_idle() {
        PLATFORM_CLIPBOARD.shutdown_if_idle();
    }
}

/// Text access to one platform clipboard connection.
trait ClipboardBackend: Sized {
    /// Opens a connection to the platform clipboard.
    fn open() -> Result<Self, SystemClipboardError>;
    /// Returns the clipboard text. Platforms materialize the whole selection,
    /// so callers enforce their size bound on the returned text.
    fn get_text(&mut self) -> Result<String, SystemClipboardError>;
    /// Takes ownership of the clipboard selection with `text`.
    fn set_text(&mut self, text: String) -> Result<(), SystemClipboardError>;
}

impl ClipboardBackend for SystemClipboard {
    fn open() -> Result<Self, SystemClipboardError> {
        SystemClipboard::new()
    }

    fn get_text(&mut self) -> Result<String, SystemClipboardError> {
        SystemClipboard::get_text(self)
    }

    fn set_text(&mut self, text: String) -> Result<(), SystemClipboardError> {
        SystemClipboard::set_text(self, text)
    }
}

/// Lazily opened clipboard connection that stays alive for the owner's
/// lifetime, so selection ownership survives past each write.
struct ClipboardOwner<B> {
    state: Mutex<ClipboardState<B>>,
}

/// Connection and shutdown authority, updated under the same lock.
struct ClipboardState<B> {
    backend: Option<B>,
    shutting_down: bool,
}

impl<B: ClipboardBackend> ClipboardOwner<B> {
    /// An owner with no connection opened yet.
    const fn new() -> Self {
        Self {
            state: Mutex::new(ClipboardState {
                backend: None,
                shutting_down: false,
            }),
        }
    }

    /// Drops the connection once and prevents another operation from reopening it.
    fn shutdown(&self) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.shutting_down = true;
        state.backend = None;
    }

    /// Forgets an idle connection without waiting for its lock or destructor.
    fn shutdown_if_idle(&self) {
        let mut state = match self.state.try_lock() {
            Ok(state) => state,
            Err(TryLockError::Poisoned(error)) => error.into_inner(),
            Err(TryLockError::WouldBlock) => return,
        };
        state.shutting_down = true;
        // The process is exiting; the OS reclaims the connection without clipboard I/O.
        std::mem::forget(state.backend.take());
    }

    /// Runs `action` on the retained connection, opening it on first use.
    ///
    /// A failure other than an empty clipboard drops the connection, so the
    /// next call reconnects instead of reusing a broken one, and is logged.
    fn with_backend<R>(
        &self,
        action: impl FnOnce(&mut B) -> Result<R, SystemClipboardError>,
    ) -> Result<R, PlatformClipboardError> {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if state.shutting_down {
            return Err(PlatformClipboardError::ShuttingDown);
        }
        let result = match &mut state.backend {
            Some(backend) => action(backend),
            empty => B::open().and_then(|backend| action(empty.insert(backend))),
        };
        if let Err(error) = &result
            && !matches!(error, SystemClipboardError::ContentNotAvailable)
        {
            state.backend = None;
            tracing::warn!(%error, "platform clipboard failed");
        }
        Ok(result?)
    }

    /// Returns the clipboard text, or `TooLong` when it exceeds `maximum_bytes`.
    fn read_text_bounded(
        &self,
        maximum_bytes: usize,
    ) -> Result<Option<Arc<str>>, PlatformClipboardError> {
        let text = match self.with_backend(B::get_text) {
            Err(PlatformClipboardError::Platform(SystemClipboardError::ContentNotAvailable)) => {
                return Ok(None);
            }
            result => result?,
        };
        if text.len() > maximum_bytes {
            return Err(PlatformClipboardError::TooLong {
                maximum: maximum_bytes,
            });
        }
        Ok(Some(Arc::from(text)))
    }

    /// Places `text` on the clipboard through the retained connection.
    fn write_text(&self, text: String) -> Result<(), PlatformClipboardError> {
        self.with_backend(|backend| backend.set_text(text))
    }
}

// The fakes report arboard's errors, which Android builds lack.
#[cfg(all(test, not(any(target_os = "android", target_arch = "wasm32"))))]
mod tests {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    use super::*;

    /// Fake platform connection that, like X11, serves the selection only
    /// while the connection that set it is alive.
    struct SelectionServingBackend;

    static SERVED_SELECTION: Mutex<Option<String>> = Mutex::new(None);
    static OPENED_CONNECTIONS: AtomicUsize = AtomicUsize::new(0);
    static DROPPED_CONNECTIONS: AtomicUsize = AtomicUsize::new(0);
    static FAIL_NEXT_READ: AtomicBool = AtomicBool::new(false);
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    /// The text the fake platform currently serves to other applications.
    fn served_selection() -> Option<String> {
        SERVED_SELECTION
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Serializes tests over the shared fake platform and clears its state.
    fn reset_platform() -> std::sync::MutexGuard<'static, ()> {
        let guard = TEST_LOCK.lock().unwrap_or_else(PoisonError::into_inner);
        *SERVED_SELECTION
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = None;
        OPENED_CONNECTIONS.store(0, Ordering::SeqCst);
        DROPPED_CONNECTIONS.store(0, Ordering::SeqCst);
        FAIL_NEXT_READ.store(false, Ordering::SeqCst);
        guard
    }

    impl ClipboardBackend for SelectionServingBackend {
        fn open() -> Result<Self, arboard::Error> {
            OPENED_CONNECTIONS.fetch_add(1, Ordering::SeqCst);
            Ok(Self)
        }

        fn get_text(&mut self) -> Result<String, arboard::Error> {
            if FAIL_NEXT_READ.swap(false, Ordering::SeqCst) {
                return Err(arboard::Error::Unknown {
                    description: "compositor connection lost".into(),
                });
            }
            served_selection().ok_or(arboard::Error::ContentNotAvailable)
        }

        fn set_text(&mut self, text: String) -> Result<(), arboard::Error> {
            *SERVED_SELECTION
                .lock()
                .unwrap_or_else(PoisonError::into_inner) = Some(text);
            Ok(())
        }
    }

    impl Drop for SelectionServingBackend {
        fn drop(&mut self) {
            DROPPED_CONNECTIONS.fetch_add(1, Ordering::SeqCst);
            *SERVED_SELECTION
                .lock()
                .unwrap_or_else(PoisonError::into_inner) = None;
        }
    }

    #[test]
    fn application_exit_releases_the_retained_connection_once() {
        let _platform = reset_platform();
        let owner = Arc::new(ClipboardOwner::<SelectionServingBackend>::new());
        let children = launcher_host::lifecycle::children::Children::new();
        let exit_owner = Arc::clone(&owner);
        let _exit = children.on_exit(move || exit_owner.shutdown_if_idle());
        owner
            .write_text("copied text".to_owned())
            .expect("write succeeds");
        assert_eq!(DROPPED_CONNECTIONS.load(Ordering::SeqCst), 0);

        owner.shutdown();
        children.stop_all(std::time::Duration::ZERO);
        children.stop_all(std::time::Duration::ZERO);
        owner.shutdown();

        assert_eq!(served_selection(), None);
        assert_eq!(DROPPED_CONNECTIONS.load(Ordering::SeqCst), 1);
        assert!(matches!(
            owner.write_text("late copy".to_owned()),
            Err(PlatformClipboardError::ShuttingDown)
        ));
        assert!(matches!(
            owner.read_text_bounded(64),
            Err(PlatformClipboardError::ShuttingDown)
        ));
        assert_eq!(OPENED_CONNECTIONS.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn shutdown_without_clipboard_access_opens_no_connection() {
        let _platform = reset_platform();
        let owner = ClipboardOwner::<SelectionServingBackend>::new();

        owner.shutdown();

        assert_eq!(OPENED_CONNECTIONS.load(Ordering::SeqCst), 0);
        assert_eq!(DROPPED_CONNECTIONS.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn forced_exit_abandons_the_backend_without_running_its_destructor() {
        let _platform = reset_platform();
        let owner = Arc::new(ClipboardOwner::<SelectionServingBackend>::new());
        let children = launcher_host::lifecycle::children::Children::new();
        let exit_owner = Arc::clone(&owner);
        let _exit = children.on_exit(move || exit_owner.shutdown_if_idle());
        owner
            .write_text("copied text".to_owned())
            .expect("write succeeds");

        children.stop_all(std::time::Duration::ZERO);
        children.stop_all(std::time::Duration::ZERO);
        owner.shutdown();

        assert_eq!(DROPPED_CONNECTIONS.load(Ordering::SeqCst), 0);
        assert!(matches!(
            owner.read_text_bounded(64),
            Err(PlatformClipboardError::ShuttingDown)
        ));
        assert_eq!(OPENED_CONNECTIONS.load(Ordering::SeqCst), 1);
        drop(owner);

        assert_eq!(DROPPED_CONNECTIONS.load(Ordering::SeqCst), 0);
        assert_eq!(served_selection().as_deref(), Some("copied text"));
    }

    #[test]
    fn forced_exit_skips_a_busy_clipboard_and_runs_the_remaining_hooks() {
        let _platform = reset_platform();
        let owner = Arc::new(ClipboardOwner::<SelectionServingBackend>::new());
        let children = launcher_host::lifecycle::children::Children::new();
        let exit_owner = Arc::clone(&owner);
        let _exit = children.on_exit(move || exit_owner.shutdown_if_idle());
        let remaining_hook_ran = Arc::new(AtomicBool::new(false));
        let ran = Arc::clone(&remaining_hook_ran);
        let _remaining = children.on_exit(move || ran.store(true, Ordering::SeqCst));
        owner
            .write_text("copied text".to_owned())
            .expect("write succeeds");

        let state = owner.state.lock().unwrap_or_else(PoisonError::into_inner);
        children.stop_all(std::time::Duration::ZERO);

        assert!(remaining_hook_ran.load(Ordering::SeqCst));
        assert_eq!(DROPPED_CONNECTIONS.load(Ordering::SeqCst), 0);
        assert_eq!(served_selection().as_deref(), Some("copied text"));
        drop(state);

        owner.shutdown();

        assert_eq!(DROPPED_CONNECTIONS.load(Ordering::SeqCst), 1);
        assert_eq!(served_selection(), None);
    }

    #[test]
    fn copied_text_stays_available_to_other_applications_after_write_returns() {
        let _platform = reset_platform();
        let owner = ClipboardOwner::<SelectionServingBackend>::new();

        owner
            .write_text("12 64 -30".to_owned())
            .expect("write succeeds");

        assert_eq!(served_selection().as_deref(), Some("12 64 -30"));
    }

    #[test]
    fn reads_and_writes_share_one_platform_connection() {
        let _platform = reset_platform();
        let owner = ClipboardOwner::<SelectionServingBackend>::new();

        owner
            .write_text("first".to_owned())
            .expect("write succeeds");
        owner
            .write_text("second".to_owned())
            .expect("write succeeds");
        let pasted = owner.read_text_bounded(64).expect("read succeeds");

        assert_eq!(pasted.as_deref(), Some("second"));
        assert_eq!(OPENED_CONNECTIONS.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn read_rejects_text_over_the_bound() {
        let _platform = reset_platform();
        let owner = ClipboardOwner::<SelectionServingBackend>::new();
        owner
            .write_text("too long".to_owned())
            .expect("write succeeds");

        let error = owner.read_text_bounded(3).expect_err("bound is enforced");

        assert!(matches!(
            error,
            PlatformClipboardError::TooLong { maximum: 3 }
        ));
    }

    #[test]
    fn a_failed_connection_is_replaced_on_the_next_call() {
        let _platform = reset_platform();
        let owner = ClipboardOwner::<SelectionServingBackend>::new();
        owner
            .write_text("first".to_owned())
            .expect("write succeeds");
        FAIL_NEXT_READ.store(true, Ordering::SeqCst);

        owner
            .read_text_bounded(64)
            .expect_err("the platform failed");
        owner
            .write_text("second".to_owned())
            .expect("write succeeds");

        assert_eq!(OPENED_CONNECTIONS.load(Ordering::SeqCst), 2);
        assert_eq!(served_selection().as_deref(), Some("second"));
    }

    #[test]
    fn an_empty_clipboard_pastes_nothing() {
        let _platform = reset_platform();
        let owner = ClipboardOwner::<SelectionServingBackend>::new();

        assert_eq!(
            owner.read_text_bounded(64).expect("empty is not an error"),
            None
        );
        assert_eq!(OPENED_CONNECTIONS.load(Ordering::SeqCst), 1);
    }
}
