//! Android's `tombstone.pb` (debuggerd's `Tombstone` protobuf, API 31+): the crashing thread's
//! unwound frames with each library's build id. Only the signal, frames and module build identifiers are read;
//! memory dumps, registers, log buffers and open files are skipped unread.

use super::{Frame, MAX_FRAMES, Module, Stack, Trust};

/// `Tombstone` fields.
const TID: u32 = 6;
const SIGNAL: u32 = 10;
const THREADS: u32 = 16;
const MAPPINGS: u32 = 17;
/// `Signal` fields.
const SIGNAL_NAME: u32 = 2;
const SIGNAL_CODE_NAME: u32 = 4;
/// `map<uint32, Thread>` entry fields.
const ENTRY_KEY: u32 = 1;
const ENTRY_VALUE: u32 = 2;
/// `Thread` fields.
const THREAD_ID: u32 = 1;
const THREAD_BACKTRACE: u32 = 4;
/// `BacktraceFrame` fields.
const FRAME_REL_PC: u32 = 1;
const FRAME_FUNCTION: u32 = 4;
const FRAME_FILE: u32 = 6;
const FRAME_BUILD_ID: u32 = 8;

/// The crashing thread's stack from a tombstone; `in_app` says which library file names are
/// Cinnabar's. `None` when the bytes are not a tombstone with a crashing thread.
pub fn stack(bytes: &[u8], in_app: impl Fn(&str) -> bool) -> Option<Stack> {
    let mut tid = None;
    let mut reason = String::new();
    let mut threads = Vec::new();
    let mut mappings = Vec::new();
    for field in Fields::new(bytes) {
        match field.ok()? {
            (TID, Value::Varint(value)) => tid = u32::try_from(value).ok(),
            (SIGNAL, Value::Bytes(signal)) => reason = signal_reason(signal)?,
            (THREADS, Value::Bytes(entry)) => threads.push(entry),
            (MAPPINGS, Value::Bytes(mapping)) => mappings.push(mapping),
            _ => {}
        }
    }
    let tid = tid?;
    let thread = threads.into_iter().find_map(|entry| {
        let mut key = None;
        let mut value = None;
        for field in Fields::new(entry) {
            match field.ok()? {
                (ENTRY_KEY, Value::Varint(id)) => key = Some(id),
                (ENTRY_VALUE, Value::Bytes(thread)) => value = Some(thread),
                _ => {}
            }
        }
        let thread = value?;
        let id = key.or_else(|| thread_id(thread))?;
        (id == u64::from(tid)).then_some(thread)
    })?;
    let mut stack = Stack {
        reason,
        ..Stack::default()
    };
    for field in Fields::new(thread) {
        let (THREAD_BACKTRACE, Value::Bytes(frame)) = field.ok()? else {
            continue;
        };
        if stack.frames.len() == MAX_FRAMES {
            break;
        }
        let frame = backtrace_frame(frame)?;
        let module = frame.file.map(|file| {
            let name = crate::native::module_name(file);
            let build_id = frame.build_id.filter(|id| !id.is_empty());
            let position = stack
                .modules
                .iter()
                .position(|module| module.name == name && module.code_id.as_deref() == build_id);
            position.unwrap_or_else(|| {
                stack.modules.push(Module {
                    name: name.to_owned(),
                    debug_id: None,
                    code_id: build_id.map(str::to_owned),
                    in_app: in_app(name),
                });
                stack.modules.len() - 1
            })
        });
        stack.frames.push(Frame {
            module,
            offset: format!("0x{:x}", frame.rel_pc),
            trust: if stack.frames.is_empty() {
                Trust::Context
            } else {
                Trust::Unwound
            },
            function: frame
                .function
                .filter(|name| !name.is_empty())
                .map(str::to_owned),
        });
    }
    for mapping in mappings {
        let mut name = None;
        let mut build_id = None;
        for field in Fields::new(mapping) {
            match field.ok()? {
                (7, Value::Bytes(value)) => name = std::str::from_utf8(value).ok(),
                (8, Value::Bytes(value)) => build_id = std::str::from_utf8(value).ok(),
                _ => {}
            }
        }
        if let (Some(name), Some(id)) = (name, build_id.filter(|id| !id.is_empty())) {
            let name = crate::native::module_name(name);
            if stack.modules.len() < super::MAX_MODULES
                && !stack
                    .modules
                    .iter()
                    .any(|module| module.name == name && module.code_id.as_deref() == Some(id))
            {
                stack.modules.push(Module {
                    name: name.into(),
                    debug_id: None,
                    code_id: Some(id.into()),
                    in_app: in_app(name),
                });
            }
        }
    }
    Some(stack.sanitized())
}

/// `SIGSEGV / SEGV_MAPERR`, as minidump readers spell a signal and its code.
fn signal_reason(signal: &[u8]) -> Option<String> {
    let mut name = "";
    let mut code = "";
    for field in Fields::new(signal) {
        match field.ok()? {
            (SIGNAL_NAME, Value::Bytes(text)) => name = std::str::from_utf8(text).ok()?,
            (SIGNAL_CODE_NAME, Value::Bytes(text)) => code = std::str::from_utf8(text).ok()?,
            _ => {}
        }
    }
    Some(match (name, code) {
        ("", _) => String::new(),
        (name, "") => name.to_owned(),
        (name, code) => format!("{name} / {code}"),
    })
}

/// Reads the thread id when the map entry omits its key.
fn thread_id(thread: &[u8]) -> Option<u64> {
    Fields::new(thread).find_map(|field| match field {
        Ok((THREAD_ID, Value::Varint(id))) => Some(id),
        _ => None,
    })
}

struct BacktraceFrame<'a> {
    rel_pc: u64,
    function: Option<&'a str>,
    file: Option<&'a str>,
    build_id: Option<&'a str>,
}

/// Reads only a frame's address, module identity and optional exported function.
fn backtrace_frame(bytes: &[u8]) -> Option<BacktraceFrame<'_>> {
    let mut frame = BacktraceFrame {
        rel_pc: 0,
        function: None,
        file: None,
        build_id: None,
    };
    for field in Fields::new(bytes) {
        match field.ok()? {
            (FRAME_REL_PC, Value::Varint(pc)) => frame.rel_pc = pc,
            (FRAME_FUNCTION, Value::Bytes(text)) => frame.function = std::str::from_utf8(text).ok(),
            (FRAME_FILE, Value::Bytes(text)) => frame.file = std::str::from_utf8(text).ok(),
            (FRAME_BUILD_ID, Value::Bytes(text)) => frame.build_id = std::str::from_utf8(text).ok(),
            _ => {}
        }
    }
    Some(frame)
}

/// One protobuf field's value; fixed-width values are skipped by length only.
enum Value<'a> {
    Varint(u64),
    Bytes(&'a [u8]),
    Fixed,
}

/// The fields of one protobuf message, in order; a malformed field ends with `Err`.
struct Fields<'a> {
    bytes: &'a [u8],
    failed: bool,
}

impl<'a> Fields<'a> {
    /// Starts a forward-only reader over one protobuf message.
    fn new(bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            failed: false,
        }
    }

    /// Consumes a bounded protobuf unsigned integer.
    fn varint(&mut self) -> Option<u64> {
        let mut value = 0_u64;
        for (index, byte) in self.bytes.iter().enumerate().take(10) {
            value |= u64::from(byte & 0x7f) << (7 * index);
            if byte & 0x80 == 0 {
                self.bytes = &self.bytes[index + 1..];
                return Some(value);
            }
        }
        None
    }

    /// Consumes a field body only when every byte is present.
    fn take(&mut self, length: usize) -> Option<&'a [u8]> {
        let bytes = self.bytes;
        let (taken, rest) = (bytes.get(..length)?, bytes.get(length..)?);
        self.bytes = rest;
        Some(taken)
    }

    /// Decodes a supported wire field without interpreting discarded payloads.
    fn field(&mut self) -> Option<(u32, Value<'a>)> {
        let key = self.varint()?;
        let number = u32::try_from(key >> 3).ok()?;
        let value = match key & 7 {
            0 => Value::Varint(self.varint()?),
            1 => {
                self.take(8)?;
                Value::Fixed
            }
            2 => {
                let length = usize::try_from(self.varint()?).ok()?;
                Value::Bytes(self.take(length)?)
            }
            5 => {
                self.take(4)?;
                Value::Fixed
            }
            _ => return None,
        };
        Some((number, value))
    }
}

impl<'a> Iterator for Fields<'a> {
    type Item = Result<(u32, Value<'a>), ()>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.failed || self.bytes.is_empty() {
            return None;
        }
        let field = self.field();
        self.failed = field.is_none();
        Some(field.ok_or(()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Encodes protobuf fields for fixtures.
    #[derive(Default)]
    struct Message(Vec<u8>);

    impl Message {
        /// Adds an unsigned fixture field.
        fn varint(mut self, field: u32, value: u64) -> Self {
            push_varint(&mut self.0, u64::from(field) << 3);
            push_varint(&mut self.0, value);
            self
        }

        /// Adds a byte-string fixture field.
        fn bytes(mut self, field: u32, value: &[u8]) -> Self {
            push_varint(&mut self.0, (u64::from(field) << 3) | 2);
            push_varint(&mut self.0, value.len() as u64);
            self.0.extend_from_slice(value);
            self
        }

        /// Adds an unknown fixed-width field that readers must skip.
        fn fixed64(mut self, field: u32) -> Self {
            push_varint(&mut self.0, (u64::from(field) << 3) | 1);
            self.0.extend_from_slice(&[0; 8]);
            self
        }
    }

    /// Encodes a fixture integer without relying on a protobuf runtime.
    fn push_varint(out: &mut Vec<u8>, mut value: u64) {
        loop {
            let byte = (value & 0x7f) as u8;
            value >>= 7;
            if value == 0 {
                out.push(byte);
                return;
            }
            out.push(byte | 0x80);
        }
    }

    /// Builds one native frame with both private paths and useful build ids.
    fn frame(rel_pc: u64, file: &str, build_id: &str, function: &str) -> Vec<u8> {
        Message::default()
            .varint(FRAME_REL_PC, rel_pc)
            .varint(2, 0x7000_0000 + rel_pc)
            .bytes(FRAME_FUNCTION, function.as_bytes())
            .bytes(FRAME_FILE, file.as_bytes())
            .bytes(FRAME_BUILD_ID, build_id.as_bytes())
            .0
    }

    /// Builds a thread-map entry carrying frames and unrelated memory.
    fn thread(id: u64, frames: &[Vec<u8>]) -> Vec<u8> {
        let mut thread = Message::default()
            .varint(THREAD_ID, id)
            .bytes(2, b"RenderThread")
            .bytes(5, b"raw memory that must not be read");
        for frame in frames {
            thread = thread.bytes(THREAD_BACKTRACE, frame);
        }
        Message::default()
            .varint(ENTRY_KEY, id)
            .bytes(ENTRY_VALUE, &thread.0)
            .0
    }

    /// Builds two threads so selection must follow the crashing thread id.
    fn tombstone() -> Vec<u8> {
        let signal = Message::default()
            .varint(1, 11)
            .bytes(SIGNAL_NAME, b"SIGSEGV")
            .varint(3, 1)
            .bytes(SIGNAL_CODE_NAME, b"SEGV_MAPERR")
            .varint(9, 0x18)
            .0;
        Message::default()
            .varint(1, 1)
            .bytes(2, b"fingerprint")
            .varint(5, 4242)
            .varint(TID, 4250)
            .bytes(9, b"com.example.cinnabar")
            .bytes(SIGNAL, &signal)
            .bytes(14, b"abort message with a server 10.0.0.1")
            .bytes(
                THREADS,
                &thread(4242, &[frame(1, "/system/lib64/libc.so", "aa", "")]),
            )
            .bytes(
                THREADS,
                &thread(
                    4250,
                    &[
                        frame(
                            0x2a0,
                            "/vendor/lib64/hw/vulkan.adreno.so",
                            "beef",
                            "compile",
                        ),
                        frame(0x10, "/vendor/lib64/hw/vulkan.adreno.so", "beef", ""),
                        frame(
                            0x5000,
                            "/data/app/~~x/lib/arm64/libcinnabar.so",
                            "c0ffee",
                            "_RNvrender",
                        ),
                    ],
                ),
            )
            .fixed64(22)
            .0
    }

    #[test]
    fn the_crashing_threads_frames_name_their_libraries_and_build_ids() {
        let stack = stack(&tombstone(), |name| name == "libcinnabar.so").unwrap();
        assert_eq!(stack.reason, "SIGSEGV / SEGV_MAPERR");
        assert_eq!(stack.frames.len(), 3);
        assert_eq!(stack.modules.len(), 2, "one module per library and build");
        assert_eq!(stack.modules[0].name, "vulkan.adreno.so");
        assert_eq!(stack.modules[0].code_id.as_deref(), Some("beef"));
        assert!(!stack.modules[0].in_app && stack.modules[1].in_app);
        assert_eq!(stack.frames[0].trust, Trust::Context);
        assert_eq!(stack.frames[1].trust, Trust::Unwound);
        assert_eq!(stack.describe(&stack.frames[1]), "vulkan.adreno.so+0x10");
        assert_eq!(
            stack.describe(stack.top_in_app().unwrap()),
            "libcinnabar.so!_RNvrender"
        );
        let text = serde_json::to_string(&stack).unwrap();
        assert!(
            !text.contains("10.0.0.1") && !text.contains("data/app"),
            "{text}"
        );
    }

    #[test]
    fn mappings_include_modules_without_frames_without_uploading_their_paths() {
        let mapping = Message::default()
            .bytes(7, b"/data/private/libunused.so")
            .bytes(8, b"abcd")
            .0;
        let bytes = Message(tombstone()).bytes(MAPPINGS, &mapping).0;
        let stack = stack(&bytes, |_| false).unwrap();
        assert_eq!(stack.modules.len(), 3);
        assert_eq!(stack.modules[2].name, "libunused.so");
        assert_eq!(stack.modules[2].code_id.as_deref(), Some("abcd"));
        assert!(
            !serde_json::to_string(&stack)
                .unwrap()
                .contains("/data/private")
        );
    }

    #[test]
    fn truncated_or_foreign_bytes_are_not_a_stack() {
        let bytes = tombstone();
        assert!(stack(&bytes[..bytes.len() / 2], |_| false).is_none());
        assert!(stack(b"PK\x03\x04 not a protobuf", |_| false).is_none());
        assert!(stack(&[], |_| false).is_none());
    }
}
