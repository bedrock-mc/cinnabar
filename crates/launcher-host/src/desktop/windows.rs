//! Win32 calls behind the client's OS integration, replacing PowerShell and `cmd` spawns.

use std::{ffi::OsString, os::windows::ffi::OsStringExt, path::PathBuf, ptr};

use windows_sys::Win32::{
    Foundation::{S_FALSE, S_OK},
    System::{
        Com::{COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx, CoUninitialize},
        SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX},
    },
    UI::{
        Controls::Dialogs::{
            CommDlgExtendedError, GetOpenFileNameW, OFN_EXPLORER, OFN_FILEMUSTEXIST,
            OFN_NOCHANGEDIR, OFN_PATHMUSTEXIST, OPENFILENAMEW,
        },
        Shell::ShellExecuteW,
        WindowsAndMessaging::{
            IDOK, MB_ICONERROR, MB_ICONINFORMATION, MB_OK, MB_OKCANCEL, MB_SETFOREGROUND,
            MB_TASKMODAL, MessageBoxW, SW_SHOWNORMAL,
        },
    },
};

/// Room for a long (`\\?\`) path; the dialog fails rather than truncating.
const PATH_CAPACITY: usize = 32_768;

/// Total physical RAM in bytes.
pub fn physical_memory() -> Option<u64> {
    let mut status = MEMORYSTATUSEX {
        dwLength: size_of::<MEMORYSTATUSEX>() as u32,
        ..Default::default()
    };
    // SAFETY: `status` is a writable MEMORYSTATUSEX whose length field is set as required.
    (unsafe { GlobalMemoryStatusEx(&mut status) } != 0).then_some(status.ullTotalPhys)
}

/// Shows the Open dialog; `Ok(None)` on cancel, `Err` with the dialog error code on failure.
/// `filter` is a double-NUL-terminated list of `name\0patterns\0` pairs.
pub fn pick_file(title: &str, filter: &str) -> Result<Option<PathBuf>, u32> {
    let _apartment = ComApartment::enter();
    let filter = filter.encode_utf16().collect::<Vec<_>>();
    let title = wide(title);
    let mut file = vec![0u16; PATH_CAPACITY];
    let mut dialog = OPENFILENAMEW {
        lStructSize: size_of::<OPENFILENAMEW>() as u32,
        lpstrFilter: filter.as_ptr(),
        nFilterIndex: 1,
        lpstrFile: file.as_mut_ptr(),
        nMaxFile: file.len() as u32,
        lpstrTitle: title.as_ptr(),
        Flags: OFN_EXPLORER | OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR,
        ..Default::default()
    };
    // SAFETY: every pointer in `dialog` refers to a live, NUL-terminated buffer owned above.
    if unsafe { GetOpenFileNameW(&mut dialog) } != 0 {
        let length = file
            .iter()
            .position(|&unit| unit == 0)
            .unwrap_or(file.len());
        return Ok(Some(PathBuf::from(OsString::from_wide(&file[..length]))));
    }
    // SAFETY: reads the calling thread's last common-dialog error.
    match unsafe { CommDlgExtendedError() } {
        0 => Ok(None),
        code => Err(code),
    }
}

#[derive(Clone, Copy)]
pub enum MessageKind {
    Confirm,
    Alert,
}

/// Shows a blocking message box; `None` when no box could be shown.
pub fn message_box(kind: MessageKind, title: &str, body: &str) -> Option<bool> {
    let style = match kind {
        MessageKind::Confirm => MB_OKCANCEL | MB_ICONINFORMATION,
        MessageKind::Alert => MB_OK | MB_ICONERROR,
    } | MB_SETFOREGROUND
        | MB_TASKMODAL;
    let (title, body) = (wide(title), wide(body));
    // SAFETY: both strings are NUL-terminated and outlive the call; no owner window.
    let result = unsafe { MessageBoxW(ptr::null_mut(), body.as_ptr(), title.as_ptr(), style) };
    (result != 0).then_some(result == IDOK)
}

/// Opens `target` with its registered handler, as `cmd /C start` would without the shell parsing.
pub fn shell_open(target: &str) -> bool {
    let _apartment = ComApartment::enter();
    let (verb, target) = (wide("open"), wide(target));
    // SAFETY: both strings are NUL-terminated and outlive the call; null means defaults.
    let result = unsafe {
        ShellExecuteW(
            ptr::null_mut(),
            verb.as_ptr(),
            target.as_ptr(),
            ptr::null(),
            ptr::null(),
            SW_SHOWNORMAL,
        )
    };
    // Values above 32 mean success; lower ones are error codes.
    result as usize > 32
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Shell dialogs and handlers need a single-threaded COM apartment on the calling thread.
struct ComApartment {
    entered: bool,
}

impl ComApartment {
    fn enter() -> Self {
        // SAFETY: balanced by `drop` only when this call succeeded; an existing apartment of
        // another kind is left untouched.
        let result = unsafe {
            CoInitializeEx(
                ptr::null(),
                (COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) as u32,
            )
        };
        Self {
            entered: result == S_OK || result == S_FALSE,
        }
    }
}

impl Drop for ComApartment {
    fn drop(&mut self) {
        if self.entered {
            // SAFETY: matches the successful CoInitializeEx in `enter` on this thread.
            unsafe { CoUninitialize() };
        }
    }
}
