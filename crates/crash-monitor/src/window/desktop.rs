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

/// A clipboard tool and its arguments; it reads the new contents from standard input.
#[cfg(not(windows))]
type Tool = (&'static str, &'static [&'static str]);

#[cfg(not(windows))]
const PBCOPY: Tool = ("pbcopy", &[]);
#[cfg(not(windows))]
const WL_COPY: Tool = ("wl-copy", &[]);
#[cfg(not(windows))]
const XCLIP: Tool = ("xclip", &["-selection", "clipboard"]);
#[cfg(not(windows))]
const XSEL: Tool = ("xsel", &["--clipboard", "--input"]);

/// Replaces the clipboard's contents with `text` through the first desktop clipboard tool that
/// accepts it; the error names each tool's failure.
#[cfg(not(windows))]
pub(super) fn copy_text(text: &str) -> Result<(), String> {
    let mut failures = Vec::new();
    for (program, args) in tools(std::env::var_os("WAYLAND_DISPLAY").is_some()) {
        match pipe_to(program, args, text) {
            Ok(()) => return Ok(()),
            Err(error) => failures.push(format!("{program}: {error}")),
        }
    }
    Err(failures.join("; "))
}

/// The clipboard tools to try, in order: `pbcopy` on macOS; elsewhere `wl-copy` under Wayland,
/// then `xclip`, then `xsel`.
#[cfg(not(windows))]
fn tools(wayland: bool) -> Vec<Tool> {
    if cfg!(target_os = "macos") {
        return vec![PBCOPY];
    }
    let mut tools = Vec::new();
    if wayland {
        tools.push(WL_COPY);
    }
    tools.extend([XCLIP, XSEL]);
    tools
}

/// Runs `program` with `text` on its standard input and its output discarded; an error unless it
/// starts, takes all of `text` and exits successfully.
#[cfg(not(windows))]
fn pipe_to(program: &str, args: &[&str], text: &str) -> Result<(), String> {
    use std::{
        io::Write,
        process::{Command, Stdio},
    };

    let child = launcher_host::lifecycle::children::spawn(
        Command::new(program)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null()),
    )
    .map_err(|error| error.to_string())?;
    // Dropping standard input after the write is the end of the text the tool waits for.
    let written = child
        .take_stdin()
        .ok_or_else(|| "no standard input".to_owned())
        .and_then(|mut stdin| {
            stdin
                .write_all(text.as_bytes())
                .map_err(|error| error.to_string())
        });
    let status = child.wait().ok_or_else(|| "lost track of it".to_owned())?;
    written?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("exited with {status}"))
    }
}

#[cfg(all(test, not(windows)))]
mod tests {
    use super::*;

    #[test]
    fn clipboard_tools_follow_the_desktop() {
        if cfg!(target_os = "macos") {
            assert_eq!(tools(true), [PBCOPY]);
            assert_eq!(tools(false), [PBCOPY]);
        } else {
            assert_eq!(tools(true), [WL_COPY, XCLIP, XSEL]);
            assert_eq!(tools(false), [XCLIP, XSEL]);
        }
    }
}
