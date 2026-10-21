//! Windows Installer owns update writes, permission consent and rollback.
use anyhow::{Result, ensure};
use std::{fs, path::Path, process::Command};

/// Opens a verified, write-locked MSI and runs the system installer with no elevation verb.
pub(super) fn install(
    artifact: &Path,
    work: &Path,
    target: &Path,
    expected: &update_manifest::Artifact,
) -> Result<()> {
    use std::os::windows::fs::OpenOptionsExt;
    ensure!(
        target
            .join(update_manifest::runtime::executable("CLIENT", "windows"))
            .is_file(),
        "target is not an installed client"
    );
    let msi = work.join("update.msi");
    fs::copy(artifact, &msi)?;
    let _locked = fs::OpenOptions::new().read(true).share_mode(1).open(&msi)?;
    crate::stage::verify_file(&msi, expected)?;
    let mut directory = [0u16; 32768];
    // SAFETY: the API receives a valid writable buffer and its length.
    let count = unsafe {
        windows_sys::Win32::System::SystemInformation::GetSystemDirectoryW(
            directory.as_mut_ptr(),
            directory.len() as u32,
        )
    } as usize;
    ensure!(
        count > 0 && count < directory.len(),
        "cannot locate Windows Installer"
    );
    use std::{ffi::OsString, os::windows::ffi::OsStringExt};
    let installer =
        std::path::PathBuf::from(OsString::from_wide(&directory[..count])).join("msiexec.exe");
    use std::os::windows::process::CommandExt;
    let mut command = Command::new(installer);
    for argument in super::msiexec_arguments(&msi, target) {
        command.raw_arg(argument);
    }
    let status = command.status()?;
    ensure!(
        matches!(status.code(), Some(0 | 3010)),
        "Windows Installer failed: {status}"
    );
    Ok(())
}
