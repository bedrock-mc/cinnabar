//! The crashing thread's frames and the loaded modules' build ids, as `stack.json` in a crash
//! folder. Desktop monitors fill it from the minidump; Android fills it from the tombstone.

pub mod tombstone;

use std::{
    fs,
    io::{self, Read},
    path::Path,
};

use serde::{Deserialize, Serialize};

/// The folder member holding a native crash's [`Stack`].
pub const STACK_FILE: &str = "stack.json";
/// Frames kept from the crashing thread, innermost first.
pub const MAX_FRAMES: usize = 64;
/// Modules kept; a process rarely loads more.
pub const MAX_MODULES: usize = 512;
/// Longest module or function name kept.
const MAX_NAME: usize = 256;

/// A native crash's location without symbols: module-relative addresses a symbolicator resolves
/// against the module's build id.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stack {
    /// The signal or exception, such as `SIGSEGV / SEGV_MAPERR`.
    pub reason: String,
    pub modules: Vec<Module>,
    pub frames: Vec<Frame>,
}

/// One loaded module: its file name (never a path) and build identifiers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Module {
    pub name: String,
    /// Breakpad debug id: PDB GUID and age, Mach-O UUID or the ELF build id's first 16 bytes,
    /// each followed by the age.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub debug_id: Option<String>,
    /// The PE timestamp and size, or the full ELF build id in hex.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code_id: Option<String>,
    /// Cinnabar's own code, as opposed to system libraries and drivers.
    pub in_app: bool,
}

/// How a frame was found; scanned frames may be stale return addresses.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Trust {
    /// The faulting instruction.
    Context,
    /// Unwound by the platform; the address already identifies the calling instruction.
    Unwound,
    /// A stack word that points into a module's code.
    Scan,
}

/// One frame: an index into [`Stack::modules`] and the module-relative offset in hex, or the
/// absolute address when no module holds it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Frame {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub module: Option<usize>,
    pub offset: String,
    pub trust: Trust,
    /// The exported symbol the platform named, when it had one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub function: Option<String>,
}

impl Stack {
    /// The first frame in Cinnabar's own code.
    pub fn top_in_app(&self) -> Option<&Frame> {
        self.frames
            .iter()
            .find(|frame| self.module(frame).is_some_and(|module| module.in_app))
    }

    /// The module holding `frame`.
    pub fn module(&self, frame: &Frame) -> Option<&Module> {
        frame.module.and_then(|index| self.modules.get(index))
    }

    /// `module+offset`, `module!function`, or the bare address of `frame`.
    pub fn describe(&self, frame: &Frame) -> String {
        match (self.module(frame), &frame.function) {
            (Some(module), Some(function)) => format!("{}!{function}", module.name),
            (Some(module), None) => format!("{}+{}", module.name, frame.offset),
            (None, _) => frame.offset.clone(),
        }
    }

    /// Bounds names and counts and drops frames that name a missing module, so a stack read
    /// from disk or a device is safe to send.
    pub fn sanitized(mut self) -> Self {
        self.reason = bounded(&self.reason);
        self.modules.truncate(MAX_MODULES);
        for module in &mut self.modules {
            module.name = bounded(crate::native::module_name(&module.name));
            module.debug_id = module
                .debug_id
                .as_deref()
                .map(hex_id)
                .filter(|id| !id.is_empty());
            module.code_id = module
                .code_id
                .as_deref()
                .map(hex_id)
                .filter(|id| !id.is_empty());
        }
        let modules = self.modules.len();
        self.frames.truncate(MAX_FRAMES);
        for frame in &mut self.frames {
            if frame.module.is_some_and(|index| index >= modules) {
                frame.module = None;
            }
            frame.offset = hex_id(&frame.offset);
            frame.function = frame.function.as_deref().map(bounded);
        }
        self
    }
}

/// Reads a folder's stack; `None` when it has none or it does not parse.
pub fn read(folder: &Path) -> Option<Stack> {
    let mut bytes = Vec::new();
    fs::File::open(folder.join(STACK_FILE))
        .ok()?
        .take(1 << 20)
        .read_to_end(&mut bytes)
        .ok()?;
    serde_json::from_slice::<Stack>(&bytes)
        .ok()
        .map(Stack::sanitized)
}

/// Writes `stack` as a folder's [`STACK_FILE`], replacing any earlier one.
pub fn write(folder: &Path, stack: &Stack) -> io::Result<()> {
    let path = folder.join(STACK_FILE);
    let temporary = folder.join(format!(".{STACK_FILE}.tmp"));
    fs::write(&temporary, serde_json::to_vec_pretty(stack)?)?;
    fs::rename(temporary, path)
}

/// Keeps printable characters of `text`, at most [`MAX_NAME`] of them.
fn bounded(text: &str) -> String {
    text.chars()
        .filter(|character| !character.is_control())
        .take(MAX_NAME)
        .collect()
}

/// Accepts hex ids and offsets, rejecting malformed values rather than inventing an id.
fn hex_id(text: &str) -> String {
    let (prefix, digits) = text
        .strip_prefix("0x")
        .map_or(("", text), |rest| ("0x", rest));
    if digits.is_empty()
        || digits.len() > 80
        || !digits.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        String::new()
    } else {
        format!("{prefix}{digits}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Models a driver frame, application frame and invalid module reference.
    fn stack() -> Stack {
        Stack {
            reason: "SIGSEGV / SEGV_MAPERR".into(),
            modules: vec![
                Module {
                    name: "/vendor/lib64/libvulkan_adreno.so".into(),
                    debug_id: None,
                    code_id: Some("abcdef".into()),
                    in_app: false,
                },
                Module {
                    name: "libcinnabar.so".into(),
                    debug_id: Some("0123456789ABCDEF0123456789ABCDEF0".into()),
                    code_id: None,
                    in_app: true,
                },
            ],
            frames: vec![
                Frame {
                    module: Some(0),
                    offset: "0x1234".into(),
                    trust: Trust::Context,
                    function: None,
                },
                Frame {
                    module: Some(1),
                    offset: "0x99".into(),
                    trust: Trust::Unwound,
                    function: Some("compile".into()),
                },
                Frame {
                    module: Some(7),
                    offset: "0x5".into(),
                    trust: Trust::Scan,
                    function: None,
                },
            ],
        }
    }

    #[test]
    fn the_top_in_app_frame_skips_driver_frames() {
        let stack = stack().sanitized();
        assert_eq!(stack.modules[0].name, "libvulkan_adreno.so");
        let top = stack.top_in_app().unwrap();
        assert_eq!(stack.describe(top), "libcinnabar.so!compile");
        assert_eq!(
            stack.describe(&stack.frames[0]),
            "libvulkan_adreno.so+0x1234"
        );
        assert_eq!(
            stack.frames[2].module, None,
            "a missing module index is dropped"
        );
    }

    #[test]
    fn ids_and_offsets_keep_only_hex() {
        assert_eq!(hex_id("0x12zz34"), "");
        assert_eq!(hex_id("C:\\Users\\me"), "");
        assert_eq!(hex_id("none"), "");
        assert_eq!(hex_id("xyz"), "");
    }

    #[test]
    fn a_written_stack_reads_back() {
        let dir = tempfile::tempdir().unwrap();
        let stack = stack().sanitized();
        write(dir.path(), &stack).unwrap();
        assert_eq!(read(dir.path()), Some(stack));
        assert_eq!(read(&dir.path().join("missing")), None);
    }
}
