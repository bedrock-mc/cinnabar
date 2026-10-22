//! Extracts addresses and module identifiers locally; process memory never enters an upload.

use crash_report::{
    native::module_name,
    stack::{Frame, MAX_FRAMES, MAX_MODULES, Module as Image, Stack, Trust},
};
use minidump::{
    Minidump, MinidumpException, MinidumpMiscInfo, MinidumpModuleList, MinidumpSystemInfo,
    MinidumpThreadList, Module,
};
use std::path::Path;

/// Reads the faulting instruction and candidate return addresses from the crashing thread.
/// Stack scanning is deliberately marked as such: it can include stale return addresses.
pub(crate) fn read(path: &Path) -> Option<Stack> {
    let dump = Minidump::read_path(path).ok()?;
    let system = dump.get_stream::<MinidumpSystemInfo>().ok()?;
    let exception = dump.get_stream::<MinidumpException>().ok()?;
    let misc = dump.get_stream::<MinidumpMiscInfo>().ok();
    let context = exception.context(&system, misc.as_ref())?;
    let modules = dump.get_stream::<MinidumpModuleList>().ok()?;
    let loaded: Vec<_> = modules.iter().take(MAX_MODULES).collect();
    let executable = std::env::current_exe().ok()?;
    let executable = executable.file_name()?.to_string_lossy();
    let images = loaded
        .iter()
        .map(|module| {
            let name = module_name(&module.code_file()).to_owned();
            Image {
                in_app: name == executable,
                name,
                debug_id: module
                    .debug_identifier()
                    .map(|id| id.breakpad().to_string()),
                code_id: module.code_identifier().map(|id| id.to_string()),
            }
        })
        .collect();
    let frame = |address: u64, trust| {
        let index = loaded.iter().position(|module| {
            address >= module.base_address() && address - module.base_address() < module.size()
        });
        Frame {
            module: index,
            offset: format!(
                "0x{:x}",
                index.map_or(address, |i| address - loaded[i].base_address())
            ),
            trust,
            function: None,
        }
    };
    let mut frames = vec![frame(context.get_instruction_pointer(), Trust::Context)];
    if let (Ok(threads), Some(memory)) =
        (dump.get_stream::<MinidumpThreadList>(), dump.get_memory())
        && let Some(thread) = threads
            .threads
            .iter()
            .find(|thread| thread.raw.thread_id == exception.get_crashing_thread_id())
        && let Some(stack) = thread.stack_memory(&memory)
    {
        let width = u64::from(system.cpu.pointer_width().size_in_bytes()?);
        // Scan only a bounded part of the captured stack, never arbitrary process memory.
        for word in 0..4096 {
            let address = context.get_stack_pointer().checked_add(word * width)?;
            let candidate = if width == 4 {
                stack.get_memory_at_address::<u32>(address).map(u64::from)
            } else {
                stack.get_memory_at_address::<u64>(address)
            };
            let Some(candidate) = candidate else { break };
            let candidate = frame(candidate, Trust::Scan);
            if candidate.module.is_some()
                && !frames.iter().any(|frame| {
                    frame.module == candidate.module && frame.offset == candidate.offset
                })
            {
                frames.push(candidate);
                if frames.len() >= MAX_FRAMES {
                    break;
                }
            }
        }
    }
    Some(
        Stack {
            reason: exception
                .get_crash_reason(system.os, system.cpu)
                .to_string(),
            modules: images,
            frames,
        }
        .sanitized(),
    )
}
