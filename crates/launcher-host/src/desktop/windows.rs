//! Win32 calls behind the client's OS integration, replacing PowerShell and `cmd` spawns.

use std::{
    ffi::{OsStr, OsString},
    os::windows::ffi::OsStringExt,
    path::PathBuf,
    ptr,
};

use windows_sys::Win32::{
    Foundation::{ERROR_INVALID_PARAMETER, S_FALSE, S_OK},
    System::{
        Com::{COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx, CoUninitialize},
        SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX},
    },
    UI::{
        Controls::Dialogs::{
            CommDlgExtendedError, GetOpenFileNameW, GetSaveFileNameW, OFN_ALLOWMULTISELECT,
            OFN_EXPLORER, OFN_FILEMUSTEXIST, OFN_NOCHANGEDIR, OFN_OVERWRITEPROMPT,
            OFN_PATHMUSTEXIST, OPENFILENAMEW,
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
/// Room for a long directory path and hundreds of selected file names.
const MULTI_FILE_CAPACITY: usize = PATH_CAPACITY * 4;

pub(crate) mod crash_dialog;

/// Runs console helpers without a console window while preserving their configured streams.
pub fn hide_console(command: &mut std::process::Command) {
    use std::os::windows::process::CommandExt;
    use windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;

    command.creation_flags(CREATE_NO_WINDOW);
}

/// Whether this executable is linked for the GUI subsystem, so failures have no console to
/// reach and must be shown in a window.
pub fn gui_subsystem() -> bool {
    #[cfg(target_pointer_width = "32")]
    use windows_sys::Win32::System::Diagnostics::Debug::IMAGE_NT_HEADERS32 as Headers;
    #[cfg(target_pointer_width = "64")]
    use windows_sys::Win32::System::Diagnostics::Debug::IMAGE_NT_HEADERS64 as Headers;
    use windows_sys::Win32::System::{
        Diagnostics::Debug::IMAGE_SUBSYSTEM_WINDOWS_GUI, LibraryLoader::GetModuleHandleW,
        SystemServices::IMAGE_DOS_HEADER,
    };

    // SAFETY: A null name selects this executable's image, which stays mapped and starts with
    // valid DOS and NT headers.
    unsafe {
        let base = GetModuleHandleW(ptr::null()).cast::<u8>();
        if base.is_null() {
            return false;
        }
        let dos = base.cast::<IMAGE_DOS_HEADER>().read_unaligned();
        let headers = base
            .offset(dos.e_lfanew as isize)
            .cast::<Headers>()
            .read_unaligned();
        headers.OptionalHeader.Subsystem == IMAGE_SUBSYSTEM_WINDOWS_GUI
    }
}

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
/// Empty input disables filtering; malformed pairs return `ERROR_INVALID_PARAMETER`.
/// Shows the Save dialog; \Ok(None)\ on cancel, \Err\ with the dialog error code on failure.
pub fn pick_save_file(
    title: &str,
    default_name: &str,
    filter: &str,
) -> Result<Option<PathBuf>, u32> {
    let filter = super::windows_filter::encode(filter).map_err(|()| ERROR_INVALID_PARAMETER)?;
    let _apartment = ComApartment::enter();
    let title = wide(title);
    let mut file = vec![0u16; PATH_CAPACITY];
    let def_units = wide(default_name);
    let copy_len = def_units.len().min(file.len() - 1);
    file[..copy_len].copy_from_slice(&def_units[..copy_len]);
    let mut dialog = OPENFILENAMEW {
        lStructSize: size_of::<OPENFILENAMEW>() as u32,
        lpstrFilter: filter.as_ref().map_or(ptr::null(), |units| units.as_ptr()),
        nFilterIndex: 1,
        lpstrFile: file.as_mut_ptr(),
        nMaxFile: file.len() as u32,
        lpstrTitle: title.as_ptr(),
        Flags: OFN_EXPLORER | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR | OFN_OVERWRITEPROMPT,
        ..Default::default()
    };
    if unsafe { GetSaveFileNameW(&mut dialog) } != 0 {
        return Ok(selected_paths(&file).into_iter().next());
    }
    match unsafe { CommDlgExtendedError() } {
        0 => Ok(None),
        code => Err(code),
    }
}

pub fn pick_file(title: &str, filter: &str) -> Result<Option<PathBuf>, u32> {
    file_chooser(title, filter, false).map(|paths| paths.into_iter().next())
}

/// Shows the Open dialog with multiple selection; an empty list means cancellation.
/// `filter` has the same double-NUL-terminated format as `pick_file`.
/// Empty input disables filtering; malformed pairs return `ERROR_INVALID_PARAMETER`.
pub fn pick_files(title: &str, filter: &str) -> Result<Vec<PathBuf>, u32> {
    file_chooser(title, filter, true)
}

/// Shares the native dialog setup while preserving each caller's selection mode.
fn file_chooser(title: &str, filter: &str, multiple: bool) -> Result<Vec<PathBuf>, u32> {
    let filter = super::windows_filter::encode(filter).map_err(|()| ERROR_INVALID_PARAMETER)?;
    let _apartment = ComApartment::enter();
    let title = wide(title);
    let capacity = if multiple {
        MULTI_FILE_CAPACITY
    } else {
        PATH_CAPACITY
    };
    let mut file = vec![0u16; capacity];
    let mut dialog = OPENFILENAMEW {
        lStructSize: size_of::<OPENFILENAMEW>() as u32,
        lpstrFilter: filter.as_ref().map_or(ptr::null(), |units| units.as_ptr()),
        nFilterIndex: 1,
        lpstrFile: file.as_mut_ptr(),
        nMaxFile: file.len() as u32,
        lpstrTitle: title.as_ptr(),
        Flags: OFN_EXPLORER
            | OFN_FILEMUSTEXIST
            | OFN_PATHMUSTEXIST
            | OFN_NOCHANGEDIR
            | if multiple { OFN_ALLOWMULTISELECT } else { 0 },
        ..Default::default()
    };
    // SAFETY: every pointer in `dialog` refers to a live, NUL-terminated buffer owned above.
    if unsafe { GetOpenFileNameW(&mut dialog) } != 0 {
        return Ok(selected_paths(&file));
    }
    // SAFETY: reads the calling thread's last common-dialog error.
    match unsafe { CommDlgExtendedError() } {
        0 => Ok(Vec::new()),
        code => Err(code),
    }
}

/// Reads an Explorer-style result: one full path, or a directory followed by file names.
/// The first empty component ends the list; UTF-16 path data is preserved without loss.
fn selected_paths(buffer: &[u16]) -> Vec<PathBuf> {
    let mut parts = buffer
        .split(|&unit| unit == 0)
        .take_while(|part| !part.is_empty());
    let Some(first) = parts.next() else {
        return Vec::new();
    };
    let path = PathBuf::from(OsString::from_wide(first));
    let mut names = parts.peekable();
    if names.peek().is_none() {
        vec![path]
    } else {
        names
            .map(|name| path.join(OsString::from_wide(name)))
            .collect()
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
pub fn shell_open(target: impl AsRef<OsStr>) -> bool {
    use std::os::windows::ffi::OsStrExt;

    let _apartment = ComApartment::enter();
    let verb = wide("open");
    let target: Vec<u16> = target
        .as_ref()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_selection_is_already_a_full_path() {
        let path = r"C:\Resource Packs\My Pack.mcpack";
        assert_eq!(selected_paths(&wide(path)), [PathBuf::from(path)]);
    }

    #[test]
    fn multiple_selections_join_names_to_the_directory_and_stop_at_double_nul() {
        let buffer = "C:\\Resource Packs\0First Pack.mcpack\0第二包.mcaddon\0\0unused";
        assert_eq!(
            selected_paths(&buffer.encode_utf16().collect::<Vec<_>>()),
            [
                PathBuf::from(r"C:\Resource Packs\First Pack.mcpack"),
                PathBuf::from(r"C:\Resource Packs\第二包.mcaddon"),
            ]
        );
    }

    #[test]
    fn selections_preserve_unc_paths_and_unpaired_utf16_units() {
        use std::os::windows::ffi::OsStrExt;

        let mut buffer = "\\\\server\\packs\0".encode_utf16().collect::<Vec<_>>();
        let name = [0xd800, b'.' as u16, b'z' as u16, b'i' as u16, b'p' as u16];
        buffer.extend(name);
        buffer.extend([0, 0]);
        let expected = PathBuf::from(r"\\server\packs").join(OsString::from_wide(&name));
        let paths = selected_paths(&buffer);
        assert_eq!(paths.as_slice(), std::slice::from_ref(&expected));
        assert_eq!(
            paths[0].as_os_str().encode_wide().collect::<Vec<_>>(),
            expected.as_os_str().encode_wide().collect::<Vec<_>>()
        );
    }

    #[test]
    fn a_hundred_long_file_names_fit_the_multi_selection_buffer() {
        let directory = format!(r"C:\{}", "directory\\".repeat(1000));
        let names = (0..100)
            .map(|index| format!("{index:03}-{}.mcpack", "p".repeat(240)))
            .collect::<Vec<_>>();
        let mut buffer = directory.encode_utf16().collect::<Vec<_>>();
        buffer.push(0);
        for name in &names {
            buffer.extend(name.encode_utf16());
            buffer.push(0);
        }
        buffer.push(0);
        assert!(buffer.len() > PATH_CAPACITY);
        assert!(buffer.len() <= MULTI_FILE_CAPACITY);
        buffer.resize(MULTI_FILE_CAPACITY, 0);
        assert_eq!(
            selected_paths(&buffer),
            names
                .iter()
                .map(|name| PathBuf::from(&directory).join(name))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn empty_selection_has_no_paths() {
        assert!(selected_paths(&[]).is_empty());
        assert!(selected_paths(&[0, 0]).is_empty());
    }
}
