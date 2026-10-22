//! Small startup observations that separate server CPU work from elapsed time.

use std::time::Instant;

use super::bds::BDS_READY_MARKER;

/// Public server output milestones; only their fixed labels enter the timing log.
const MARKERS: [&str; 7] = [
    "Starting Server",
    "LOADING VANILLA WORLD",
    "Opening level",
    "Accepting clients",
    "Pack Stack",
    "Signed in to signaling",
    BDS_READY_MARKER,
];

/// Records each startup milestone once without promoting arbitrary server output to info.
pub(super) struct StartupTiming {
    started: Instant,
    pid: u32,
    seen: [bool; MARKERS.len()],
}

impl StartupTiming {
    /// Starts the observation before spawning the child, including its loader work.
    pub(super) fn new(started: Instant, pid: u32) -> Self {
        Self {
            started,
            pid,
            seen: [false; MARKERS.len()],
        }
    }

    /// Logs a recognized milestone with elapsed and, where available, process CPU time.
    pub(super) fn observe(&mut self, label: &str, line: &str) {
        let Some(index) = marker(line) else {
            return;
        };
        if std::mem::replace(&mut self.seen[index], true) {
            return;
        }
        let elapsed_ms = self.started.elapsed().as_millis();
        let phase = MARKERS[index];
        if let Some(child_cpu_ms) = process_cpu_ms(self.pid) {
            tracing::info!(
                label,
                phase,
                elapsed_ms,
                child_cpu_ms,
                "local server startup"
            );
        } else {
            tracing::info!(label, phase, elapsed_ms, "local server startup");
        }
    }
}

/// Recognizes server message prefixes after its optional timestamp and banner decoration.
fn marker(line: &str) -> Option<usize> {
    let line = line.trim();
    let message = if line.starts_with('[') {
        line.split_once("] ")?.1
    } else {
        line
    };
    let message = message.trim_matches('#').trim();
    MARKERS.iter().position(|marker| {
        message.strip_prefix(marker).is_some_and(|suffix| {
            suffix.is_empty()
                || suffix.starts_with(char::is_whitespace)
                || suffix.starts_with([':', '-'])
        })
    })
}

/// Reads CPU usage only at startup milestones; inaccessible process counters are optional.
#[cfg(any(target_os = "linux", target_os = "android"))]
fn process_cpu_ms(pid: u32) -> Option<u64> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // SAFETY: sysconf reads a process-independent clock frequency and takes no pointers.
    let ticks_per_second = u64::try_from(unsafe { libc::sysconf(libc::_SC_CLK_TCK) }).ok()?;
    cpu_ms(&stat, ticks_per_second)
}

/// Other platforms still record elapsed time without an additional process-inspection API.
#[cfg(not(any(target_os = "linux", target_os = "android")))]
fn process_cpu_ms(_pid: u32) -> Option<u64> {
    None
}

/// Parses Linux's user and system counters, allowing spaces and parentheses in the command.
#[cfg(any(target_os = "linux", target_os = "android", test))]
fn cpu_ms(stat: &str, ticks_per_second: u64) -> Option<u64> {
    let (_, fields) = stat.rsplit_once(')')?;
    let mut fields = fields.split_whitespace();
    let user = fields.nth(11)?.parse::<u64>().ok()?;
    let system = fields.next()?.parse::<u64>().ok()?;
    let ticks = u128::from(user.checked_add(system)?);
    u64::try_from((ticks * 1000).checked_div(u128::from(ticks_per_second))?).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_only_startup_message_prefixes() {
        for (index, milestone) in MARKERS.iter().enumerate() {
            assert_eq!(marker(milestone), Some(index));
            assert_eq!(
                marker(&format!("[timestamp INFO] {milestone}")),
                Some(index)
            );
            assert_eq!(marker(&format!("#   {milestone}   #")), Some(index));
            assert_eq!(marker(&format!("unrelated {milestone}")), None);
            assert_eq!(marker(&format!("{milestone}extra")), None);
        }
        assert!(marker("Opening level 'worlds/example/db'").is_some());
        assert!(marker("Pack Stack - None").is_some());
        assert!(marker("Accepting clients on loopback").is_some());
        assert_eq!(marker("[unfinished timestamp"), None);
    }

    #[test]
    fn parses_cpu_counters_after_the_entire_command() {
        let stat = "42 (server ) worker) R 1 2 3 4 5 6 7 8 9 10 123 27 999 999";
        assert_eq!(cpu_ms(stat, 100), Some(1500));
        assert_eq!(cpu_ms(stat, 1000), Some(150));
        assert_eq!(cpu_ms(stat, 0), None);
        assert_eq!(cpu_ms("42 (server) R 1", 100), None);
        assert_eq!(cpu_ms("42 server R 1", 100), None);
        assert_eq!(cpu_ms(&stat.replace("123", "-1"), 100), None);
        assert_eq!(cpu_ms(&stat.replace("123", "bad"), 100), None);
        assert_eq!(
            cpu_ms(&stat.replace("123", &u64::MAX.to_string()), 100),
            None
        );
    }
}
