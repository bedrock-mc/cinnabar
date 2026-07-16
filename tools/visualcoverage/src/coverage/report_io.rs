use super::*;

pub fn parse_baseline(bytes: &[u8]) -> Result<Baseline, CoverageError> {
    if bytes.len() > MAX_BASELINE_BYTES {
        return Err(CoverageError::BaselineTooLarge);
    }
    Ok(serde_json::from_slice(bytes)?)
}

pub fn deterministic_json<T: Serialize>(value: &T) -> Result<Vec<u8>, CoverageError> {
    let mut bytes = serde_json::to_vec_pretty(value)?;
    bytes.push(b'\n');
    Ok(bytes)
}

static ATOMIC_REPORT_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Serializes deterministic JSON completely before atomically replacing the
/// destination through a unique same-directory temporary file.
pub fn write_deterministic_json_atomic<T: Serialize>(
    path: &Path,
    value: &T,
) -> Result<(), CoverageError> {
    let bytes = deterministic_json(value)?;
    write_report_atomic(path, &bytes).map_err(|source| CoverageError::ReportWrite {
        path: path.to_path_buf(),
        source,
    })
}

pub(super) fn write_report_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let file_name = path.file_name().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "report path has no file name")
    })?;
    let mut temporary = None;
    for _ in 0..128 {
        let sequence = ATOMIC_REPORT_COUNTER.fetch_add(1, Ordering::Relaxed);
        let candidate = parent.join(format!(
            ".{}.tmp-{}-{sequence}",
            file_name.to_string_lossy(),
            std::process::id(),
        ));
        match OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&candidate)
        {
            Ok(file) => {
                temporary = Some((candidate, file));
                break;
            }
            Err(source) if source.kind() == io::ErrorKind::AlreadyExists => {}
            Err(source) => return Err(source),
        }
    }
    let (temporary_path, mut file) = temporary.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::AlreadyExists,
            "could not reserve an atomic report temporary file",
        )
    })?;
    let write_result = file
        .write_all(bytes)
        .and_then(|()| file.flush())
        .and_then(|()| file.sync_all());
    drop(file);
    if let Err(source) = write_result {
        let _ = fs::remove_file(&temporary_path);
        return Err(source);
    }
    if let Err(source) = replace_report_atomic(&temporary_path, path) {
        let _ = fs::remove_file(&temporary_path);
        return Err(source);
    }
    Ok(())
}

#[cfg(windows)]
fn replace_report_atomic(temporary: &Path, destination: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;

    const MOVEFILE_REPLACE_EXISTING: u32 = 1;
    const MOVEFILE_WRITE_THROUGH: u32 = 8;
    #[link(name = "Kernel32")]
    unsafe extern "system" {
        fn MoveFileExW(
            existing_file_name: *const u16,
            new_file_name: *const u16,
            flags: u32,
        ) -> i32;
    }

    let temporary = temporary
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let destination = destination
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    // SAFETY: Both pointers reference live, NUL-terminated UTF-16 buffers for
    // the duration of the call, and the flags request a same-volume atomic
    // replacement with write-through durability.
    let replaced = unsafe {
        MoveFileExW(
            temporary.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if replaced == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
fn replace_report_atomic(temporary: &Path, destination: &Path) -> io::Result<()> {
    fs::rename(temporary, destination)
}
