//! The import button opens the platform file picker on the import worker.
use resource_pack::PACK_IMPORT_EXTENSIONS;
use std::{path::PathBuf, process::Command};

/// Returns a selected filename, cancellation, or a visible picker error.
pub(super) fn pick() -> Result<Option<PathBuf>, String> {
    let mut command = command_for(std::env::consts::OS);
    let output = command.output().map_err(|error| {
        format!("File picker unavailable: {error}. Drop a pack onto the window to import it.")
    })?;
    if !output.status.success() {
        return Ok(None);
    }
    let path = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    Ok((!path.is_empty()).then(|| PathBuf::from(path)))
}

/// Builds the native picker command separately so its file restrictions can be checked.
fn command_for(platform: &str) -> Command {
    let patterns = PACK_IMPORT_EXTENSIONS.map(|extension| format!("*.{extension}"));
    let mut command;
    if platform == "macos" {
        let types = PACK_IMPORT_EXTENSIONS
            .map(|extension| format!("\"{extension}\""))
            .join(", ");
        let script = format!(
            "POSIX path of (choose file with prompt \"Import resource pack\" of type {{{types}}})"
        );
        command = Command::new("osascript");
        command.args(["-e", &script]);
    } else if platform == "windows" {
        let filter = patterns.join(";");
        let script = format!(
            "Add-Type -AssemblyName System.Windows.Forms; $d = New-Object System.Windows.Forms.OpenFileDialog; $d.Filter = 'Resource packs|{filter}'; if ($d.ShowDialog() -eq 'OK') {{ [Console]::WriteLine($d.FileName) }}"
        );
        command = Command::new("powershell");
        command.args(["-NoProfile", "-STA", "-Command", &script]);
    } else {
        let filter = format!("--file-filter=Resource packs | {}", patterns.join(" "));
        command = Command::new("zenity");
        command.args(["--file-selection", "--title=Import resource pack", &filter]);
    }
    command
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn macos_picker_filters_pack_archives() {
        let command = command_for("macos");
        let args = command
            .get_args()
            .map(|arg| arg.to_str().unwrap())
            .collect::<Vec<_>>();
        let script = args[1];
        let (_, types) = script
            .split_once("of type {")
            .expect("native file type filter");
        let (types, _) = types.split_once('}').expect("file type list");
        for extension in PACK_IMPORT_EXTENSIONS {
            assert!(
                types
                    .split(", ")
                    .any(|kind| kind == format!("\"{extension}\""))
            );
        }
        assert_eq!(types.split(", ").count(), PACK_IMPORT_EXTENSIONS.len());
    }
}
