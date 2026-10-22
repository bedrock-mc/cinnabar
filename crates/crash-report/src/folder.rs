//! Crash folders under `crashes/`: `<UTC yyyymmdd-hhmmss>-<id>/` with fixed member files.

use std::{
    fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

use crate::{
    report::{Report, SCHEMA},
    section, utc,
};

/// Crash folders kept on disk, so a crash loop cannot fill it.
pub const MAX_REPORTS: usize = 8;
/// The report inside each folder.
pub const REPORT_FILE: &str = "report.json";
/// An in-process fallback has finished every member and a healthy sender may recover it.
pub const FALLBACK_READY_FILE: &str = ".fallback-ready";
/// The crashing session's section of `client.log`.
pub const CLIENT_LOG_FILE: &str = "client.log";
/// The crashing session's section of `core.log`.
pub const CORE_LOG_FILE: &str = "core.log";
/// The native fault's minidump, written by the crash monitor.
pub const DUMP_FILE: &str = "crash.dmp";
/// The Go core's runtime crash output (panic and goroutine traces), in `core` reports.
pub const CORE_CRASH_FILE: &str = "core-crash.txt";
/// The packet header ring at the crash as a table, oldest first, with packet names.
pub const PACKETS_FILE: &str = "packets.txt";
/// The last log lines the flight recorder kept, including levels the session files leave out.
pub const FLIGHT_LOG_FILE: &str = "flight.log";
/// Bodies of packets the crashing session rejected, with an `index.txt` describing them.
pub const DECODE_SAMPLES_DIR: &str = "decode-samples";
/// Largest `report.json` that [`read`] accepts.
const MAX_REPORT_BYTES: u64 = 16 << 20;
/// Same-second folders with one id before a write gives up.
const MAX_COLLISIONS: usize = 1024;

/// Log sections stored beside a report; empty sections are written as empty files.
#[derive(Clone, Debug, Default)]
pub struct Logs {
    pub client: Vec<u8>,
    pub core: Vec<u8>,
}

/// Where a crashing session's logs live.
#[derive(Clone, Copy, Debug)]
pub struct LogSources<'a> {
    /// The reporting client session; absent for logs without session identities.
    pub session_id: Option<&'a str>,
    pub client: &'a Path,
    pub core: &'a Path,
    /// Unix milliseconds when the session began; `core.log` sections from before it belong to
    /// an earlier session and are left out.
    pub started_at_ms: u64,
}

impl Logs {
    /// Reads the crashing session's sections of both logs; an unreadable log gives an empty
    /// section.
    pub fn read(sources: &LogSources<'_>) -> Self {
        Self {
            client: match sources.session_id {
                Some(session_id) => section::session::read(
                    sources.client,
                    section::CLIENT_SESSION_START,
                    session_id,
                    section::MAX_SECTION_BYTES,
                ),
                None => section::read(
                    sources.client,
                    section::CLIENT_SESSION_START,
                    section::MAX_SECTION_BYTES,
                ),
            }
            .unwrap_or_default(),
            core: match sources.session_id {
                Some(session_id) => section::session::core(
                    sources.core,
                    session_id,
                    section::MAX_SECTION_BYTES,
                    sources.started_at_ms,
                ),
                None => section::read_since(
                    sources.core,
                    section::CORE_SESSION_START,
                    section::MAX_SECTION_BYTES,
                    sources.started_at_ms,
                ),
            }
            .unwrap_or_default(),
        }
    }
}

/// Names the folder for a crash at `now_ms` (Unix milliseconds); `id` keeps `[0-9a-z-]` only.
pub fn name(now_ms: u64, id: &str) -> String {
    let id: String = id
        .chars()
        .filter(|character| {
            character.is_ascii_digit() || character.is_ascii_lowercase() || *character == '-'
        })
        .collect();
    let id = if id.is_empty() {
        "unknown"
    } else {
        id.as_str()
    };
    format!("{}-{id}", utc::stamp(now_ms))
}

/// Unix milliseconds of the UTC second a crash folder's name starts with.
pub fn created_at_ms(folder: &Path) -> Option<u64> {
    let name = folder.file_name()?.to_str()?;
    let stamp = name.get(..utc::STAMP_LEN)?;
    if name.as_bytes().get(utc::STAMP_LEN) != Some(&b'-') {
        return None;
    }
    utc::parse(stamp)
}

/// Writes `report` and `logs` into a new folder named by `now_ms` and the report's session id,
/// moving the minidump at `dump`, when given, in as [`DUMP_FILE`].
///
/// Prunes first so the directory holds at most [`MAX_REPORTS`] folders afterwards. A folder
/// that already exists gets a `-N` suffix, so simultaneous writers never share one. Every file
/// is synced before this returns its folder.
pub fn write(
    crash_dir: &Path,
    report: &Report,
    logs: &Logs,
    dump: Option<&Path>,
    now_ms: u64,
) -> io::Result<PathBuf> {
    write_with(crash_dir, report, logs, dump, &[], now_ms)
}

/// An extra member of a crash folder, such as [`CORE_CRASH_FILE`].
#[derive(Clone, Copy, Debug)]
pub struct Attachment<'a> {
    /// A relative path of normal components, such as `core-crash.txt` or `samples/index.txt`;
    /// it may not name one of the fixed members.
    pub name: &'a str,
    pub bytes: &'a [u8],
}

/// [`write`] plus `attachments`, each written beside the fixed members with its parent
/// directories created inside the folder.
///
/// Every attachment name is checked before anything is written: an absolute path, a `..` or
/// other non-normal component, a duplicate, or a fixed member's name fails with
/// [`io::ErrorKind::InvalidInput`].
pub fn write_with(
    crash_dir: &Path,
    report: &Report,
    logs: &Logs,
    dump: Option<&Path>,
    attachments: &[Attachment<'_>],
    now_ms: u64,
) -> io::Result<PathBuf> {
    for (index, attachment) in attachments.iter().enumerate() {
        check_attachment(attachment.name)?;
        if attachments[..index]
            .iter()
            .any(|earlier| Path::new(earlier.name) == Path::new(attachment.name))
        {
            return Err(invalid_attachment(attachment.name));
        }
    }
    prune(crash_dir, MAX_REPORTS - 1);
    fs::create_dir_all(crash_dir)?;
    let base = name(now_ms, &report.session_id);
    let folder = create_unique(crash_dir, &base)?;
    write_new(
        &folder.join(REPORT_FILE),
        &serde_json::to_vec_pretty(report)?,
    )?;
    write_new(&folder.join(CLIENT_LOG_FILE), &logs.client)?;
    write_new(&folder.join(CORE_LOG_FILE), &logs.core)?;
    if let Some(dump) = dump {
        move_file(dump, &folder.join(DUMP_FILE))?;
    }
    for attachment in attachments {
        let path = folder.join(attachment.name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        write_new(&path, attachment.bytes)?;
    }
    Ok(folder)
}

/// Marks an in-process fallback only after every report member is finished.
/// This grants no consent; a healthy sender still checks the player's current settings.
pub fn mark_fallback_ready(folder: &Path) -> io::Result<()> {
    write_new(&folder.join(FALLBACK_READY_FILE), b"")
}

/// Accepts relative paths of normal components whose first component is not a fixed member.
fn check_attachment(name: &str) -> io::Result<()> {
    let mut components = Path::new(name).components();
    let first = components.next();
    let normal = first.is_some()
        && first
            .into_iter()
            .chain(components)
            .all(|component| matches!(component, std::path::Component::Normal(_)));
    let fixed = first.is_some_and(|first| {
        [
            REPORT_FILE,
            CLIENT_LOG_FILE,
            CORE_LOG_FILE,
            DUMP_FILE,
            FALLBACK_READY_FILE,
        ]
        .iter()
        .any(|member| first.as_os_str() == *member)
    });
    if normal && !fixed {
        Ok(())
    } else {
        Err(invalid_attachment(name))
    }
}

fn invalid_attachment(name: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        format!("crash folder attachment `{name}` is not a free relative member path"),
    )
}

/// Renames `from` to `to`, copying across volumes.
fn move_file(from: &Path, to: &Path) -> io::Result<()> {
    if fs::rename(from, to).is_ok() {
        return Ok(());
    }
    fs::copy(from, to)?;
    fs::remove_file(from)
}

/// Reads a folder's report, rejecting other schema versions.
pub fn read(folder: &Path) -> io::Result<Report> {
    let mut bytes = Vec::new();
    fs::File::open(folder.join(REPORT_FILE))?
        .take(MAX_REPORT_BYTES)
        .read_to_end(&mut bytes)?;
    let report: Report = serde_json::from_slice(&bytes)?;
    if report.schema != SCHEMA {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("crash report schema {} is not {SCHEMA}", report.schema),
        ));
    }
    Ok(report)
}

/// Crash folders in `crash_dir`, oldest first; other entries are ignored.
/// The report file's write time orders crashes without later upload-state changes affecting
/// retention. Prepared folder names can predate a fault; names are only a fallback before a
/// report is written. The path breaks ties when filesystem times are equal.
pub fn list(crash_dir: &Path) -> Vec<PathBuf> {
    let mut folders: Vec<PathBuf> = fs::read_dir(crash_dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .map(|entry| entry.path())
        .filter(|path| created_at_ms(path).is_some())
        .collect();
    folders.sort_by_cached_key(|path| {
        let written = fs::metadata(path.join(REPORT_FILE))
            .and_then(|metadata| metadata.modified())
            .ok()
            .or_else(|| {
                created_at_ms(path).and_then(|at| {
                    std::time::UNIX_EPOCH.checked_add(std::time::Duration::from_millis(at))
                })
            });
        (written, path.clone())
    });
    folders
}

/// Deletes legacy `crash-*.json` report files, then the oldest folders beyond `keep`.
pub fn prune(crash_dir: &Path, keep: usize) {
    for entry in fs::read_dir(crash_dir).into_iter().flatten().flatten() {
        let name = entry.file_name();
        let legacy = name
            .to_str()
            .is_some_and(|name| name.starts_with("crash-") && name.ends_with(".json"));
        if legacy && entry.file_type().is_ok_and(|kind| kind.is_file()) {
            let _ = fs::remove_file(entry.path());
        }
    }
    let folders = list(crash_dir);
    let excess = folders.len().saturating_sub(keep);
    for folder in &folders[..excess] {
        let _ = fs::remove_dir_all(folder);
    }
}

/// Creates and returns `crash_dir/base`, or the first free `base-N` when other writers already
/// hold it; fails after [`MAX_COLLISIONS`] taken names or on any other I/O error.
fn create_unique(crash_dir: &Path, base: &str) -> io::Result<PathBuf> {
    for suffix in 0..MAX_COLLISIONS {
        let folder = if suffix == 0 {
            crash_dir.join(base)
        } else {
            crash_dir.join(format!("{base}-{suffix}"))
        };
        match fs::create_dir(&folder) {
            Ok(()) => return Ok(folder),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "too many crash folders share one second",
    ))
}

/// Writes `bytes` to a file that must not exist yet and syncs it to disk before returning.
fn write_new(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = fs::File::options()
        .write(true)
        .create_new(true)
        .open(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::{Identity, Kind};

    const NOW_MS: u64 = 1_760_443_200_000;

    /// Creates a report for the fixture session at its fixed timestamp.
    fn report(message: &str) -> Report {
        let identity = Identity {
            session_id: "0123456789abcdef".into(),
            started_at_ms: NOW_MS - 5_000,
            release: "1.2.3".into(),
            commit: "abc123".into(),
            build_profile: "debug".into(),
        };
        Report::new(Kind::Panic, message.into(), &identity, NOW_MS)
    }

    /// Lists retained folder names in their chronological order.
    fn names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    /// Sets synthetic filesystem time without reading the clock or waiting for a write interval.
    fn set_modified(path: &Path, at_ms: u64) {
        let mut options = fs::OpenOptions::new();
        options.read(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            options.custom_flags(0x02000000).access_mode(0x100);
        }
        options
            .open(path)
            .unwrap()
            .set_times(
                fs::FileTimes::new()
                    .set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_millis(at_ms)),
            )
            .unwrap();
    }

    #[test]
    fn uploading_newest_first_cannot_reverse_crash_retention() {
        use crate::upload::state::{self, Consent, Outcome};
        // A prepared signal folder can be named long before the fault writes its report.
        for newer_stamp in [NOW_MS, NOW_MS - 3_600_000] {
            let dir = tempfile::tempdir().unwrap();
            let older =
                write(dir.path(), &report("older"), &Logs::default(), None, NOW_MS).unwrap();
            let newer = write(
                dir.path(),
                &report("newer"),
                &Logs::default(),
                None,
                newer_stamp,
            )
            .unwrap();
            set_modified(&older.join(REPORT_FILE), NOW_MS);
            set_modified(&newer.join(REPORT_FILE), NOW_MS + 1);
            for (folder, id) in [(&newer, "a"), (&older, "b")] {
                state::consent(folder, Consent::User, &id.repeat(32), NOW_MS).unwrap();
                state::record(folder, &Outcome::Sent, NOW_MS).unwrap();
            }
            // Uploads replace files in newest-first order; simulate their directory write times.
            set_modified(&newer, NOW_MS + 10);
            set_modified(&older, NOW_MS + 20);
            assert_eq!(list(dir.path()), [older.clone(), newer.clone()]);
            prune(dir.path(), 1);
            assert!(!older.exists());
            assert!(newer.exists());
        }
    }

    #[test]
    fn write_creates_a_dated_folder_that_reads_back() {
        let dir = tempfile::tempdir().unwrap();
        let crash_dir = dir.path().join("crashes");
        let logs = Logs {
            client: b"CLIENT_SESSION_START\nclient\n".to_vec(),
            core: b"CORE_SESSION_START\ncore\n".to_vec(),
        };
        fs::create_dir_all(&crash_dir).unwrap();
        let dump = crash_dir.join("pending.dmp");
        fs::write(&dump, b"MDMP").unwrap();
        let folder = write(&crash_dir, &report("boom"), &logs, Some(&dump), NOW_MS).unwrap();
        assert_eq!(
            folder.file_name().unwrap(),
            "20251014-120000-0123456789abcdef"
        );
        assert_eq!(created_at_ms(&folder), Some(NOW_MS));
        assert_eq!(read(&folder).unwrap(), report("boom"));
        assert_eq!(fs::read(folder.join(CLIENT_LOG_FILE)).unwrap(), logs.client);
        assert_eq!(fs::read(folder.join(CORE_LOG_FILE)).unwrap(), logs.core);
        assert_eq!(fs::read(folder.join(DUMP_FILE)).unwrap(), b"MDMP");
        assert!(!dump.exists());
        assert_eq!(list(&crash_dir), [folder]);
    }

    #[test]
    fn attachments_land_inside_the_folder_in_their_subdirectories() {
        let dir = tempfile::tempdir().unwrap();
        let folder = write_with(
            dir.path(),
            &report("core"),
            &Logs::default(),
            None,
            &[
                Attachment {
                    name: CORE_CRASH_FILE,
                    bytes: b"panic: boom\n",
                },
                Attachment {
                    name: "samples/index.txt",
                    bytes: b"index\n",
                },
            ],
            NOW_MS,
        )
        .unwrap();
        assert_eq!(
            fs::read(folder.join(CORE_CRASH_FILE)).unwrap(),
            b"panic: boom\n"
        );
        assert_eq!(
            fs::read(folder.join("samples").join("index.txt")).unwrap(),
            b"index\n"
        );
        assert_eq!(read(&folder).unwrap(), report("core"));
    }

    #[test]
    fn attachments_cannot_escape_or_replace_fixed_members() {
        let dir = tempfile::tempdir().unwrap();
        let absolute = dir.path().join("outside.txt");
        let absolute = absolute.to_str().unwrap();
        for name in [
            "",
            "../outside.txt",
            "samples/../../outside.txt",
            "./core-crash.txt",
            absolute,
            REPORT_FILE,
            CORE_LOG_FILE,
            FALLBACK_READY_FILE,
            "crash.dmp/inner.txt",
        ] {
            let error = write_with(
                dir.path(),
                &report("core"),
                &Logs::default(),
                None,
                &[Attachment { name, bytes: b"x" }],
                NOW_MS,
            )
            .unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::InvalidInput, "{name}");
        }
        let twice = [Attachment {
            name: "a.txt",
            bytes: b"x",
        }; 2];
        let error = write_with(
            dir.path(),
            &report("core"),
            &Logs::default(),
            None,
            &twice,
            NOW_MS,
        )
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        assert!(
            list(dir.path()).is_empty(),
            "a rejected write creates no folder"
        );
        assert!(!dir.path().join("outside.txt").exists());
    }

    #[test]
    fn logs_read_this_sessions_sections() {
        let dir = tempfile::tempdir().unwrap();
        let client = dir.path().join("client.log");
        let core = dir.path().join("core.log");
        fs::write(
            &client,
            "CLIENT_SESSION_START session=old\nold\nCLIENT_SESSION_START session=new\nnew\n",
        )
        .unwrap();
        fs::write(&core, "CORE_SESSION_START timestamp_ms=5\nearlier core\n").unwrap();
        let logs = Logs::read(&LogSources {
            session_id: None,
            client: &client,
            core: &core,
            started_at_ms: 10,
        });
        assert_eq!(logs.client, b"CLIENT_SESSION_START session=new\nnew\n");
        assert!(logs.core.is_empty());
        let missing = dir.path().join("missing.log");
        let logs = Logs::read(&LogSources {
            session_id: None,
            client: &missing,
            core: &missing,
            started_at_ms: 0,
        });
        assert!(logs.client.is_empty() && logs.core.is_empty());
    }

    #[test]
    fn simultaneous_writes_do_not_collide() {
        let dir = tempfile::tempdir().unwrap();
        let crash_dir = dir.path().join("crashes");
        let target = crash_dir.as_path();
        let folders: Vec<PathBuf> = std::thread::scope(|scope| {
            let writers: Vec<_> = (0..4)
                .map(|index| {
                    scope.spawn(move || {
                        write(
                            target,
                            &report(&format!("crash {index}")),
                            &Logs::default(),
                            None,
                            NOW_MS,
                        )
                        .unwrap()
                    })
                })
                .collect();
            writers
                .into_iter()
                .map(|writer| writer.join().unwrap())
                .collect()
        });
        let mut messages: Vec<String> = folders
            .iter()
            .map(|folder| read(folder).unwrap().message)
            .collect();
        messages.sort();
        assert_eq!(messages, ["crash 0", "crash 1", "crash 2", "crash 3"]);
        assert_eq!(list(&crash_dir).len(), 4);
    }

    #[test]
    fn prune_keeps_the_newest_folders_and_removes_legacy_files() {
        let dir = tempfile::tempdir().unwrap();
        let crash_dir = dir.path();
        for second in 0..10_u64 {
            fs::create_dir(crash_dir.join(name(NOW_MS + second * 1000, "a"))).unwrap();
        }
        for legacy in [
            "crash-1-00000000000000000000.json",
            "crash-2-native.json",
            "crash-3-4-unix.json",
        ] {
            fs::write(crash_dir.join(legacy), "{}").unwrap();
        }
        fs::write(crash_dir.join("notes.txt"), "keep").unwrap();
        fs::create_dir(crash_dir.join("unrelated")).unwrap();
        prune(crash_dir, MAX_REPORTS);
        let mut expected: Vec<String> = (2..10_u64)
            .map(|second| name(NOW_MS + second * 1000, "a"))
            .collect();
        expected.extend(["notes.txt".to_owned(), "unrelated".to_owned()]);
        expected.sort();
        assert_eq!(names(crash_dir), expected);
    }

    #[test]
    fn writing_keeps_at_most_the_bound() {
        let dir = tempfile::tempdir().unwrap();
        for second in 0..12_u64 {
            write(
                dir.path(),
                &report("loop"),
                &Logs::default(),
                None,
                NOW_MS + second * 1000,
            )
            .unwrap();
        }
        let folders = list(dir.path());
        assert_eq!(folders.len(), MAX_REPORTS);
        assert_eq!(created_at_ms(&folders[0]), Some(NOW_MS + 4_000));
    }

    /// Same-second crash loops retain the latest writes even when folder names are reused.
    #[test]
    fn same_second_crashes_are_pruned_in_write_order() {
        let dir = tempfile::tempdir().unwrap();
        for index in 0..24_u64 {
            let folder = write(
                dir.path(),
                &report(&format!("crash {index}")),
                &Logs::default(),
                None,
                NOW_MS,
            )
            .unwrap();
            set_modified(&folder.join(REPORT_FILE), NOW_MS + index);
        }
        let messages: Vec<_> = list(dir.path())
            .iter()
            .map(|folder| read(folder).unwrap().message)
            .collect();
        let expected: Vec<_> = (16..24).map(|index| format!("crash {index}")).collect();
        assert_eq!(messages, expected);
    }

    #[test]
    fn read_rejects_other_schema_versions() {
        let dir = tempfile::tempdir().unwrap();
        let folder = write(dir.path(), &report("boom"), &Logs::default(), None, NOW_MS).unwrap();
        let mut value: serde_json::Value =
            serde_json::from_slice(&fs::read(folder.join(REPORT_FILE)).unwrap()).unwrap();
        value["schema"] = 2.into();
        fs::write(folder.join(REPORT_FILE), value.to_string()).unwrap();
        assert_eq!(
            read(&folder).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
    }

    #[test]
    fn names_keep_only_safe_id_characters() {
        assert_eq!(name(0, ""), "19700101-000000-unknown");
        assert_eq!(name(0, "../AB-c1"), "19700101-000000--c1");
        assert_eq!(created_at_ms(Path::new("19700101-000000")), None);
        assert_eq!(created_at_ms(Path::new("crash-1.json")), None);
    }
}

#[cfg(test)]
mod session_selection_tests {
    use super::*;

    #[test]
    fn a_report_uses_its_own_archived_client_session() {
        let dir = tempfile::tempdir().unwrap();
        let client = dir.path().join("client.log");
        let archived = dir.path().join("client-20260101-000000.log");
        fs::write(
            &client,
            "CLIENT_SESSION_START session=second timestamp_ms=20\nother client\n",
        )
        .unwrap();
        fs::write(
            &archived,
            "CLIENT_SESSION_START session=first timestamp_ms=10\nreporting client\n",
        )
        .unwrap();
        let logs = Logs::read(&LogSources {
            session_id: Some("first"),
            client: &client,
            core: &dir.path().join("core.log"),
            started_at_ms: 10,
        });
        assert_eq!(logs.client, fs::read(&archived).unwrap());
    }
}
