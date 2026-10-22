//! The native fault handler and crash monitor for iOS, which has neither: it forbids starting
//! another process, and `crash-handler` has no iOS support. Nothing ever connects, so a panic
//! writes its folder in-process and a native fault ends the process without a report.

/// Native fault capture, which attaches nothing.
pub(super) mod handler {
    use super::super::{Context, LogHooks};

    /// Attaches no handler: native faults take the system's default action unreported.
    pub(in crate::lifecycle::crash) fn install(_context: &'static Context, _hooks: LogHooks) {}

    /// Dumps nothing, as no monitor is ever connected to receive a dump.
    pub(in crate::lifecycle::crash) fn dump_this_thread() -> bool {
        false
    }
}

/// The crash monitor, which never starts: every query reports none and every send fails.
pub mod monitor {
    use std::{io, path::Path, time::Duration};

    use super::super::protocol::{Hello, Restart};

    /// How to start the monitor; no value exists, since iOS cannot start one.
    #[derive(Clone, Debug)]
    pub enum MonitorCommand {}

    impl MonitorCommand {
        /// Fails with [`io::ErrorKind::Unsupported`]: iOS forbids starting another process.
        pub fn current_exe() -> io::Result<Self> {
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "iOS cannot start a crash monitor process",
            ))
        }
    }

    /// A connection to the monitor; no value exists, since none is ever opened.
    pub(in crate::lifecycle::crash) enum Client {}

    /// Unreachable: no [`MonitorCommand`] exists to start from.
    pub(in crate::lifecycle::crash) fn start(
        command: &MonitorCommand,
        _crash_dir: &Path,
        _hello: &Hello,
        _restart: Restart,
    ) {
        match *command {}
    }

    /// Never reconnects: false.
    pub(in crate::lifecycle::crash) fn reconnect() -> bool {
        false
    }

    /// No connection is ever up.
    pub(in crate::lifecycle::crash) fn connection() -> Option<Client> {
        None
    }

    /// Every crash writes its folder in-process.
    pub(in crate::lifecycle::crash) fn route() -> Option<Client> {
        None
    }

    /// Sends nothing: false.
    pub(in crate::lifecycle::crash) fn send(_kind: u32, _payload: &[u8]) -> bool {
        false
    }

    /// Unreachable: no [`Client`] exists to send on.
    pub(in crate::lifecycle::crash) fn send_on(
        client: &Client,
        _kind: u32,
        _payload: &[u8],
    ) -> bool {
        match *client {}
    }

    /// Returns false at once: no connection attempt is ever made.
    pub fn wait_for_connection(_timeout: Duration) -> bool {
        false
    }

    /// No monitor process exists.
    pub fn pid() -> Option<u32> {
        None
    }

    /// No monitor socket exists.
    pub fn socket() -> Option<&'static Path> {
        None
    }
}
