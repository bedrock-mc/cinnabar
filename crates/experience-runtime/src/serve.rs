//! The stdio session of one Experience. The adapter's first frame loads it; each callback frame
//! then gets a result with the same `seq`, until `shutdown` or the end of input. Only frames go
//! to the output; logs go to stderr.

use std::fmt::Display;
use std::io::{self, ErrorKind, Read, Write};
use std::path::Path;

use anyhow::Result;
use wasmtime::Engine;

use crate::callback;
use crate::load::{self, EpochTicker, Loaded};
use crate::manifest::Manifest;
use crate::protocol::{
    BlockDef, Call, FailKind, ItemDef, Outcome, PROTOCOL_VERSION, Request, Response, ServerItem,
    bounded_reason, read_frame, write_frame,
};

/// The session ended with `shutdown`, or with the end of input between frames.
pub const EXIT_OK: i32 = 0;
/// The artifact did not load, and the adapter was answered `load_failed`.
pub const EXIT_LOAD_FAILED: i32 = 1;
/// A frame did not decode or could not be written, or was not the request expected at its point
/// in the session.
pub const EXIT_PROTOCOL: i32 = 2;

/// Why a result that is too large for a frame failed instead.
const OVERSIZED_RESULT: &str = "result exceeds the frame limit";
/// Why a load failed whose `loaded` answer is too large for a frame.
const OVERSIZED_LOADED: &str = "the loaded answer exceeds the frame limit";
/// Why a load failed whose `load_failed` answer is too large for a frame.
const OVERSIZED_LOAD_FAILED: &str = "the load failure exceeds the frame limit";

/// Serves one session, reading requests from `input` and writing responses to `output`, and
/// returns the process exit code. With `report_fuel`, each callback logs the fuel it consumed.
/// An error is a frame that could not be written; the binary then exits with [`EXIT_PROTOCOL`].
pub fn serve(mut input: impl Read, mut output: impl Write, report_fuel: bool) -> Result<i32> {
    let (dir, items) = match read_frame(&mut input) {
        Ok(Some(Request::Load { dir, items })) => (dir, items),
        Ok(Some(Request::Callback { .. } | Request::Shutdown {})) => {
            return Ok(protocol_error("the first frame is not load"));
        }
        Ok(None) => return Ok(EXIT_OK),
        Err(error) => return Ok(protocol_error(error)),
    };
    // The ticker drives every deadline, so it lives as long as the session.
    let (engine, _ticker, loaded) = match start(Path::new(&dir), items) {
        Ok(started) => started,
        Err(error) => {
            answer_load_failed(&mut output, format!("{error:#}"))?;
            return Ok(EXIT_LOAD_FAILED);
        }
    };
    if !answer_loaded(
        &mut output,
        &loaded.manifest,
        &loaded.blocks,
        &loaded.items,
        loaded.focus,
    )? {
        return Ok(EXIT_LOAD_FAILED);
    }
    loop {
        let request = match read_frame(&mut input) {
            Ok(Some(request)) => request,
            Ok(None) => return Ok(EXIT_OK),
            Err(error) => return Ok(protocol_error(error)),
        };
        let (seq, call) = match &request {
            Request::Callback { seq, call, .. } => (*seq, call),
            Request::Shutdown {} => return Ok(EXIT_OK),
            Request::Load { .. } => return Ok(protocol_error("load after the artifact loaded")),
        };
        let (outcome, fuel) = callback::run_metered(&engine, &loaded, &request);
        if report_fuel {
            eprintln!("fuel {} {} {fuel}", loaded.manifest.id, call_kind(call));
        }
        answer(&mut output, seq, outcome)?;
    }
}

/// The engine with its ticker, and the artifact in `dir` loaded on it, which may make stacks of
/// the server's `items`.
fn start(dir: &Path, items: Vec<ServerItem>) -> Result<(Engine, EpochTicker, Loaded)> {
    let (engine, ticker) = load::engine()?;
    let loaded = load::load(&engine, dir)?.with_server_items(items)?;
    Ok((engine, ticker, loaded))
}

/// Answers a load with what loaded: the manifest's id and version, the blocks and items, and
/// whether its world takes a `focus`. An answer too large for a frame fails the load instead.
/// Returns whether the load stands.
fn answer_loaded(
    output: &mut impl Write,
    manifest: &Manifest,
    blocks: &[BlockDef],
    items: &[ItemDef],
    focus: bool,
) -> io::Result<bool> {
    let response = Response::Loaded {
        protocol: PROTOCOL_VERSION,
        id: manifest.id.clone(),
        version: manifest.version.clone(),
        blocks: blocks.to_vec(),
        items: items.to_vec(),
        focus,
    };
    let failed = || Response::LoadFailed {
        reason: OVERSIZED_LOADED.to_owned(),
    };
    write_or(output, &response, failed)
}

/// Answers a failed load with `reason` cut by [`bounded_reason`], and logs the same.
fn answer_load_failed(output: &mut impl Write, reason: String) -> io::Result<()> {
    let reason = bounded_reason(reason);
    eprintln!("serve: load failed: {reason}");
    let fixed = || Response::LoadFailed {
        reason: OVERSIZED_LOAD_FAILED.to_owned(),
    };
    write_or(output, &Response::LoadFailed { reason }, fixed)?;
    Ok(())
}

/// Logs why the session ends without an answer, and returns [`EXIT_PROTOCOL`].
fn protocol_error(error: impl Display) -> i32 {
    eprintln!("serve: protocol error: {error}");
    EXIT_PROTOCOL
}

/// Writes the result of callback `seq`. A result too large for a frame fails as a limit
/// instead, so the callback is still answered.
fn answer(output: &mut impl Write, seq: u64, outcome: Outcome) -> io::Result<()> {
    let failed = || Response::Result {
        seq,
        outcome: Outcome::Failed {
            kind: FailKind::Limit,
            reason: OVERSIZED_RESULT.to_owned(),
        },
    };
    write_or(output, &Response::Result { seq, outcome }, failed)?;
    Ok(())
}

/// Writes `response`, or `fallback()` when `response` is too large for a frame, which
/// [`write_frame`] refuses before it writes anything. Returns whether `response` was written.
fn write_or(
    output: &mut impl Write,
    response: &Response,
    fallback: impl FnOnce() -> Response,
) -> io::Result<bool> {
    match write_frame(output, response) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == ErrorKind::InvalidInput => {
            eprintln!("serve: answer refused: {error}");
            write_frame(output, &fallback())?;
            Ok(false)
        }
        Err(error) => Err(error),
    }
}

/// The `type` of `call` in the protocol.
fn call_kind(call: &Call) -> &'static str {
    match call {
        Call::Place { .. } => "place",
        Call::Break { .. } => "break",
        Call::Interact { .. } => "interact",
        Call::Neighbor { .. } => "neighbor",
        Call::ClientMessage { .. } => "client_message",
        Call::Epoch { .. } => "epoch",
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{OVERSIZED_LOADED, OVERSIZED_RESULT, answer, answer_loaded};
    use crate::limits::MAX_FRAME_BYTES;
    use crate::manifest::Manifest;
    use crate::protocol::{BlockDef, FailKind, Mining, Op, Outcome, Response, read_frame};

    /// A `loaded` answer too large for a frame fails the load instead, and nothing of it is
    /// written.
    #[test]
    fn oversized_loaded_fails_the_load() {
        let manifest = Manifest {
            id: "probe".to_owned(),
            version: "0.1.0".to_owned(),
            api: "0.1".to_owned(),
            data_schema: 1,
            files: BTreeMap::new(),
        };
        let block = BlockDef {
            id: "probe:counter".to_owned(),
            display_name: "x".repeat(MAX_FRAME_BYTES),
            textures: Vec::new(),
            mining: Mining::Unbreakable {},
            states: Vec::new(),
            placement: Vec::new(),
            visual: None,
            permutations: Vec::new(),
            network: false,
        };
        let mut output = Vec::new();
        assert!(!answer_loaded(&mut output, &manifest, &[block], &[], false).unwrap());
        let mut frames = output.as_slice();
        let failed = Response::LoadFailed {
            reason: OVERSIZED_LOADED.to_owned(),
        };
        assert_eq!(read_frame(&mut frames).unwrap(), Some(failed));
        assert_eq!(read_frame::<Response>(&mut frames).unwrap(), None);
    }

    /// A result too large for a frame is answered as a `limit` failure instead, and nothing of
    /// it is written.
    #[test]
    fn oversized_result_fails_with_limit() {
        let tell = Op::Tell {
            player: String::new(),
            text: "x".repeat(MAX_FRAME_BYTES),
        };
        let mut output = Vec::new();
        answer(&mut output, 7, Outcome::Committed { ops: vec![tell] }).unwrap();
        let mut frames = output.as_slice();
        let failed = Response::Result {
            seq: 7,
            outcome: Outcome::Failed {
                kind: FailKind::Limit,
                reason: OVERSIZED_RESULT.to_owned(),
            },
        };
        assert_eq!(read_frame(&mut frames).unwrap(), Some(failed));
        assert_eq!(read_frame::<Response>(&mut frames).unwrap(), None);
    }
}
