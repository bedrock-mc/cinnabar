//! Platform PNG chooser, called only by the skin worker.

use std::path::PathBuf;
#[cfg(not(windows))]
use std::process::Command;

pub(super) fn pick(cape: bool) -> Result<Option<PathBuf>, String> {
    let title = if cape {
        "Import cape PNG"
    } else {
        "Import skin PNG"
    };
    #[cfg(windows)]
    return crate::desktop::windows::pick_file(title, "PNG images\0*.png\0\0")
        .map_err(|error| format!("The PNG chooser could not open ({error:#x})."));
    #[cfg(target_os = "linux")]
    if let Ok(path) = crate::desktop::dbus::pick_file(title, "PNG images", &["*.png".to_owned()]) {
        return Ok(path);
    }
    #[cfg(not(windows))]
    {
        let commands = if cfg!(target_os = "macos") {
            let mut command = Command::new("osascript");
            let script = format!(
                "POSIX path of (choose file with prompt \"{title}\" of type {{\"public.png\"}})"
            );
            command.args(["-e", &script]);
            vec![command]
        } else {
            let mut zenity = Command::new("zenity");
            zenity.args([
                "--file-selection",
                &format!("--title={title}"),
                "--file-filter=PNG images | *.png",
            ]);
            let mut kdialog = Command::new("kdialog");
            kdialog.args([
                "--getopenfilename",
                ".",
                "*.png|PNG images",
                "--title",
                title,
            ]);
            vec![zenity, kdialog]
        };
        for mut command in commands {
            match command.output() {
                Ok(output) if output.status.success() => {
                    let path = String::from_utf8_lossy(&output.stdout).trim().to_owned();
                    return Ok((!path.is_empty()).then(|| PathBuf::from(path)));
                }
                Ok(_) => return Ok(None),
                Err(_) => {}
            }
        }
        Err("The PNG chooser is unavailable.".to_owned())
    }
}
