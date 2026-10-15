use super::*;
use crash_report::report::{Identity, Kind};
use minidump::format::{PlatformId, ProcessorArchitecture};
use minidump_synth::{
    DumpString, Exception, Memory, Module as SynthModule, SynthMinidump, SystemInfo, Thread,
    ThreadName, amd64_context,
};
use test_assembler::{Endian, Section};

const MODULE_BASE: u64 = 0x7ff6_0000_0000;
const CRASH_OFFSET: u64 = 0x1234;
const STACK_BASE: u64 = 0x10_0000;
const THREAD_ID: u32 = 7;

fn args(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

fn report(commit: &str, release: &str) -> Report {
    let identity = Identity {
        session_id: "session-1".to_owned(),
        started_at_ms: 1_000,
        release: release.to_owned(),
        commit: commit.to_owned(),
        build_profile: "release".to_owned(),
    };
    let mut report = Report::new(
        Kind::Native,
        "access violation".to_owned(),
        &identity,
        4_000,
    );
    report.backtrace = "0: cinnabar::main\n1: std::rt::lang_start".to_owned();
    report
}

/// A Windows x86-64 dump whose only thread faulted inside `cinnabar.exe`.
fn synthetic_dump() -> Vec<u8> {
    let module_name = DumpString::new(r"C:\Program Files\Cinnabar\cinnabar.exe", Endian::Little);
    let module = SynthModule::new(
        Endian::Little,
        MODULE_BASE,
        0x10_000,
        &module_name,
        0x5f00_0000,
        0,
        None,
    );
    let context = amd64_context(Endian::Little, MODULE_BASE + CRASH_OFFSET, STACK_BASE);
    let stack = Memory::with_section(
        Section::with_endian(Endian::Little).append_repeated(0, 0x100),
        STACK_BASE,
    );
    let thread = Thread::new(Endian::Little, THREAD_ID, &stack, &context);
    let thread_name = DumpString::new("main", Endian::Little);
    let system_info = SystemInfo::new(Endian::Little)
        .set_processor_architecture(ProcessorArchitecture::PROCESSOR_ARCHITECTURE_AMD64 as u16)
        .set_platform_id(PlatformId::VER_PLATFORM_WIN32_NT as u32);
    let mut exception = Exception::new(Endian::Little);
    exception.thread_id = THREAD_ID;
    exception.exception_record.exception_code = 0xc000_0005;
    exception.exception_record.exception_address = MODULE_BASE + CRASH_OFFSET;
    SynthMinidump::with_endian(Endian::Little)
        .add_module(module)
        .add(module_name)
        .add_thread(thread)
        .add_thread_name(ThreadName::new(
            Endian::Little,
            THREAD_ID,
            Some(&thread_name),
        ))
        .add(thread_name)
        .add_exception(exception)
        .add_system_info(system_info)
        .add(context)
        .add_memory(stack)
        .finish()
        .expect("synthetic minidump")
}

#[test]
fn arguments_name_one_input_and_optional_symbols() {
    assert_eq!(
        parse_args(&args(&["crash", "--symbols", "symbols.zip"])).unwrap(),
        Options {
            input: PathBuf::from("crash"),
            symbols: Some(PathBuf::from("symbols.zip")),
        }
    );
    assert_eq!(
        parse_args(&args(&["crash.dmp"])).unwrap(),
        Options {
            input: PathBuf::from("crash.dmp"),
            symbols: None,
        }
    );
    for invalid in [
        &[][..],
        &["--symbols", "dir"][..],
        &["a", "b"][..],
        &["a", "--symbols"][..],
        &["a", "--symbols", "x", "--symbols", "y"][..],
        &["a", "--network"][..],
    ] {
        assert!(
            matches!(parse_args(&args(invalid)), Err(DevtoolError::Usage(_))),
            "{invalid:?} should be a usage error"
        );
    }
}

#[test]
fn folders_supply_their_dump_and_report_and_files_their_sibling_report() {
    let root = tempfile::tempdir().unwrap();
    let report_only = root.path().join("report-only");
    fs::create_dir(&report_only).unwrap();
    fs::write(report_only.join(REPORT_FILE), "{}").unwrap();
    assert_eq!(
        locate(&report_only).unwrap(),
        CrashInput {
            dump: None,
            report_folder: Some(report_only.clone()),
        }
    );

    let native = root.path().join("native");
    fs::create_dir(&native).unwrap();
    fs::write(native.join(REPORT_FILE), "{}").unwrap();
    fs::write(native.join(DUMP_FILE), b"MDMP").unwrap();
    let expected = CrashInput {
        dump: Some(native.join(DUMP_FILE)),
        report_folder: Some(native.clone()),
    };
    assert_eq!(locate(&native).unwrap(), expected);
    assert_eq!(locate(&native.join(DUMP_FILE)).unwrap(), expected);

    let loose = root.path().join("elsewhere.dmp");
    fs::write(&loose, b"MDMP").unwrap();
    assert_eq!(
        locate(&loose).unwrap(),
        CrashInput {
            dump: Some(loose.clone()),
            report_folder: None,
        }
    );

    let empty = root.path().join("empty");
    fs::create_dir(&empty).unwrap();
    let error = locate(&empty).unwrap_err().to_string();
    assert!(
        error.contains(DUMP_FILE) && error.contains(REPORT_FILE),
        "{error}"
    );
    assert!(locate(&root.path().join("missing")).is_err());
}

#[test]
fn report_summary_names_the_build_and_backtrace() {
    let summary = report_summary(Path::new("crashes/one"), &report("abc123", "1.2.3"));
    for expected in [
        "kind: native",
        "message: access violation",
        "release: 1.2.3",
        "commit: abc123",
        "build profile: release",
        "session: session-1",
        "uptime ms: 3000",
        "    0: cinnabar::main",
    ] {
        assert!(
            summary.contains(expected),
            "missing {expected:?} in\n{summary}"
        );
    }
}

#[test]
fn symbols_from_another_build_are_flagged() {
    let symbols = tempfile::tempdir().unwrap();
    fs::write(
        symbols.path().join(SYMBOLS_MANIFEST),
        r#"{"version": "1.2.3", "commit": "abc123def", "platform": "windows-x86_64"}"#,
    )
    .unwrap();

    let same = symbols_summary(symbols.path(), Some(&report("abc123", "1.2.3")));
    assert!(
        same.contains("1.2.3 (abc123def) for windows-x86_64"),
        "{same}"
    );
    assert!(!same.contains("warning"), "{same}");

    let other = symbols_summary(symbols.path(), Some(&report("fedcba", "1.2.4")));
    assert!(
        other.contains("symbols are from commit abc123def, the crash from fedcba"),
        "{other}"
    );
    assert!(
        other.contains("symbols are release 1.2.3, the crash is 1.2.4"),
        "{other}"
    );

    let missing = tempfile::tempdir().unwrap();
    assert!(symbols_summary(missing.path(), None).contains(&format!("no {SYMBOLS_MANIFEST}")));
}

#[test]
fn symbol_manifest_name_matches_the_release_packaging() {
    let assets = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packaging/release-assets.json");
    let assets: Value = serde_json::from_slice(&fs::read(assets).unwrap()).unwrap();
    assert_eq!(assets["symbols"]["manifest"], SYMBOLS_MANIFEST);
}

#[test]
fn zip_symbols_are_extracted_for_the_walk() {
    let root = tempfile::tempdir().unwrap();
    let archive = root.path().join("symbols.zip");
    let mut writer = zip::ZipWriter::new(fs::File::create(&archive).unwrap());
    writer
        .start_file(SYMBOLS_MANIFEST, zip::write::SimpleFileOptions::default())
        .unwrap();
    std::io::Write::write_all(&mut writer, br#"{"version": "1.2.3"}"#).unwrap();
    writer.finish().unwrap();

    let opened = SymbolRoot::open(&archive).unwrap();
    assert!(opened.path().join(SYMBOLS_MANIFEST).is_file());
    assert!(matches!(
        SymbolRoot::open(root.path()).unwrap(),
        SymbolRoot::Directory(_)
    ));
    fs::write(root.path().join("not.zip"), b"plain text").unwrap();
    assert!(SymbolRoot::open(&root.path().join("not.zip")).is_err());
}

#[test]
fn dump_threads_walk_with_module_offsets_and_mismatched_symbols_are_reported() {
    let root = tempfile::tempdir().unwrap();
    let dump = root.path().join(DUMP_FILE);
    fs::write(&dump, synthetic_dump()).unwrap();
    let symbols = root.path().join("symbols");
    fs::create_dir(&symbols).unwrap();
    // Same name as the crashed module but no binary at all, so it cannot match its build.
    fs::write(symbols.join("cinnabar.exe"), b"not a portable executable").unwrap();

    let walked = symbolicate_dump(&dump, &symbols).unwrap();
    assert!(
        walked
            .notes
            .contains("warning: no symbols match cinnabar.exe"),
        "{}",
        walked.notes
    );
    let stack = format_state(&walked.state);
    assert!(stack.starts_with("Crash: "), "{stack}");
    assert!(
        stack.contains(&format!("Thread 0 main id {THREAD_ID:#x} (crashed)")),
        "{stack}"
    );
    let first_frame = format!(
        "#0   {:#018x} cinnabar.exe!{CRASH_OFFSET:#x}",
        MODULE_BASE + CRASH_OFFSET
    );
    assert!(
        stack.contains(&first_frame),
        "missing {first_frame:?} in\n{stack}"
    );
}

#[test]
fn unreadable_dumps_are_rejected() {
    let root = tempfile::tempdir().unwrap();
    let dump = root.path().join(DUMP_FILE);
    fs::write(&dump, b"not a minidump").unwrap();
    let error = symbolicate_dump(&dump, root.path())
        .err()
        .unwrap()
        .to_string();
    assert!(error.contains("not a readable minidump"), "{error}");
}
