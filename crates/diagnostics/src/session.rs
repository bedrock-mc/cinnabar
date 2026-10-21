//! Process-wide identity shared by log headers and crash reports.

use std::sync::LazyLock;

/// Source commit stamped by the build through `markers::BUILD_COMMIT`, or `unknown`.
pub const COMMIT: &str = match option_env!("CINNABAR_BUILD_COMMIT") {
    Some(commit) => commit,
    None => "unknown",
};

/// Cargo profile family of this binary: `debug` with debug assertions, otherwise `release`.
pub const BUILD_PROFILE: &str = if cfg!(debug_assertions) {
    "debug"
} else {
    "release"
};

struct Session {
    id: String,
    started_at_ms: u64,
}

static SESSION: LazyLock<Session> = LazyLock::new(|| {
    let started_at_ms = web_time::SystemTime::now()
        .duration_since(web_time::UNIX_EPOCH)
        .map_or(0, |elapsed| {
            u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)
        });
    Session {
        id: derive_id(started_at_ms, std::process::id()),
        started_at_ms,
    }
});

/// Identifies this process: 16 lowercase hex characters derived from its start time and pid.
///
/// The first call to this function or [`started_at_ms`] reads the clock; later calls never do.
pub fn id() -> &'static str {
    &SESSION.id
}

/// Unix time in milliseconds when this process's session began, fixed by the first call.
pub fn started_at_ms() -> u64 {
    SESSION.started_at_ms
}

/// Mixes start time and pid so concurrent clients and successive launches get distinct ids.
fn derive_id(started_at_ms: u64, pid: u32) -> String {
    // SplitMix64 finalizer: a bijection, so distinct inputs never share an id.
    let mut value = started_at_ms.rotate_left(20) ^ u64::from(pid);
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^= value >> 31;
    format!("{value:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn id_is_sixteen_lowercase_hex_characters_and_stable() {
        let first = id();
        assert_eq!(first.len(), 16);
        assert!(
            first
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        );
        assert_eq!(id(), first);
        assert_eq!(first, derive_id(started_at_ms(), std::process::id()));
    }

    #[test]
    fn different_pids_or_start_times_get_different_ids() {
        let at = 1_760_400_000_000;
        assert_ne!(derive_id(at, 1), derive_id(at, 2));
        assert_ne!(derive_id(at, 1), derive_id(at + 1, 1));
        assert_eq!(derive_id(at, 1), derive_id(at, 1));
        assert_eq!(derive_id(0, 0).len(), 16);
    }

    /// `option_env!` needs a literal; this keeps it tied to the registered marker constant.
    #[test]
    fn commit_reads_the_registered_build_marker() {
        assert_eq!(super::super::markers::BUILD_COMMIT, "CINNABAR_BUILD_COMMIT");
    }
}
