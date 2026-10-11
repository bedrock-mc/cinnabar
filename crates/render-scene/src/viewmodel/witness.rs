//! Opt-in, process-bounded numeric diagnostics. Never rendering authority.
use std::{
    io::{self, Write},
    net::SocketAddr,
    sync::{
        Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
};

const MAX_ROWS: usize = 64;
const MAX_BYTES: usize = 32 * 1024;
static CONFIGURED: AtomicBool = AtomicBool::new(false);
static RETIRED: AtomicBool = AtomicBool::new(false);
static OWNER: OnceLock<Mutex<State>> = OnceLock::new();

#[derive(Clone, Copy, Eq, PartialEq)]
struct Record {
    reason: u8,
    values: [i128; 32],
}
#[derive(Default)]
struct State {
    last: [Option<Record>; 6],
    rows: usize,
    bytes: usize,
    exhausted: bool,
}
impl State {
    fn admit(&mut self, stage: usize, record: Record, bytes: usize) -> bool {
        if self.exhausted || stage >= self.last.len() || self.last[stage] == Some(record) {
            return false;
        }
        if self.rows == MAX_ROWS || bytes > MAX_BYTES.saturating_sub(self.bytes) {
            self.exhausted = true;
            return false;
        }
        self.last[stage] = Some(record);
        self.rows += 1;
        self.bytes += bytes;
        true
    }
}
struct Buffer {
    bytes: [u8; 2048],
    used: usize,
}
impl Write for Buffer {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let end = self
            .used
            .checked_add(bytes.len())
            .filter(|end| *end <= self.bytes.len())
            .ok_or_else(|| io::Error::other("diagnostic row capacity"))?;
        self.bytes[self.used..end].copy_from_slice(bytes);
        self.used = end;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
fn qualified(address: Option<&str>, marker: Option<&str>) -> bool {
    address.zip(marker).is_some_and(|(address, marker)| {
        address == marker
            && address
                .parse::<SocketAddr>()
                .is_ok_and(|address| address.ip().is_loopback() && address.port() != 0)
    })
}
pub fn configure(address: Option<&str>) {
    if CONFIGURED.load(Ordering::Acquire) {
        return;
    }
    let marker = std::env::var("RUST_MCBE_CRAFT_OBSERVATION").ok();
    configure_once(&CONFIGURED, &RETIRED, &OWNER, address, marker.as_deref());
}
fn configure_once(
    configured: &AtomicBool,
    retired: &AtomicBool,
    owner: &OnceLock<Mutex<State>>,
    address: Option<&str>,
    marker: Option<&str>,
) {
    if configured.swap(true, Ordering::AcqRel) || retired.load(Ordering::Acquire) {
        return;
    }
    if qualified(address, marker) {
        let _ = owner.set(Mutex::new(State::default()));
    }
}
pub fn retire() {
    RETIRED.store(true, Ordering::Release);
}
pub fn enabled() -> bool {
    !RETIRED.load(Ordering::Acquire) && OWNER.get().is_some()
}
pub fn record(stage: usize, reason: u8, values: [i128; 32]) {
    if !enabled() {
        return;
    }
    let Some(owner) = OWNER.get() else {
        return;
    };
    let mut state = owner.lock().unwrap_or_else(|error| error.into_inner());
    let record = Record { reason, values };
    if stage >= 6 || state.exhausted || state.last[stage] == Some(record) {
        return;
    }
    let mut row = Buffer {
        bytes: [0; 2048],
        used: 0,
    };
    if writeln!(
        row,
        "RUST_MCBE_HAND_WITNESS stage={stage} reason={reason} values={values:?}"
    )
    .is_err()
    {
        state.exhausted = true;
        RETIRED.store(true, Ordering::Release);
        return;
    }
    let admitted = state.admit(stage, record, row.used);
    if state.exhausted {
        RETIRED.store(true, Ordering::Release);
    }
    if admitted && !cfg!(test) {
        let _ = io::stderr().lock().write_all(&row.bytes[..row.used]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_endpoint_only() {
        assert!(qualified(Some("127.0.0.1:60475"), Some("127.0.0.1:60475")));
        for marker in [
            None,
            Some("1"),
            Some("127.0.0.1:60476"),
            Some("192.0.2.1:60475"),
        ] {
            assert!(!qualified(Some("127.0.0.1:60475"), marker));
        }
        assert!(!qualified(Some("localhost:60475"), Some("localhost:60475")));
    }
    #[test]
    fn shared_stages_changes_and_budget_never_remint() {
        let mut state = State::default();
        let record = Record {
            reason: 1,
            values: [0; 32],
        };
        assert!(state.admit(0, record, 1));
        assert!(!state.admit(0, record, 1));
        for n in 1..MAX_ROWS {
            assert!(state.admit(
                n % 6,
                Record {
                    values: [n as i128; 32],
                    ..record
                },
                1
            ));
        }
        assert!(!state.admit(
            0,
            Record {
                reason: 2,
                ..record
            },
            1
        ));
        assert!(state.exhausted);
        assert!(!state.admit(5, record, 0));
        assert_eq!(state.rows, MAX_ROWS);
        let mut bytes = State::default();
        assert!(bytes.admit(0, record, MAX_BYTES));
        assert!(!bytes.admit(1, record, 1));
        assert_eq!(bytes.bytes, MAX_BYTES);
    }
    #[test]
    fn concurrent_stage_writers_share_one_budget() {
        let state = std::sync::Arc::new(Mutex::new(State::default()));
        std::thread::scope(|scope| {
            for stage in 0..6 {
                let state = state.clone();
                scope.spawn(move || {
                    for value in 0..100 {
                        state.lock().unwrap().admit(
                            stage,
                            Record {
                                reason: 1,
                                values: [value; 32],
                            },
                            100,
                        );
                    }
                });
            }
        });
        let state = state.lock().unwrap();
        assert_eq!(state.rows, MAX_ROWS);
        assert_eq!(state.bytes, MAX_ROWS * 100);
    }
    #[test]
    fn configuration_and_retirement_are_permanent_without_global_reset() {
        let configured = AtomicBool::new(false);
        let retired = AtomicBool::new(false);
        let owner = OnceLock::new();
        configure_once(&configured, &retired, &owner, Some("127.0.0.1:60475"), None);
        configure_once(
            &configured,
            &retired,
            &owner,
            Some("127.0.0.1:60475"),
            Some("127.0.0.1:60475"),
        );
        assert!(owner.get().is_none());
        let configured = AtomicBool::new(false);
        let owner = OnceLock::new();
        configure_once(
            &configured,
            &retired,
            &owner,
            Some("127.0.0.1:60475"),
            Some("127.0.0.1:60475"),
        );
        owner.get().unwrap().lock().unwrap().rows = 12;
        retired.store(true, Ordering::Release);
        configure_once(
            &configured,
            &retired,
            &owner,
            Some("127.0.0.1:60475"),
            Some("127.0.0.1:60475"),
        );
        assert_eq!(owner.get().unwrap().lock().unwrap().rows, 12);
        assert!(retired.load(Ordering::Acquire));
    }
}
