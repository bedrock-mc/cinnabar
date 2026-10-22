//! Offline symbol caches and symbolication for the address-only crash upload payload.

use crate::{DevtoolError, symbolicate::SymbolRoot};
use crash_report::{stack::Trust, upload::Payload};
use std::{
    fs,
    path::{Path, PathBuf},
};
use symbolic::{
    debuginfo::Archive,
    symcache::{SymCache, SymCacheConverter},
};

/// Builds build-id caches, or resolves one downloaded report against those caches.
pub(crate) fn run(args: &[String]) -> Result<(), DevtoolError> {
    match args {
        [command, input, output] if command == "index" => {
            let count = index(Path::new(input), Path::new(output))?;
            println!("Indexed {count} symbol objects");
            Ok(())
        }
        [command, input, symbols] if command == "resolve" => {
            let bytes = fs::read(input).map_err(error)?;
            let payload: Payload = serde_json::from_slice(&bytes).map_err(error)?;
            println!("{}", serde_json::to_string_pretty(&resolve(&payload, Path::new(symbols))?).map_err(error)?);
            Ok(())
        }
        _ => Err(DevtoolError::Usage("crash-symbols index <symbols dir|zip|object> <cache dir> | crash-symbols resolve <report.json> <cache dir>".into())),
    }
}

/// Converts local debug objects to caches named by debug id and full code build id.
/// A cache is never selected by filename or release version alone.
fn index(input: &Path, output: &Path) -> Result<usize, DevtoolError> {
    let root = if input.extension().is_some_and(|ext| ext == "zip") {
        Some(SymbolRoot::open(input)?)
    } else {
        None
    };
    let mut pending = vec![(root.as_ref().map_or(input, SymbolRoot::path).to_owned(), 0)];
    let mut count = 0;
    while let Some((path, depth)) = pending.pop() {
        if path.is_dir() {
            if depth > 16 {
                return Err(error("symbol directory is nested too deeply"));
            }
            for entry in fs::read_dir(path).map_err(error)? {
                pending.push((entry.map_err(error)?.path(), depth + 1));
            }
            continue;
        }
        let bytes = fs::read(&path).map_err(error)?;
        let Ok(archive) = Archive::parse(&bytes) else {
            continue;
        };
        for object in archive.objects() {
            let object = object.map_err(error)?;
            if !object.has_debug_info() {
                continue;
            }
            let mut converter = SymCacheConverter::new();
            converter.process_object(&object).map_err(error)?;
            let mut cache = Vec::new();
            converter.serialize(&mut cache).map_err(error)?;
            let mut keys = vec![("debug", object.debug_id().breakpad().to_string())];
            if let Some(code) = object.code_id() {
                keys.push(("code", code.to_string()));
            }
            for (kind, id) in keys {
                let path = cache_path(output, kind, &id)
                    .ok_or_else(|| error("invalid symbol build id"))?;
                fs::create_dir_all(path.parent().expect("cache path has a parent"))
                    .map_err(error)?;
                fs::write(path, &cache).map_err(error)?;
            }
            count += 1;
        }
    }
    if count == 0 {
        return Err(error("no debug objects found"));
    }
    Ok(count)
}

/// Resolves each recorded frame only against its module's matching build id; misses stay raw.
fn resolve(payload: &Payload, symbols: &Path) -> Result<serde_json::Value, DevtoolError> {
    let mut out = serde_json::json!({ "report_id": payload.report_id, "signature": payload.signature, "frames": [] });
    let frames = out["frames"].as_array_mut().expect("frames is an array");
    let Some(stack) = &payload.stack else {
        return Ok(out);
    };
    for frame in &stack.frames {
        let mut result = serde_json::json!({ "raw": stack.describe(frame), "trust": frame.trust, "symbols": [] });
        if let Some(module) = stack.module(frame) {
            let candidates = [
                ("debug", module.debug_id.as_deref()),
                ("code", module.code_id.as_deref()),
            ];
            for (kind, id) in candidates {
                let Some(path) = id.and_then(|id| cache_path(symbols, kind, id)) else {
                    continue;
                };
                let Ok(bytes) = fs::read(path) else { continue };
                let cache = SymCache::parse(&bytes).map_err(error)?;
                if kind == "debug"
                    && module.debug_id.as_deref().map(str::to_ascii_lowercase)
                        != Some(cache.debug_id().breakpad().to_string().to_ascii_lowercase())
                {
                    continue;
                }
                let offset = u64::from_str_radix(frame.offset.trim_start_matches("0x"), 16)
                    .map_err(error)?;
                // Scanned words are raw return addresses. Platform-unwound PCs already name
                // the calling instruction and must not be adjusted again.
                let address = if frame.trust == Trust::Scan {
                    offset.saturating_sub(1)
                } else {
                    offset
                };
                let locations: Vec<_> = cache.lookup(address).map(|location| serde_json::json!({
                    "function": location.function().name(),
                    "file": location.file().map(|file| file.full_path()), "line": location.line(),
                })).collect();
                result["symbols"] = serde_json::to_value(locations).map_err(error)?;
                break;
            }
        }
        frames.push(result);
    }
    Ok(out)
}

/// A safe, normalized build-id key shared by index creation and lookup.
fn cache_path(root: &Path, kind: &str, id: &str) -> Option<PathBuf> {
    (!id.is_empty() && id.len() <= 80 && id.bytes().all(|b| b.is_ascii_hexdigit())).then(|| {
        root.join(kind)
            .join(format!("{}.symcache", id.to_ascii_lowercase()))
    })
}

/// Wraps a local object or cache error without sending it over the network.
fn error(error: impl std::fmt::Display) -> DevtoolError {
    DevtoolError::Symbolicate(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_keys_cannot_escape_the_symbol_root() {
        let root = Path::new("symbols");
        assert!(cache_path(root, "debug", "../../secret").is_none());
        assert!(cache_path(root, "code", "").is_none());
        assert_eq!(
            cache_path(root, "code", "ABCDEF"),
            Some(root.join("code/abcdef.symcache"))
        );
    }

    #[test]
    fn breakpad_fixture_indexes_and_resolves_by_build_id() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("fixture.sym");
        let id = "0123456789ABCDEF0123456789ABCDEF0";
        fs::write(&source, format!("MODULE Linux x86_64 {id} fixture\nFILE 0 fixture.rs\nFUNC 100 20 0 fixture_crash\n100 10 41 0\n110 10 42 0\n")).unwrap();
        let symbols = dir.path().join("cache");
        assert_eq!(index(&source, &symbols).unwrap(), 1);
        let report = crash_report::report::Report::new(
            crash_report::report::Kind::Native,
            "SIGSEGV".into(),
            &crash_report::report::Identity {
                session_id: String::new(),
                started_at_ms: 0,
                release: "test".into(),
                commit: "test".into(),
                build_profile: "test".into(),
            },
            0,
        );
        let mut payload = crash_report::upload::payload(
            &report,
            "",
            "",
            Some(crash_report::stack::Stack {
                reason: "SIGSEGV".into(),
                modules: vec![crash_report::stack::Module {
                    name: "fixture".into(),
                    debug_id: Some(id.into()),
                    code_id: None,
                    in_app: true,
                }],
                frames: vec![crash_report::stack::Frame {
                    module: Some(0),
                    offset: "0x110".into(),
                    trust: Trust::Context,
                    function: None,
                }],
            }),
            crash_report::upload::Ids {
                install: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                report: "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            },
            None,
        )
        .unwrap();
        let found = resolve(&payload, &symbols).unwrap();
        assert_eq!(
            found["frames"][0]["symbols"][0]["function"],
            "fixture_crash"
        );
        assert_eq!(found["frames"][0]["symbols"][0]["line"], 42);
        for (trust, line) in [(Trust::Unwound, 42), (Trust::Scan, 41)] {
            payload.stack.as_mut().unwrap().frames[0].trust = trust;
            assert_eq!(
                resolve(&payload, &symbols).unwrap()["frames"][0]["symbols"][0]["line"],
                line,
                "{trust:?} selected the wrong instruction at a line boundary"
            );
        }
        payload.stack.as_mut().unwrap().modules[0].debug_id =
            Some("ffffffffffffffffffffffffffffffff0".into());
        assert_eq!(
            resolve(&payload, &symbols).unwrap()["frames"][0]["symbols"],
            serde_json::json!([])
        );
    }
}
