//! Local crash capture: a panic hook records a report on disk for debugging; nothing is uploaded.

use std::{
    backtrace::Backtrace,
    fs,
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use serde::Serialize;

use launcher::install_layout::InstallLayout;

const LOG_TAIL_BYTES: u64 = 16 * 1024;
const MAX_REPORTS: usize = 8;

#[derive(Debug, Serialize)]
struct Report<'a> {
    source: &'a str,
    message: String,
    backtrace: String,
    log_tail: String,
    release: &'a str,
    os: &'a str,
    arch: &'a str,
}

/// Records a report under the crash directory for every panic.
pub(crate) fn install_panic_hook(layout: &InstallLayout) {
    let crash_dir = layout.crash_dir();
    let core_log = layout.log_dir().join("core.log");
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        write_report(&crash_dir, &core_log, &info.to_string());
        // Queued log lines precede the panic message.
        diagnostics::console::flush_before_exit();
        previous(info);
    }));
}

/// Drops the oldest reports beyond the bound so a crash loop cannot fill the disk.
pub(crate) fn prune_reports(layout: &InstallLayout) {
    prune(&layout.crash_dir(), MAX_REPORTS);
}

fn prune(crash_dir: &Path, keep: usize) {
    let mut files = reports(crash_dir);
    while files.len() > keep {
        let _ = fs::remove_file(files.remove(0));
    }
}

fn write_report(crash_dir: &Path, core_log: &Path, message: &str) {
    let report = Report {
        source: "client",
        message: message.to_owned(),
        backtrace: Backtrace::force_capture().to_string(),
        log_tail: tail(core_log, LOG_TAIL_BYTES),
        release: env!("CARGO_PKG_VERSION"),
        os: std::env::consts::OS,
        arch: std::env::consts::ARCH,
    };
    let Ok(bytes) = serde_json::to_vec(&report) else {
        return;
    };
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis());
    write_at(crash_dir, millis, &bytes);
}

/// Writes one report at its timestamp without losing a simultaneous report.
fn write_at(crash_dir: &Path, millis: u128, bytes: &[u8]) {
    if fs::create_dir_all(crash_dir).is_err() {
        return;
    }
    for suffix in 0_u64.. {
        let path = crash_dir.join(format!("crash-{millis}-{suffix:020}.json"));
        match fs::File::options().write(true).create_new(true).open(path) {
            Ok(mut file) => {
                let _ = file.write_all(bytes);
                return;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(_) => return,
        }
    }
}

fn tail(path: &Path, limit: u64) -> String {
    let Ok(mut file) = fs::File::open(path) else {
        return String::new();
    };
    let length = file.metadata().map_or(0, |meta| meta.len());
    let _ = file.seek(SeekFrom::Start(length.saturating_sub(limit)));
    let mut bytes = Vec::new();
    let _ = file.take(limit).read_to_end(&mut bytes);
    String::from_utf8_lossy(&bytes).into_owned()
}

fn reports(crash_dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = fs::read_dir(crash_dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .collect();
    files.sort();
    files
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(label: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("cinnabar-crash-{}-{label}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn names(dir: &Path) -> Vec<String> {
        reports(dir)
            .iter()
            .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn review_same_timestamp_crashes_are_both_retained() {
        let dir = scratch("simultaneous");
        std::thread::scope(|scope| {
            scope.spawn(|| write_at(&dir, 1234, b"first"));
            scope.spawn(|| write_at(&dir, 1234, b"second"));
        });
        let mut contents = reports(&dir)
            .into_iter()
            .map(|p| fs::read(p).unwrap())
            .collect::<Vec<_>>();
        contents.sort();
        fs::remove_dir_all(dir).unwrap();
        assert_eq!(contents, [b"first".to_vec(), b"second".to_vec()]);
    }

    #[test]
    fn report_records_the_panic_and_the_log_tail() {
        let dir = scratch("report");
        let log = dir.join("core.log");
        fs::write(&log, "old\nrecent line").unwrap();
        write_report(&dir.join("crashes"), &log, "boom");
        let file = reports(&dir.join("crashes")).remove(0);
        let value: serde_json::Value = serde_json::from_slice(&fs::read(file).unwrap()).unwrap();
        for key in [
            "source",
            "message",
            "backtrace",
            "log_tail",
            "release",
            "os",
            "arch",
        ] {
            assert!(value.get(key).is_some(), "missing {key}");
        }
        assert_eq!(value["source"], "client");
        assert_eq!(value["message"], "boom");
        assert!(value["log_tail"].as_str().unwrap().contains("recent line"));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn tail_is_bounded_to_the_final_bytes() {
        let dir = scratch("tail");
        let log = dir.join("l");
        fs::write(&log, "0123456789").unwrap();
        assert_eq!(tail(&log, 4), "6789");
        assert_eq!(tail(&dir.join("missing"), 4), "");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn prune_keeps_only_the_newest_json_reports() {
        let dir = scratch("prune");
        for name in ["crash-2.json", "crash-1.json", "crash-3.json", "note.txt"] {
            fs::write(dir.join(name), "{}").unwrap();
        }
        prune(&dir, 2);
        assert_eq!(names(&dir), ["crash-2.json", "crash-3.json"]);
        assert!(dir.join("note.txt").is_file());
        fs::remove_dir_all(dir).unwrap();
    }
}
