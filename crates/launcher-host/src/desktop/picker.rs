//! Import buttons open the platform file picker on their worker thread.
use std::path::PathBuf;
#[cfg(any(not(any(windows, target_os = "android")), test))]
use std::process::Command;

/// What one import dialog asks for.
pub struct Filter {
    pub title: &'static str,
    pub name: &'static str,
    pub extensions: &'static [&'static str],
    /// Appended to errors when no picker opens.
    pub unavailable_hint: &'static str,
    /// The largest file the importer reads, or `None` without a size limit. Android copies picked
    /// documents into app storage and refuses one that grows past this instead of filling storage.
    pub max_bytes: Option<u64>,
}

/// Returns one selected filename, cancellation, or a visible picker error.
pub fn pick(filter: &Filter) -> Result<Option<PathBuf>, String> {
    choose(filter, false).map(|paths| paths.into_iter().next())
}

/// Returns all selected files, an empty list on cancellation, or a visible picker error.
pub fn pick_files(filter: &Filter) -> Result<Vec<PathBuf>, String> {
    choose(filter, true)
}

/// Shares filters and fallback tools while preserving each caller's selection mode.
// Windows and Android compile only their own branch, so its early return is the tail.
#[cfg_attr(any(windows, target_os = "android"), allow(clippy::needless_return))]
fn choose(filter: &Filter, multiple: bool) -> Result<Vec<PathBuf>, String> {
    #[cfg(windows)]
    {
        let result = if multiple {
            super::windows::pick_files(filter.title, &windows_filter(filter))
        } else {
            super::windows::pick_file(filter.title, &windows_filter(filter))
                .map(|path| path.into_iter().collect())
        };
        result.map_err(|code| {
            format!(
                "File picker failed (error {code:#x}). {}",
                filter.unavailable_hint
            )
        })
    }
    #[cfg(target_os = "linux")]
    {
        let result = if multiple {
            super::dbus::pick_files(filter.title, filter.name, &patterns(filter))
        } else {
            super::dbus::pick_file(filter.title, filter.name, &patterns(filter))
                .map(|path| path.into_iter().collect())
        };
        match result {
            Ok(paths) => return Ok(paths),
            Err(error) => {
                tracing::debug!("portal file chooser unavailable ({error}); trying dialog tools")
            }
        }
    }
    // Android has no picker tools; the Storage Access Framework copies picks into app storage.
    #[cfg(target_os = "android")]
    return super::android::pick_documents(multiple, filter.max_bytes)
        .map_err(|error| format!("{error}."))?
        .ok_or_else(|| unavailable("no document picker is available", filter))
        .and_then(|paths| selected(filter, paths));
    #[cfg(not(any(windows, target_os = "android")))]
    selected(
        filter,
        run_tools(
            tool_commands(std::env::consts::OS, filter, multiple),
            filter,
        )?,
    )
}

/// Keeps picked files with an accepted extension; picking only other files is a visible error.
#[cfg(any(not(windows), test))]
fn selected(filter: &Filter, paths: Vec<PathBuf>) -> Result<Vec<PathBuf>, String> {
    let picked = !paths.is_empty();
    let accepted: Vec<PathBuf> = paths
        .into_iter()
        .filter(|path| {
            path.extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| {
                    filter
                        .extensions
                        .iter()
                        .any(|accepted| extension.eq_ignore_ascii_case(accepted))
                })
        })
        .collect();
    if picked && accepted.is_empty() {
        let kinds = filter
            .extensions
            .iter()
            .map(|extension| format!(".{extension}"))
            .collect::<Vec<_>>()
            .join(", ");
        return Err(format!("Choose a {kinds} file."));
    }
    Ok(accepted)
}

/// Derives every platform's filter from the selected import extensions.
#[cfg(any(not(target_os = "android"), test))]
fn patterns(filter: &Filter) -> Vec<String> {
    filter
        .extensions
        .iter()
        .map(|extension| format!("*.{extension}"))
        .collect()
}

/// The Open dialog's filter: `name\0pattern;pattern\0\0`.
#[cfg(any(windows, test))]
fn windows_filter(filter: &Filter) -> String {
    format!("{}\0{}\0\0", filter.name, patterns(filter).join(";"))
}

/// Runs the first picker tool that launches; a launched tool's failure counts as cancellation.
#[cfg(not(any(windows, target_os = "android")))]
fn run_tools(commands: Vec<Command>, filter: &Filter) -> Result<Vec<PathBuf>, String> {
    let mut launch_error = None;
    for mut command in commands {
        match command.output() {
            Ok(output) if output.status.success() => {
                return Ok(output_paths(&output.stdout));
            }
            Ok(_) => return Ok(Vec::new()),
            Err(error) => launch_error = Some(error),
        }
    }
    let reason = launch_error.map_or_else(|| "no picker".to_owned(), |error| error.to_string());
    Err(unavailable(&reason, filter))
}

/// The visible error when no picker could open, followed by the caller's alternative.
#[cfg(not(windows))]
fn unavailable(reason: &str, filter: &Filter) -> String {
    format!(
        "File picker unavailable: {reason}. {}",
        filter.unavailable_hint
    )
}

/// Reads one selected path per line, preserving spaces and ignoring empty output lines.
#[cfg(any(not(any(windows, target_os = "android")), test))]
fn output_paths(output: &[u8]) -> Vec<PathBuf> {
    String::from_utf8_lossy(output)
        .lines()
        .filter(|line| !line.is_empty())
        .map(PathBuf::from)
        .collect()
}

/// Picker tools in fallback order; Linux reaches these only when the portal is unavailable.
#[cfg(any(not(any(windows, target_os = "android")), test))]
fn tool_commands(platform: &str, filter: &Filter, multiple: bool) -> Vec<Command> {
    let (title, name) = (filter.title, filter.name);
    if platform == "macos" {
        // `of type` matches only file types Launch Services knows, so `.wasm` and (without
        // Minecraft installed) `.mcpack` would be greyed out; `selected` checks extensions instead.
        let script = if multiple {
            format!(
                "set selectedFiles to choose file with prompt \"{title}\" with multiple selections allowed\n\
             set selectedPaths to \"\"\n\
             repeat with selectedFile in selectedFiles\n\
             set selectedPaths to selectedPaths & (POSIX path of selectedFile) & linefeed\n\
             end repeat\n\
             return selectedPaths"
            )
        } else {
            format!("POSIX path of (choose file with prompt \"{title}\")")
        };
        let mut command = Command::new("osascript");
        command.args(["-e", &script]);
        return vec![command];
    }
    let patterns = patterns(filter).join(" ");
    let mut zenity = Command::new("zenity");
    zenity.args([
        "--file-selection".to_owned(),
        format!("--title={title}"),
        format!("--file-filter={name} | {patterns}"),
    ]);
    let mut kdialog = Command::new("kdialog");
    kdialog.args([
        "--title".to_owned(),
        title.to_owned(),
        "--getopenfilename".to_owned(),
        ".".to_owned(),
        format!("{patterns}|{name}"),
    ]);
    if multiple {
        zenity.args(["--multiple", "--separator=\n"]);
        kdialog.args(["--multiple", "--separate-output"]);
    }
    vec![zenity, kdialog]
}

#[cfg(test)]
mod tests {
    use super::*;
    use resource_pack::PACK_IMPORT_EXTENSIONS;

    const PACKS: Filter = Filter {
        title: "Import resource pack",
        name: "Resource packs",
        extensions: &PACK_IMPORT_EXTENSIONS,
        unavailable_hint: "Drop a pack onto the window to import it.",
        max_bytes: Some(resource_pack::MAX_ARCHIVE_BYTES as u64),
    };

    /// Returns a tool's arguments as strings for inspecting its dialog configuration.
    fn args(command: &Command) -> Vec<&str> {
        command
            .get_args()
            .map(|arg| arg.to_str().unwrap())
            .collect()
    }

    /// macOS cannot match unregistered types like `.wasm`, so the dialog lists every file.
    #[test]
    fn macos_picker_leaves_extension_checks_to_the_selection() {
        for multiple in [false, true] {
            let commands = tool_commands("macos", &PACKS, multiple);
            assert!(!args(&commands[0])[1].contains("of type"));
        }
    }

    #[test]
    fn selections_keep_only_accepted_extensions() {
        let picked = vec![
            PathBuf::from("/packs/Faithful.MCPACK"),
            PathBuf::from("/packs/notes.txt"),
            PathBuf::from("/packs/bundle.zip"),
        ];
        assert_eq!(
            selected(&PACKS, picked).unwrap(),
            [
                PathBuf::from("/packs/Faithful.MCPACK"),
                PathBuf::from("/packs/bundle.zip")
            ]
        );
        assert!(
            selected(&PACKS, Vec::new()).unwrap().is_empty(),
            "cancelling is not an error"
        );
        let error = selected(&PACKS, vec![PathBuf::from("/mods/mod.wasm")]).unwrap_err();
        assert!(error.contains(".mcpack"), "{error}");
    }

    #[test]
    fn linux_fallbacks_try_zenity_then_kdialog_with_the_pack_filter() {
        let commands = tool_commands("linux", &PACKS, true);
        let programs = commands
            .iter()
            .map(|command| command.get_program().to_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(programs, ["zenity", "kdialog"]);
        for command in &commands {
            let arguments = args(command);
            let filter = arguments
                .iter()
                .find(|argument| argument.contains("*."))
                .unwrap();
            for extension in PACK_IMPORT_EXTENSIONS {
                assert!(filter.contains(&format!("*.{extension}")), "{filter}");
            }
        }
    }

    #[test]
    fn fallback_selection_mode_preserves_single_and_multiple_imports() {
        for multiple in [false, true] {
            for command in tool_commands("linux", &PACKS, multiple) {
                assert_eq!(args(&command).contains(&"--multiple"), multiple);
            }
            let commands = tool_commands("macos", &PACKS, multiple);
            assert_eq!(
                args(&commands[0])[1].contains("multiple selections allowed"),
                multiple
            );
        }
    }

    #[test]
    fn windows_filter_lists_every_pack_pattern_and_ends_with_double_nul() {
        let filter = windows_filter(&PACKS);
        assert!(filter.ends_with("\0\0"));
        let mut parts = filter.trim_end_matches('\0').split('\0');
        assert_eq!(parts.next(), Some(PACKS.name));
        let patterns = parts.next().unwrap().split(';').collect::<Vec<_>>();
        assert_eq!(patterns.len(), PACK_IMPORT_EXTENSIONS.len());
        assert_eq!(parts.next(), None);
    }

    #[test]
    fn tool_output_returns_each_path_without_splitting_spaces_or_pipes() {
        assert_eq!(
            output_paths(b"/packs/First Pack.mcpack\n/packs/Second|Pack.mcaddon\n\n"),
            [
                PathBuf::from("/packs/First Pack.mcpack"),
                PathBuf::from("/packs/Second|Pack.mcaddon"),
            ]
        );
        assert_eq!(
            output_paths(b"/packs/a.mcpack\n"),
            [PathBuf::from("/packs/a.mcpack")]
        );
        assert!(output_paths(b"\n").is_empty());
        assert!(output_paths(b"").is_empty());
    }

    #[cfg(all(unix, not(target_os = "android")))]
    #[test]
    fn a_missing_tool_falls_through_and_a_cancelled_one_stops() {
        let shell = |script: &str| {
            let mut command = Command::new("sh");
            command.args(["-c", script]);
            command
        };
        let missing = || Command::new("cinnabar-no-such-picker");
        assert_eq!(
            run_tools(
                vec![
                    missing(),
                    shell("printf '/packs/a.mcpack\\n/packs/b.mcpack\\n'")
                ],
                &PACKS
            ),
            Ok(vec![
                PathBuf::from("/packs/a.mcpack"),
                PathBuf::from("/packs/b.mcpack")
            ])
        );
        assert_eq!(
            run_tools(vec![shell("exit 1"), shell("echo /packs/b.mcpack")], &PACKS),
            Ok(Vec::new())
        );
        let error = run_tools(vec![missing()], &PACKS).unwrap_err();
        assert!(error.contains(PACKS.unavailable_hint), "{error}");
    }
}
