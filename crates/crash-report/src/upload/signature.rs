//! Grouping signatures: what one cause of crashes has in common across reports, without the
//! numbers, addresses and names that differ between them.

use crate::{report::Kind, stack::Stack};

/// Longest signature.
const MAX_SIGNATURE: usize = 240;
/// Longest message part of a signature.
const MAX_MESSAGE: usize = 120;

/// The signature of a crash from its redacted message, the core's redacted crash output and
/// its stack: the panic site, the error line, or the signal with the top in-app frame.
pub fn signature(kind: Kind, message: &str, core_crash: &str, stack: Option<&Stack>) -> String {
    let signature = match kind {
        Kind::Panic => panic(message),
        Kind::Native => native(message, stack),
        Kind::Core => {
            let line = core_crash
                .lines()
                .find(|line| line.starts_with("panic:") || line.starts_with("fatal error:"))
                .unwrap_or(message);
            format!("core: {}", normalize(line))
        }
        Kind::Abort => format!("abort: {}", normalize(message)),
        Kind::Hang => format!("hang: {}", normalize(message)),
        Kind::Startup => format!("startup: {}", normalize(message)),
        Kind::Runtime => format!("runtime: {}", normalize(message)),
    };
    signature.chars().take(MAX_SIGNATURE).collect()
}

/// `panic: <file:line>: <message>` from `panicked at <file:line:column>:\n<message>`.
fn panic(message: &str) -> String {
    let (head, rest) = message.split_once('\n').unwrap_or((message, ""));
    let Some(at) = head.strip_prefix("panicked at ") else {
        return format!("panic: {}", normalize(message));
    };
    let at = at.trim_end_matches(':');
    // Drop the column: rustc may move it between builds of the same line.
    let site = match at.rsplit_once(':') {
        Some((file_line, column)) if column.bytes().all(|byte| byte.is_ascii_digit()) => file_line,
        _ => at,
    };
    let first = rest
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or_default();
    let site = source_path(site);
    if first.is_empty() {
        format!("panic: {site}")
    } else {
        format!("panic: {site}: {}", normalize(first))
    }
}

/// `native: <reason> in <faulting module> via <top in-app frame>`.
fn native(message: &str, stack: Option<&Stack>) -> String {
    let Some(stack) = stack else {
        return format!("native: {}", normalize(message));
    };
    let reason = if stack.reason.is_empty() {
        normalize(message)
    } else {
        stack.reason.clone()
    };
    if stack.frames.is_empty() {
        return format!("native: {reason}");
    }
    let faulting = &stack.frames[0];
    let in_app = stack.top_in_app();
    let mut out = format!("native: {reason}");
    match stack.module(faulting) {
        Some(module) if module.in_app => {
            out.push_str(&format!(" in {}", stack.describe(faulting)));
            return out;
        }
        Some(module) => out.push_str(&format!(" in {}", module.name)),
        None => out.push_str(" in unknown code"),
    }
    if let Some(frame) = in_app {
        out.push_str(&format!(" via {}", stack.describe(frame)));
    }
    out
}

/// A source path relative to its crate: registry and checkout prefixes and absolute build
/// directories removed, `\` as `/`.
fn source_path(path: &str) -> String {
    let path = path.replace('\\', "/");
    for marker in ["/registry/src/", "/git/checkouts/"] {
        if let Some((_, rest)) = path.split_once(marker) {
            // Skip the index or checkout directory itself.
            let rest = rest.split_once('/').map_or(rest, |(_, rest)| rest);
            return rest.to_owned();
        }
    }
    for marker in ["/crates/", "/app/", "/tools/", "/packaging/", "/rustc/"] {
        if let Some(index) = path.rfind(marker) {
            return path[index + 1..].to_owned();
        }
    }
    path.trim_start_matches('/').to_owned()
}

/// The first line of `text` with numbers and hex addresses replaced and spaces collapsed.
fn normalize(text: &str) -> String {
    let line = text
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or_default();
    let mut out = String::new();
    let mut chars = line.trim().chars().peekable();
    while let Some(character) = chars.next() {
        if character == '0'
            && chars
                .peek()
                .is_some_and(|next| *next == 'x' || *next == 'X')
        {
            chars.next();
            while chars.peek().is_some_and(char::is_ascii_hexdigit) {
                chars.next();
            }
            out.push_str("0x_");
        } else if character.is_ascii_digit() {
            while chars.peek().is_some_and(char::is_ascii_digit) {
                chars.next();
            }
            out.push('N');
        } else if character.is_whitespace() {
            if !out.ends_with(' ') {
                out.push(' ');
            }
        } else if !character.is_control() {
            out.push(character);
        }
    }
    out.chars().take(MAX_MESSAGE).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stack::{Frame, Module, Trust};

    #[test]
    fn panics_group_by_site_and_message_shape() {
        let a = signature(
            Kind::Panic,
            "panicked at crates/render/src/atlas.rs:120:17:\nindex out of bounds: the len is 3 but the index is 5",
            "",
            None,
        );
        let b = signature(
            Kind::Panic,
            "panicked at crates/render/src/atlas.rs:120:9:\nindex out of bounds: the len is 30 but the index is 52",
            "",
            None,
        );
        assert_eq!(a, b);
        assert_eq!(
            a,
            "panic: crates/render/src/atlas.rs:120: index out of bounds: the len is N but the index is N"
        );
    }

    #[test]
    fn dependency_panics_drop_build_machine_paths() {
        let signature = signature(
            Kind::Panic,
            "panicked at /home/runner/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/wgpu-30.0.1/src/backend/wgpu_core.rs:1234:5:\nwgpu error: Validation Error",
            "",
            None,
        );
        assert_eq!(
            signature,
            "panic: wgpu-30.0.1/src/backend/wgpu_core.rs:1234: wgpu error: Validation Error"
        );
        assert_eq!(
            source_path(r"C:\a\_work\cinnabar\crates\ui\src\lib.rs:5"),
            "crates/ui/src/lib.rs:5"
        );
    }

    /// Models a driver fault followed by a frame in the application.
    fn stack(faulting_in_app: bool) -> Stack {
        Stack {
            reason: "SIGSEGV / SEGV_MAPERR".into(),
            modules: vec![
                Module {
                    name: "libGLES_mali.so".into(),
                    debug_id: None,
                    code_id: Some("aa".into()),
                    in_app: false,
                },
                Module {
                    name: "libcinnabar.so".into(),
                    debug_id: None,
                    code_id: Some("bb".into()),
                    in_app: true,
                },
            ],
            frames: vec![
                Frame {
                    module: Some(usize::from(faulting_in_app)),
                    offset: "0x10".into(),
                    trust: Trust::Context,
                    function: None,
                },
                Frame {
                    module: Some(1),
                    offset: "0x20".into(),
                    trust: Trust::Unwound,
                    function: Some("create_pipeline".into()),
                },
            ],
        }
    }

    #[test]
    fn native_crashes_name_the_signal_driver_and_top_in_app_frame() {
        assert_eq!(
            signature(Kind::Native, "Native crash", "", Some(&stack(false))),
            "native: SIGSEGV / SEGV_MAPERR in libGLES_mali.so via libcinnabar.so!create_pipeline"
        );
        assert_eq!(
            signature(Kind::Native, "Native crash", "", Some(&stack(true))),
            "native: SIGSEGV / SEGV_MAPERR in libcinnabar.so+0x10"
        );
        assert_eq!(
            signature(Kind::Native, "EXCEPTION_ACCESS_VIOLATION_WRITE", "", None),
            "native: EXCEPTION_ACCESS_VIOLATION_WRITE"
        );
    }

    #[test]
    fn other_kinds_use_their_normalized_error_line() {
        assert_eq!(
            signature(
                Kind::Runtime,
                "Quitting the application due to OutOfMemory RenderError\nmore",
                "",
                None
            ),
            "runtime: Quitting the application due to OutOfMemory RenderError"
        );
        assert_eq!(
            signature(
                Kind::Abort,
                "Exited with code 0xC0000409 without a crash dump",
                "",
                None
            ),
            "abort: Exited with code 0x_ without a crash dump"
        );
        assert_eq!(
            signature(
                Kind::Core,
                "core exited",
                "goroutine 1\npanic: runtime error: index out of range [5] with length 3\n",
                None
            ),
            "core: panic: runtime error: index out of range [N] with length N"
        );
    }
}
