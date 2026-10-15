//! The crash dialog: a task dialog with the crash window's actions, else a message box.
//!
//! Task dialogs live in Common Controls 6, which an executable without a manifest only reaches
//! through an activation context; the function is looked up at run time so the binary never
//! imports it from the older library the loader would bind.

use std::ptr;

use windows_sys::{
    Win32::{
        Foundation::{HANDLE, INVALID_HANDLE_VALUE},
        System::{
            ApplicationInstallationAndServicing::{
                ACTCTXW, ActivateActCtx, CreateActCtxW, DeactivateActCtx, ReleaseActCtx,
            },
            LibraryLoader::{GetProcAddress, LoadLibraryW},
        },
        UI::{
            Controls::{
                TASKDIALOG_BUTTON, TASKDIALOGCONFIG, TD_ERROR_ICON, TDF_ALLOW_DIALOG_CANCELLATION,
                TDF_SIZE_TO_CONTENT,
            },
            WindowsAndMessaging::{
                IDCANCEL, IDNO, IDYES, MB_ICONERROR, MB_SETFOREGROUND, MB_TASKMODAL,
                MB_YESNOCANCEL, MessageBoxW,
            },
        },
    },
    core::{BOOL, HRESULT},
};

use super::wide;
use crate::native_dialog::CrashChoice;

/// Requests Common Controls 6 for the activation context.
const COMMON_CONTROLS_MANIFEST: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <dependency>
    <dependentAssembly>
      <assemblyIdentity type="win32" name="Microsoft.Windows.Common-Controls" version="6.0.0.0" processorArchitecture="*" publicKeyToken="6595b64144ccf1df" language="*"/>
    </dependentAssembly>
  </dependency>
</assembly>
"#;
/// Task dialog button ids, in display order.
const BUTTONS: [(i32, &str, CrashChoice); 4] = [
    (100, "Restart", CrashChoice::Restart),
    (101, "Open folder", CrashChoice::OpenFolder),
    (102, "Copy report", CrashChoice::CopyReport),
    (103, "Close", CrashChoice::Close),
];

type TaskDialogIndirect =
    unsafe extern "system" fn(*const TASKDIALOGCONFIG, *mut i32, *mut i32, *mut BOOL) -> HRESULT;

/// Shows the crash dialog and waits for the player's choice.
pub(crate) fn crash_dialog(title: &str, body: &str) -> CrashChoice {
    task_dialog(title, body).unwrap_or_else(|| message_box(title, body))
}

fn task_dialog(title: &str, body: &str) -> Option<CrashChoice> {
    let _context = CommonControls::activate()?;
    let library = wide("comctl32.dll");
    // SAFETY: The name is NUL-terminated; the activation context selects version 6.
    let module = unsafe { LoadLibraryW(library.as_ptr()) };
    if module.is_null() {
        return None;
    }
    // SAFETY: The symbol name is NUL-terminated ASCII.
    let function = unsafe { GetProcAddress(module, c"TaskDialogIndirect".as_ptr().cast()) }?;
    // SAFETY: TaskDialogIndirect has exactly this signature in Common Controls 6.
    let function: TaskDialogIndirect = unsafe { std::mem::transmute(function) };
    let labels: Vec<Vec<u16>> = BUTTONS.iter().map(|(_, label, _)| wide(label)).collect();
    let buttons: Vec<TASKDIALOG_BUTTON> = BUTTONS
        .iter()
        .zip(&labels)
        .map(|((id, _, _), label)| TASKDIALOG_BUTTON {
            nButtonID: *id,
            pszButtonText: label.as_ptr(),
        })
        .collect();
    let (title, body) = (wide(title), wide(body));
    let mut config = TASKDIALOGCONFIG {
        cbSize: size_of::<TASKDIALOGCONFIG>() as u32,
        dwFlags: TDF_ALLOW_DIALOG_CANCELLATION | TDF_SIZE_TO_CONTENT,
        pszWindowTitle: title.as_ptr(),
        pszMainInstruction: title.as_ptr(),
        pszContent: body.as_ptr(),
        cButtons: buttons.len() as u32,
        pButtons: buttons.as_ptr(),
        nDefaultButton: BUTTONS[0].0,
        ..TASKDIALOGCONFIG::default()
    };
    config.Anonymous1.pszMainIcon = TD_ERROR_ICON;
    let mut pressed = 0;
    // SAFETY: Every string and the button array outlive the modal call; outputs are owned.
    let result = unsafe { function(&config, &mut pressed, ptr::null_mut(), ptr::null_mut()) };
    if result < 0 {
        return None;
    }
    Some(
        BUTTONS
            .iter()
            .find(|(id, _, _)| *id == pressed)
            .map_or(CrashChoice::Close, |(_, _, choice)| *choice),
    )
}

/// The same choices without a task dialog: Yes restarts, No opens the folder.
fn message_box(title: &str, body: &str) -> CrashChoice {
    let body = format!(
        "{body}\n\nSelect Yes to restart, No to open the report folder or Cancel to close."
    );
    let (title, body) = (wide(title), wide(&body));
    // SAFETY: Both strings are NUL-terminated and outlive the call; no owner window.
    let result = unsafe {
        MessageBoxW(
            ptr::null_mut(),
            body.as_ptr(),
            title.as_ptr(),
            MB_YESNOCANCEL | MB_ICONERROR | MB_SETFOREGROUND | MB_TASKMODAL,
        )
    };
    match result {
        0 => CrashChoice::Unavailable,
        IDYES => CrashChoice::Restart,
        IDNO => CrashChoice::OpenFolder,
        IDCANCEL => CrashChoice::Close,
        _ => CrashChoice::Close,
    }
}

/// An active Common Controls 6 activation context; dropping it deactivates and releases it.
struct CommonControls {
    context: HANDLE,
    cookie: usize,
}

impl CommonControls {
    fn activate() -> Option<Self> {
        let manifest = std::env::temp_dir().join(format!(
            "cinnabar-common-controls-{}.manifest",
            std::process::id()
        ));
        std::fs::write(&manifest, COMMON_CONTROLS_MANIFEST).ok()?;
        let source = wide(&manifest.to_string_lossy());
        let request = ACTCTXW {
            cbSize: size_of::<ACTCTXW>() as u32,
            lpSource: source.as_ptr(),
            ..ACTCTXW::default()
        };
        // SAFETY: The request and its source path are valid for the call.
        let context = unsafe { CreateActCtxW(&request) };
        let _ = std::fs::remove_file(&manifest);
        if context == INVALID_HANDLE_VALUE {
            return None;
        }
        let mut cookie = 0;
        // SAFETY: The context was just created; the cookie is owned storage.
        if unsafe { ActivateActCtx(context, &mut cookie) } == 0 {
            // SAFETY: Releases the context created above.
            unsafe { ReleaseActCtx(context) };
            return None;
        }
        Some(Self { context, cookie })
    }
}

impl Drop for CommonControls {
    fn drop(&mut self) {
        // SAFETY: Deactivates the cookie activated on this thread, then releases the context.
        unsafe {
            DeactivateActCtx(0, self.cookie);
            ReleaseActCtx(self.context);
        }
    }
}
