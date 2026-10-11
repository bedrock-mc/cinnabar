//! The import button opens the platform file picker on the import worker.
use resource_pack::PACK_IMPORT_EXTENSIONS;
use std::path::PathBuf;
#[cfg(any(not(windows), test))]
use std::process::Command;

const TITLE: &str = "Import resource pack";
const FILTER_NAME: &str = "Resource packs";
const UNAVAILABLE_HINT: &str = "Drop a pack onto the window to import it.";

/// Returns a selected filename, cancellation, or a visible picker error.
pub(super) fn pick() -> Result<Option<PathBuf>, String> {
    #[cfg(windows)]
    return launcher_host::desktop::windows::pick_file(TITLE, &windows_filter())
        .map_err(|code| format!("File picker failed (error {code:#x}). {UNAVAILABLE_HINT}"));
    #[cfg(target_os = "linux")]
    match launcher_host::desktop::dbus::pick_file(TITLE, FILTER_NAME, &patterns()) {
        Ok(choice) => return Ok(choice),
        Err(error) => {
            bevy::log::debug!("portal file chooser unavailable ({error}); trying dialog tools")
        }
    }
    #[cfg(not(windows))]
    run_tools(tool_commands(std::env::consts::OS))
}

fn patterns() -> Vec<String> {
    PACK_IMPORT_EXTENSIONS
        .iter()
        .map(|extension| format!("*.{extension}"))
        .collect()
}

/// The Open dialog's filter: `name\0pattern;pattern\0\0`.
#[cfg(any(windows, test))]
fn windows_filter() -> String {
    format!("{FILTER_NAME}\0{}\0\0", patterns().join(";"))
}

/// Runs the first picker tool that launches; a launched tool's failure counts as cancellation.
#[cfg(not(windows))]
fn run_tools(commands: Vec<Command>) -> Result<Option<PathBuf>, String> {
    let mut launch_error = None;
    for mut command in commands {
        match command.output() {
            Ok(output) if output.status.success() => {
                let path = String::from_utf8_lossy(&output.stdout).trim().to_owned();
                return Ok((!path.is_empty()).then(|| PathBuf::from(path)));
            }
            Ok(_) => return Ok(None),
            Err(error) => launch_error = Some(error),
        }
    }
    let reason = launch_error.map_or_else(|| "no picker".to_owned(), |error| error.to_string());
    Err(format!(
        "File picker unavailable: {reason}. {UNAVAILABLE_HINT}"
    ))
}

/// Picker tools in fallback order; Linux reaches these only when the portal is unavailable.
#[cfg(any(not(windows), test))]
fn tool_commands(platform: &str) -> Vec<Command> {
    if platform == "macos" {
        let types = PACK_IMPORT_EXTENSIONS
            .map(|extension| format!("\"{extension}\""))
            .join(", ");
        let script =
            format!("POSIX path of (choose file with prompt \"{TITLE}\" of type {{{types}}})");
        let mut command = Command::new("osascript");
        command.args(["-e", &script]);
        return vec![command];
    }
    let patterns = patterns().join(" ");
    let mut zenity = Command::new("zenity");
    zenity.args([
        "--file-selection".to_owned(),
        format!("--title={TITLE}"),
        format!("--file-filter={FILTER_NAME} | {patterns}"),
    ]);
    let mut kdialog = Command::new("kdialog");
    kdialog.args([
        "--title".to_owned(),
        TITLE.to_owned(),
        "--getopenfilename".to_owned(),
        ".".to_owned(),
        format!("{patterns}|{FILTER_NAME}"),
    ]);
    vec![zenity, kdialog]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(command: &Command) -> Vec<&str> {
        command
            .get_args()
            .map(|arg| arg.to_str().unwrap())
            .collect()
    }

    #[test]
    fn macos_picker_filters_pack_archives() {
        let commands = tool_commands("macos");
        let args = args(&commands[0]);
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

    #[test]
    fn linux_fallbacks_try_zenity_then_kdialog_with_the_pack_filter() {
        let commands = tool_commands("linux");
        let programs = commands
            .iter()
            .map(|command| command.get_program().to_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(programs, ["zenity", "kdialog"]);
        for command in &commands {
            let filter = *args(command).last().unwrap();
            for extension in PACK_IMPORT_EXTENSIONS {
                assert!(filter.contains(&format!("*.{extension}")), "{filter}");
            }
        }
    }

    #[test]
    fn windows_filter_lists_every_pack_pattern_and_ends_with_double_nul() {
        let filter = windows_filter();
        assert!(filter.ends_with("\0\0"));
        let mut parts = filter.trim_end_matches('\0').split('\0');
        assert_eq!(parts.next(), Some(FILTER_NAME));
        let patterns = parts.next().unwrap().split(';').collect::<Vec<_>>();
        assert_eq!(patterns.len(), PACK_IMPORT_EXTENSIONS.len());
        assert_eq!(parts.next(), None);
    }

    #[cfg(unix)]
    #[test]
    fn a_missing_tool_falls_through_and_a_cancelled_one_stops() {
        let shell = |script: &str| {
            let mut command = Command::new("sh");
            command.args(["-c", script]);
            command
        };
        let missing = || Command::new("cinnabar-no-such-picker");
        assert_eq!(
            run_tools(vec![missing(), shell("echo /packs/a.mcpack")]),
            Ok(Some(PathBuf::from("/packs/a.mcpack")))
        );
        assert_eq!(
            run_tools(vec![shell("exit 1"), shell("echo /packs/b.mcpack")]),
            Ok(None)
        );
        let error = run_tools(vec![missing()]).unwrap_err();
        assert!(error.contains(UNAVAILABLE_HINT), "{error}");
    }
}
