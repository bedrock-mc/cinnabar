//! Offline crash symbolication: walks a crash folder's minidump against a release symbol archive.
//! Nothing here touches the network; symbols come only from the directory or zip the caller names.
//! `wholesym` links an HTTP client, but only its symbol-server paths use it, and neither entry
//! point here enables them: `SymbolManager::library_info_for_binary_at_path` only reads the file,
//! and `DebugInfoSymbolProvider::new` builds `SymbolManagerConfig::new()`, whose defaults (0.8)
//! configure no Windows, breakpad or debuginfod servers and ignore `_NT_SYMBOL_PATH` and
//! `DEBUGINFOD_URLS`. Modules reach it only as archive paths, never as server lookups.
use crate::DevtoolError;
use crash_report::{
    folder::{self, DUMP_FILE, REPORT_FILE},
    report::Report,
};
use minidump::{
    Minidump, MinidumpModule, MinidumpModuleList, MinidumpSystemInfo, Module, system_info::Cpu,
};
use minidump_processor::ProcessState;
use minidump_unwind::debuginfo::DebugInfoSymbolProvider;
use serde_json::Value;
use std::{
    collections::BTreeMap,
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
};
use wholesym::{LibraryInfo, SymbolManager};

/// The release symbol archive's description of its build, named in packaging/release-assets.json.
pub(crate) const SYMBOLS_MANIFEST: &str = "manifest.json";
/// Folder levels searched below the symbols root; archives are flat, extracted copies may nest.
const MAX_SYMBOL_DEPTH: usize = 3;

/// Parsed `symbolicate` arguments.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Options {
    pub(crate) input: PathBuf,
    pub(crate) symbols: Option<PathBuf>,
}

/// Accepts one crash folder or minidump path and an optional `--symbols` directory or zip.
pub(crate) fn parse_args(args: &[String]) -> Result<Options, DevtoolError> {
    let usage = |message: &str| DevtoolError::Usage(format!("symbolicate: {message}"));
    let mut input = None;
    let mut symbols = None;
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--symbols" => {
                let path = args
                    .next()
                    .filter(|path| !path.is_empty())
                    .ok_or_else(|| usage("`--symbols` requires a directory or zip"))?;
                if symbols.replace(PathBuf::from(path)).is_some() {
                    return Err(usage("`--symbols` given twice"));
                }
            }
            flag if flag.starts_with("--") => {
                return Err(usage(&format!("unknown argument `{flag}`")));
            }
            path => {
                if input.replace(PathBuf::from(path)).is_some() {
                    return Err(usage("expected one crash folder or minidump"));
                }
            }
        }
    }
    let input = input.ok_or_else(|| usage("missing crash folder or minidump"))?;
    Ok(Options { input, symbols })
}

/// The files `symbolicate` reads for one crash.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct CrashInput {
    pub(crate) dump: Option<PathBuf>,
    /// The crash folder, when it holds a report.
    pub(crate) report_folder: Option<PathBuf>,
}

/// A file input is the minidump, with any `report.json` beside it; a folder supplies its own
/// `crash.dmp` and `report.json`, at least one of which must exist.
pub(crate) fn locate(input: &Path) -> Result<CrashInput, DevtoolError> {
    let metadata = fs::metadata(input).map_err(|error| failure(input, error))?;
    let (dump, folder) = if metadata.is_dir() {
        let dump = input.join(DUMP_FILE);
        (dump.is_file().then_some(dump), input)
    } else {
        (
            Some(input.to_path_buf()),
            input.parent().unwrap_or(Path::new(".")),
        )
    };
    let crash = CrashInput {
        dump,
        report_folder: folder
            .join(REPORT_FILE)
            .is_file()
            .then(|| folder.to_path_buf()),
    };
    if crash.dump.is_none() && crash.report_folder.is_none() {
        return Err(DevtoolError::Symbolicate(format!(
            "{} holds neither {DUMP_FILE} nor {REPORT_FILE}",
            input.display()
        )));
    }
    Ok(crash)
}

/// Prints the report summary and, for a minidump, every thread's symbolicated stack.
pub(crate) fn run(args: &[String]) -> Result<(), DevtoolError> {
    let options = parse_args(args)?;
    let crash = locate(&options.input)?;
    let mut out = String::new();
    let report = match &crash.report_folder {
        Some(crash_folder) => {
            let report = folder::read(crash_folder)
                .map_err(|error| failure(&crash_folder.join(REPORT_FILE), error))?;
            out.push_str(&report_summary(crash_folder, &report));
            Some(report)
        }
        None => None,
    };
    if let Some(dump) = &crash.dump {
        let symbols = options.symbols.as_deref().ok_or_else(|| {
            DevtoolError::Usage(format!("symbolicate: {} needs `--symbols`", dump.display()))
        })?;
        let symbols = SymbolRoot::open(symbols)?;
        out.push_str(&symbols_summary(symbols.path(), report.as_ref()));
        let walked = symbolicate_dump(dump, symbols.path())?;
        out.push_str(&walked.notes);
        out.push_str(&format_state(&walked.state));
    }
    print!("{out}");
    Ok(())
}

/// A symbols directory, or a zip archive extracted for the lifetime of this value.
pub(crate) enum SymbolRoot {
    Directory(PathBuf),
    Extracted(tempfile::TempDir),
}

impl SymbolRoot {
    pub(crate) fn open(path: &Path) -> Result<Self, DevtoolError> {
        if fs::metadata(path)
            .map_err(|error| failure(path, error))?
            .is_dir()
        {
            return Ok(Self::Directory(path.to_path_buf()));
        }
        let file = fs::File::open(path).map_err(|error| failure(path, error))?;
        let mut archive = zip::ZipArchive::new(file).map_err(|error| {
            DevtoolError::Symbolicate(format!(
                "{} is neither a directory nor a zip archive: {error}",
                path.display()
            ))
        })?;
        let temp = tempfile::tempdir().map_err(|error| failure(path, error))?;
        archive.extract(temp.path()).map_err(|error| {
            DevtoolError::Symbolicate(format!("cannot extract {}: {error}", path.display()))
        })?;
        Ok(Self::Extracted(temp))
    }

    pub(crate) fn path(&self) -> &Path {
        match self {
            Self::Directory(path) => path,
            Self::Extracted(temp) => temp.path(),
        }
    }
}

/// A processed minidump plus one line per crash module matched or rejected against the symbols.
pub(crate) struct Walked {
    pub(crate) state: ProcessState,
    pub(crate) notes: String,
}

/// Walks every thread of `dump`, symbolicating only modules whose build matches a binary
/// under `symbols`; other modules keep module-and-offset frames.
pub(crate) fn symbolicate_dump(dump: &Path, symbols: &Path) -> Result<Walked, DevtoolError> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| failure(dump, error))?;
    let index = index_files(symbols)?;
    runtime.block_on(async {
        let minidump = Minidump::read_path(dump).map_err(|error| dump_failure(dump, error))?;
        let system_info = minidump
            .get_stream::<MinidumpSystemInfo>()
            .map_err(|error| dump_failure(dump, error))?;
        if !matches!(system_info.cpu, Cpu::X86_64 | Cpu::Arm64) {
            return Err(DevtoolError::Symbolicate(format!(
                "{}: unsupported CPU {}",
                dump.display(),
                system_info.cpu
            )));
        }
        let modules = minidump
            .get_stream::<MinidumpModuleList>()
            .unwrap_or_else(|_| MinidumpModuleList::new());
        let (local, notes) = match_modules(&modules, &index).await;
        // Default wholesym config: local files only (see the module docs).
        let provider =
            DebugInfoSymbolProvider::new(&system_info, &MinidumpModuleList::from_modules(local))
                .await;
        let state = minidump_processor::process_minidump(&minidump, &provider)
            .await
            .map_err(|error| DevtoolError::Symbolicate(format!("{}: {error}", dump.display())))?;
        Ok::<_, DevtoolError>(Walked { state, notes })
    })
}

/// Files under `root` by lowercase name. `.dSYM` bundles are not entered: their binary's
/// symbol loader finds them beside it.
fn index_files(root: &Path) -> Result<BTreeMap<String, Vec<PathBuf>>, DevtoolError> {
    let mut index = BTreeMap::<String, Vec<PathBuf>>::new();
    let mut pending = vec![(root.to_path_buf(), 0)];
    while let Some((directory, depth)) = pending.pop() {
        let entries = fs::read_dir(&directory).map_err(|error| failure(&directory, error))?;
        for entry in entries {
            let path = entry.map_err(|error| failure(&directory, error))?.path();
            let Some(name) = path
                .file_name()
                .map(|name| name.to_string_lossy().to_lowercase())
            else {
                continue;
            };
            if path.is_dir() {
                if depth < MAX_SYMBOL_DEPTH && !name.ends_with(".dsym") {
                    pending.push((path, depth + 1));
                }
            } else if path.is_file() {
                index.entry(name).or_default().push(path);
            }
        }
    }
    for paths in index.values_mut() {
        paths.sort();
    }
    Ok(index)
}

/// The file name of a module path recorded on any OS.
fn module_file_name(code_file: &str) -> &str {
    code_file.rsplit(['/', '\\']).next().unwrap_or(code_file)
}

/// Copies of the modules with a same-build local binary, renamed to that binary, plus notes.
async fn match_modules(
    modules: &MinidumpModuleList,
    index: &BTreeMap<String, Vec<PathBuf>>,
) -> (Vec<MinidumpModule>, String) {
    let mut local = Vec::new();
    let mut notes = String::new();
    for module in modules.iter() {
        let code_file = module.code_file();
        let name = module_file_name(&code_file);
        let Some(candidates) = index.get(&name.to_lowercase()) else {
            continue;
        };
        let mut rejected = Vec::new();
        let mut matched = None;
        for candidate in candidates {
            match SymbolManager::library_info_for_binary_at_path(candidate, None).await {
                Ok(info) if same_build(module, &info) => {
                    matched = Some(candidate);
                    break;
                }
                Ok(info) => rejected.push(format!(
                    "{} is build {}",
                    candidate.display(),
                    info.debug_id
                        .map_or_else(|| "unknown".into(), |id| id.to_string())
                )),
                Err(error) => rejected.push(format!("{}: {error}", candidate.display())),
            }
        }
        match matched {
            Some(path) => {
                let _ = writeln!(notes, "symbols: {name} from {}", path.display());
                let mut copy = module.clone();
                copy.name = path.to_string_lossy().into_owned();
                // The binary's own debug link names its debug file beside it.
                copy.codeview_info = None;
                local.push(copy);
            }
            None => {
                let crashed = module
                    .debug_identifier()
                    .map_or_else(|| "unknown".into(), |id| id.to_string());
                let _ = writeln!(
                    notes,
                    "warning: no symbols match {name} build {crashed}: {}",
                    rejected.join("; ")
                );
            }
        }
    }
    (local, notes)
}

/// The crashed module and a local binary are one build when their debug or code IDs agree.
fn same_build(module: &MinidumpModule, info: &LibraryInfo) -> bool {
    let debug = module
        .debug_identifier()
        .is_some_and(|id| info.debug_id == Some(id));
    let code = module
        .code_identifier()
        .zip(info.code_id.as_ref())
        .is_some_and(|(crashed, local)| crashed.as_str().eq_ignore_ascii_case(&local.to_string()));
    debug || code
}

/// The report fields a reader needs to place the crash.
pub(crate) fn report_summary(crash_folder: &Path, report: &Report) -> String {
    let mut out = format!("Crash report {}\n", crash_folder.display());
    let kind = serde_json::to_value(report.kind)
        .ok()
        .as_ref()
        .and_then(scalar)
        .unwrap_or_default();
    let uptime = report.uptime_ms.to_string();
    for (label, value) in [
        ("kind", Some(kind.as_str())),
        ("message", Some(report.message.as_str())),
        ("release", Some(report.release.as_str())),
        ("commit", Some(report.commit.as_str())),
        ("build profile", Some(report.build_profile.as_str())),
        ("os", Some(report.os.as_str())),
        ("arch", Some(report.arch.as_str())),
        ("gpu", report.gpu.as_deref()),
        ("driver", report.driver.as_deref()),
        ("session", Some(report.session_id.as_str())),
        ("uptime ms", Some(uptime.as_str())),
        ("comment", report.user_comment.as_deref()),
    ] {
        if let Some(value) = value.filter(|value| !value.is_empty()) {
            let _ = writeln!(out, "  {label}: {value}");
        }
    }
    let _ = writeln!(
        out,
        "  thread: {} (id {})",
        report.thread.name.as_deref().unwrap_or("unnamed"),
        report.thread.id
    );
    for (label, text) in [
        ("backtrace", Some(report.backtrace.as_str())),
        ("span trace", report.span_trace.as_deref()),
    ] {
        if let Some(text) = text.filter(|text| !text.is_empty()) {
            let _ = writeln!(out, "  {label}:");
            for line in text.lines() {
                let _ = writeln!(out, "    {line}");
            }
        }
    }
    out
}

/// The symbol archive's build, with a warning when it differs from the report's build.
pub(crate) fn symbols_summary(root: &Path, report: Option<&Report>) -> String {
    let Ok(text) = fs::read_to_string(root.join(SYMBOLS_MANIFEST)) else {
        return format!("Symbols {} (no {SYMBOLS_MANIFEST})\n", root.display());
    };
    let Ok(manifest) = serde_json::from_str::<Value>(&text) else {
        return format!(
            "Symbols {} (unreadable {SYMBOLS_MANIFEST})\n",
            root.display()
        );
    };
    let field = |key: &str| manifest.get(key).and_then(scalar);
    let version = field("version");
    let commit = field("commit");
    let mut out = format!(
        "Symbols {} ({}) for {}\n",
        version.as_deref().unwrap_or("unknown version"),
        commit.as_deref().unwrap_or("unknown commit"),
        field("platform").as_deref().unwrap_or("unknown platform"),
    );
    if let (Some(symbols), Some(crashed)) = (&commit, report.map(|report| report.commit.as_str()))
        && !crashed.is_empty()
        && !(symbols.starts_with(crashed) || crashed.starts_with(symbols.as_str()))
    {
        let _ = writeln!(
            out,
            "warning: symbols are from commit {symbols}, the crash from {crashed}"
        );
    }
    if let (Some(symbols), Some(crashed)) = (&version, report.map(|report| report.release.as_str()))
        && symbols != crashed
    {
        let _ = writeln!(
            out,
            "warning: symbols are release {symbols}, the crash is {crashed}"
        );
    }
    out
}

/// A string or number as display text.
fn scalar(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        Value::Bool(flag) => Some(flag.to_string()),
        _ => None,
    }
}

/// The crash reason and every thread's frames, innermost first, inlined frames before their caller.
pub(crate) fn format_state(state: &ProcessState) -> String {
    let mut out = String::new();
    if let Some(exception) = &state.exception_info {
        let _ = writeln!(
            out,
            "Crash: {} at {:#x}",
            exception.reason, exception.address.0
        );
    }
    for (index, thread) in state.threads.iter().enumerate() {
        let crashed = if state.requesting_thread == Some(index) {
            " (crashed)"
        } else {
            ""
        };
        let _ = writeln!(
            out,
            "\nThread {index} {} id {:#x}{crashed}",
            thread.thread_name.as_deref().unwrap_or("unnamed"),
            thread.thread_id
        );
        let mut number = 0;
        for frame in &thread.frames {
            let module = frame
                .module
                .as_ref()
                .map(|module| module_file_name(&module.code_file()).to_owned());
            let module = module.as_deref().unwrap_or("???");
            for inline in &frame.inlines {
                let _ = writeln!(
                    out,
                    "  #{number:<3} {:#018x} {module}!{} (inlined){}",
                    frame.instruction,
                    inline.function_name,
                    source(inline.source_file_name.as_deref(), inline.source_line)
                );
                number += 1;
            }
            let location = match (&frame.function_name, frame.function_base) {
                (Some(function), Some(base)) => {
                    format!("{function} + {:#x}", frame.instruction.saturating_sub(base))
                }
                (Some(function), None) => function.clone(),
                _ => {
                    let base = frame
                        .module
                        .as_ref()
                        .map_or(0, |module| module.base_address());
                    format!("{:#x}", frame.instruction.saturating_sub(base))
                }
            };
            let _ = writeln!(
                out,
                "  #{number:<3} {:#018x} {module}!{location}{}",
                frame.instruction,
                source(frame.source_file_name.as_deref(), frame.source_line)
            );
            number += 1;
        }
    }
    out
}

fn source(file: Option<&str>, line: Option<u32>) -> String {
    match (file, line) {
        (Some(file), Some(line)) => format!("  {file}:{line}"),
        (Some(file), None) => format!("  {file}"),
        _ => String::new(),
    }
}

fn failure(path: &Path, error: impl std::fmt::Display) -> DevtoolError {
    DevtoolError::Symbolicate(format!("{}: {error}", path.display()))
}

fn dump_failure(path: &Path, error: minidump::Error) -> DevtoolError {
    DevtoolError::Symbolicate(format!(
        "{} is not a readable minidump: {error}",
        path.display()
    ))
}

#[cfg(test)]
mod tests;
