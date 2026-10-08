//! Tells Discord how to start Cinnabar when a friend accepts an invite while it is closed.
//!
//! The command takes no arguments: the joining address arrives over IPC once the game connects.

use std::{io, path::Path};

pub(crate) fn register(application_id: u64) -> io::Result<()> {
    // An AppImage runs from a temporary mount, so Discord must start the image itself.
    let command = match std::env::var_os("APPIMAGE") {
        Some(image) if cfg!(target_os = "linux") => image.into(),
        _ => std::env::current_exe()?,
    };
    platform::register(application_id, &command)
}

/// The URL scheme Discord opens to start an application's game.
fn scheme(application_id: u64) -> String {
    format!("discord-{application_id}")
}

/// A desktop entry handling the scheme. `Exec` quoting escapes `"`, `` ` ``, `$` and `\`, and
/// the file's string escaping then doubles every backslash.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn desktop_entry(application_id: u64, command: &Path) -> String {
    let mut quoted = String::from('"');
    for character in command.to_string_lossy().chars() {
        match character {
            '"' | '`' | '$' => quoted.push_str(r"\\"),
            '\\' => quoted.push_str(r"\\\"),
            // A bare `%` would start a field code.
            '%' => quoted.push('%'),
            _ => {}
        }
        quoted.push(character);
    }
    quoted.push('"');
    format!(
        "[Desktop Entry]\nName={}\nExec={quoted}\nType=Application\nNoDisplay=true\n\
         Categories=Discord;Games;\nMimeType=x-scheme-handler/{};\n",
        launcher::PRODUCT_NAME,
        scheme(application_id)
    )
}

/// The file Discord reads on macOS for a game's launch command.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn macos_game_file(command: &Path) -> String {
    serde_json::json!({ "command": command.to_string_lossy() }).to_string()
}

/// Writes `contents` unless the file already holds them; `true` when it changed.
#[cfg_attr(windows, allow(dead_code))]
fn write_if_changed(path: &Path, contents: &str) -> io::Result<bool> {
    if std::fs::read(path).is_ok_and(|existing| existing == contents.as_bytes()) {
        return Ok(false);
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, contents)?;
    Ok(true)
}

#[cfg(target_os = "linux")]
mod platform {
    use std::{io, path::Path, path::PathBuf, process::Command};

    pub(super) fn register(application_id: u64, command: &Path) -> io::Result<()> {
        let data = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::home_dir().map(|home| home.join(".local/share")))
            .ok_or_else(|| io::Error::other("no home directory"))?;
        let scheme = super::scheme(application_id);
        let file = format!("{scheme}.desktop");
        let entry = super::desktop_entry(application_id, command);
        if super::write_if_changed(&data.join("applications").join(&file), &entry)? {
            let status = Command::new("xdg-mime")
                .args(["default", &file, &format!("x-scheme-handler/{scheme}")])
                .status()?;
            if !status.success() {
                return Err(io::Error::other(format!("xdg-mime exited with {status}")));
            }
        }
        Ok(())
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use std::{io, path::Path};

    pub(super) fn register(application_id: u64, command: &Path) -> io::Result<()> {
        let home = std::env::home_dir().ok_or_else(|| io::Error::other("no home directory"))?;
        let file = home
            .join("Library/Application Support/discord/games")
            .join(format!("{application_id}.json"));
        super::write_if_changed(&file, &super::macos_game_file(command)).map(drop)
    }
}

#[cfg(windows)]
mod platform {
    use std::{io, path::Path, ptr};

    use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, REG_SZ, RegSetKeyValueW};

    pub(super) fn register(application_id: u64, command: &Path) -> io::Result<()> {
        let key = format!(r"Software\Classes\{}", super::scheme(application_id));
        let executable = command.to_string_lossy();
        set(
            &key,
            None,
            &format!("URL:Run game {application_id} protocol"),
        )?;
        set(&key, Some("URL Protocol"), "")?;
        set(&format!(r"{key}\DefaultIcon"), None, &executable)?;
        set(
            &format!(r"{key}\shell\open\command"),
            None,
            &format!("\"{executable}\""),
        )
    }

    /// Sets a per-user string value, creating its key; `None` names the default value.
    fn set(key: &str, name: Option<&str>, value: &str) -> io::Result<()> {
        let key = wide(key);
        let name = name.map(wide);
        let value = wide(value);
        let bytes = u32::try_from(value.len() * size_of::<u16>())
            .map_err(|_| io::Error::other("registry value too long"))?;
        // SAFETY: every pointer is a NUL-terminated UTF-16 buffer that outlives the call, and
        // `bytes` is the data buffer's length including its terminator.
        let status = unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                name.as_ref().map_or(ptr::null(), |name| name.as_ptr()),
                REG_SZ,
                value.as_ptr().cast(),
                bytes,
            )
        };
        if status == 0 {
            Ok(())
        } else {
            Err(io::Error::from_raw_os_error(status as i32))
        }
    }

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain([0]).collect()
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
mod platform {
    use std::{io, path::Path};

    pub(super) fn register(_: u64, _: &Path) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_entry_quotes_the_command_and_claims_the_discord_scheme() {
        let entry = desktop_entry(42, Path::new(r#"/opt/My "Games"/$bin\100%"#));
        assert!(
            entry.contains(r#"Exec="/opt/My \\"Games\\"/\\$bin\\\\100%%""#),
            "{entry}"
        );
        assert!(entry.contains("MimeType=x-scheme-handler/discord-42;\n"));
    }

    #[test]
    fn macos_game_file_names_the_command() {
        let file: serde_json::Value =
            serde_json::from_str(&macos_game_file(Path::new("/Applications/C.app/x"))).unwrap();
        assert_eq!(file["command"], "/Applications/C.app/x");
    }
}
