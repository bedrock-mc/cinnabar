//! What a minidump says about the fault it captured.

mod stack;

pub(crate) use stack::read as stack;

use std::{fs::File, io, path::Path};

use crash_report::native::{Address, module_name};
use minidump::{
    Minidump, MinidumpException, MinidumpMiscInfo, MinidumpModuleList, MinidumpSystemInfo,
    MinidumpThreadNames, Module,
};

/// Creates a new dump without exposing its raw process memory to other Unix users.
/// Existing paths, including symlinks, are rejected rather than truncated or reused.
pub(crate) fn create(path: &Path) -> io::Result<File> {
    let mut options = File::options();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}

/// The faulting thread and instruction of a native crash.
#[derive(Clone, Debug)]
pub(crate) struct Fault {
    /// The platform's name for the fault, such as `EXCEPTION_ACCESS_VIOLATION_READ` or
    /// `SIGSEGV / SEGV_MAPERR`.
    pub reason: String,
    /// The address the fault concerns, such as the one an access violation read.
    pub address: u64,
    /// The faulting instruction, relative to its module.
    pub instruction: Address,
    pub thread_id: u32,
    pub thread_name: Option<String>,
}

/// Reads the exception, module and thread name streams of the dump at `path`.
pub(crate) fn fault(path: &Path) -> Option<Fault> {
    let dump = Minidump::read_path(path)
        .map_err(|error| tracing::warn!("unreadable minidump: {error}"))
        .ok()?;
    let system = dump.get_stream::<MinidumpSystemInfo>().ok()?;
    let exception = dump.get_stream::<MinidumpException>().ok()?;
    let misc = dump.get_stream::<MinidumpMiscInfo>().ok();
    let thread_id = exception.get_crashing_thread_id();
    let instruction = exception.context(&system, misc.as_ref()).map_or(
        exception.raw.exception_record.exception_address,
        |context| context.get_instruction_pointer(),
    );
    let modules = dump.get_stream::<MinidumpModuleList>().ok();
    let module = modules
        .as_ref()
        .and_then(|modules| modules.module_at_address(instruction))
        .map(|module| {
            (
                module_name(&module.code_file()).to_owned(),
                module.base_address(),
            )
        });
    let thread_name = dump
        .get_stream::<MinidumpThreadNames>()
        .ok()
        .and_then(|names| names.get_name(thread_id).map(|name| name.into_owned()))
        .filter(|name| !name.is_empty());
    Some(Fault {
        reason: exception
            .get_crash_reason(system.os, system.cpu)
            .to_string(),
        address: exception.get_crash_address(system.os, system.cpu),
        instruction: Address::new(instruction, module),
        thread_id,
        thread_name,
    })
}

/// Rewrites `file` as a dump of all process memory, for developers who asked for one; the
/// client still waits on its dump request, so its memory is intact.
#[cfg(windows)]
pub(crate) fn rewrite_with_full_memory(
    file: &mut std::fs::File,
    context: launcher_host::lifecycle::crash::protocol::DumpContext,
) {
    use std::io::Seek;

    use minidump_writer::{crash_context::CrashContext, minidump_writer::MinidumpWriter};

    let crash = CrashContext {
        exception_pointers: context.exception_pointers as usize as *const _,
        process_id: context.process_id,
        thread_id: context.thread_id,
        exception_code: context.exception_code,
    };
    let rewound = file.set_len(0).and_then(|()| file.rewind());
    let written = rewound.map_err(|error| error.to_string()).and_then(|()| {
        MinidumpWriter::dump_crash_context(
            &crash,
            Some(minidump_writer::MinidumpType::WithFullMemory),
            file,
        )
        .map_err(|error| error.to_string())
    });
    match written {
        Ok(_) => tracing::info!("full-memory minidump written"),
        Err(error) => tracing::warn!("full-memory minidump failed: {error}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dump_creation_never_reuses_an_existing_path() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("existing.dmp");
        std::fs::write(&path, "previous dump").unwrap();
        assert_eq!(
            create(&path).unwrap_err().kind(),
            io::ErrorKind::AlreadyExists
        );
        assert_eq!(std::fs::read_to_string(path).unwrap(), "previous dump");
    }
}
