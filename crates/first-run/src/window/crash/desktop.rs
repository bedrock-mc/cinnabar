//! Desktop hand-offs for the crash window: the report folder and the clipboard.

use std::path::Path;

/// Opens `folder` in the desktop's file manager on a worker thread.
pub(super) fn open_folder(folder: &Path) {
    launcher_host::desktop::open_url(&folder.display().to_string());
}

/// Replaces the clipboard's contents with `text`.
#[cfg(windows)]
pub(super) fn copy_text(text: &str) -> Result<(), String> {
    use windows_sys::Win32::{
        Foundation::GlobalFree,
        System::{
            DataExchange::{CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData},
            Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock},
            Ole::CF_UNICODETEXT,
        },
    };

    let wide: Vec<u16> = text.encode_utf16().chain(Some(0)).collect();
    // SAFETY: the clipboard is opened for this thread and closed on every path; the global block
    // is sized for `wide`, written only while locked, and freed unless the clipboard took it.
    unsafe {
        if OpenClipboard(std::ptr::null_mut()) == 0 {
            return Err("the clipboard is in use".into());
        }
        let result = (|| {
            if EmptyClipboard() == 0 {
                return Err("the clipboard could not be cleared".to_owned());
            }
            let block = GlobalAlloc(GMEM_MOVEABLE, wide.len() * size_of::<u16>());
            if block.is_null() {
                return Err("out of memory".into());
            }
            let target = GlobalLock(block).cast::<u16>();
            if target.is_null() {
                GlobalFree(block);
                return Err("the clipboard block could not be locked".into());
            }
            std::ptr::copy_nonoverlapping(wide.as_ptr(), target, wide.len());
            GlobalUnlock(block);
            if SetClipboardData(u32::from(CF_UNICODETEXT), block).is_null() {
                GlobalFree(block);
                return Err("the clipboard refused the text".into());
            }
            Ok(())
        })();
        CloseClipboard();
        result
    }
}

/// Clipboard access is only wired up on Windows so far.
#[cfg(not(windows))]
pub(super) fn copy_text(_text: &str) -> Result<(), String> {
    Err("no clipboard on this platform yet".into())
}
