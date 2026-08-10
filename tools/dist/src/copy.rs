use std::{
    fs,
    io::{Read, Write},
    path::Path,
};

use same_file::Handle;

use crate::{DistError, MAX_FILE_BYTES, MAX_TOTAL_BYTES, Platform, io_error, validate_input};

pub(crate) fn copy_validated(
    source: &Path,
    destination: &Path,
    executable: bool,
    total_bytes: &mut u64,
) -> Result<(), DistError> {
    validate_input(source)?;
    let mut input = fs::File::open(source).map_err(|error| io_error(source, error))?;
    let opened = input.metadata().map_err(|error| io_error(source, error))?;
    if !opened.is_file() {
        return Err(DistError::NotRegular(source.to_owned()));
    }
    validate_input(source)?;
    let opened_identity =
        Handle::from_file(input.try_clone().map_err(|error| io_error(source, error))?)
            .map_err(|error| io_error(source, error))?;
    let current_identity = Handle::from_path(source).map_err(|error| io_error(source, error))?;
    if opened_identity != current_identity {
        return Err(DistError::InputChanged(source.to_owned()));
    }
    let mut output = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)
        .map_err(|error| io_error(destination, error))?;
    let mut file_bytes = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = input
            .read(&mut buffer)
            .map_err(|error| io_error(source, error))?;
        if read == 0 {
            break;
        }
        let read = read as u64;
        file_bytes = file_bytes
            .checked_add(read)
            .ok_or_else(|| DistError::FileTooLarge {
                path: source.to_owned(),
                limit: MAX_FILE_BYTES,
            })?;
        if file_bytes > MAX_FILE_BYTES {
            return Err(DistError::FileTooLarge {
                path: source.to_owned(),
                limit: MAX_FILE_BYTES,
            });
        }
        *total_bytes = total_bytes
            .checked_add(read)
            .ok_or(DistError::TotalTooLarge(MAX_TOTAL_BYTES))?;
        if *total_bytes > MAX_TOTAL_BYTES {
            return Err(DistError::TotalTooLarge(MAX_TOTAL_BYTES));
        }
        output
            .write_all(&buffer[..read as usize])
            .map_err(|error| io_error(destination, error))?;
    }
    output
        .flush()
        .map_err(|error| io_error(destination, error))?;
    set_executable_permissions(destination, executable)
}

pub(crate) fn executable_destination(platform: Platform, destination: &str) -> bool {
    match platform {
        Platform::Windows => false,
        Platform::Linux => matches!(destination, "bin/bedrock-client" | "bin/bedrock-core"),
        Platform::Macos => matches!(
            destination,
            "Cinnabar.app/Contents/MacOS/bedrock-client"
                | "Cinnabar.app/Contents/MacOS/bedrock-core"
        ),
    }
}

#[cfg(unix)]
fn set_executable_permissions(path: &Path, executable: bool) -> Result<(), DistError> {
    use std::os::unix::fs::PermissionsExt;
    if executable {
        fs::set_permissions(path, fs::Permissions::from_mode(0o755))
            .map_err(|source| io_error(path, source))?;
    }
    Ok(())
}

#[cfg(windows)]
fn set_executable_permissions(_path: &Path, _executable: bool) -> Result<(), DistError> {
    Ok(())
}
