//! The real media helper process: one decode over stdio under its memory ceiling.

#[cfg(all(feature = "media", not(windows)))]
use server_experience::media::Output;
use server_experience::media::{
    descriptor::{Descriptor, Profile},
    ipc::{self, Reply, Request, Start},
    worker::HELPER_COMMAND,
};
use std::process::{Command, Stdio};

const FIXTURE: &[u8] = include_bytes!("../../server-experience/src/media/testdata/fixture.webm");
const CHUNK: usize = 64 * 1024;

fn descriptor() -> Descriptor {
    Descriptor {
        id: "fixture.media".into(),
        timeline: "fixture.timeline".into(),
        profile: Profile::WebmAv1OpusBt709,
        url: "https://example.com/media.webm".into(),
        bytes: FIXTURE.len() as u64,
        chunk_bytes: CHUNK as u32,
        chunk_hashes: FIXTURE
            .chunks(CHUNK)
            .map(server_experience::crypto::digest)
            .collect(),
        sha256: server_experience::crypto::digest(FIXTURE),
        width: 64,
        height: 64,
        fps: 10,
        duration_us: 500_000,
        audio_channels: 1,
        poster: "poster.png".into(),
    }
}

/// Runs one session, serving chunk requests from the fixture; returns every reply.
fn session(developer: bool) -> (Vec<Reply>, std::process::ExitStatus) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_cinnabar-media-helper"));
    command.arg(HELPER_COMMAND).env_clear();
    if developer {
        command.env(server_experience::policy::DEVELOPER_ENV, "1");
    }
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let mut output = child.stdout.take().unwrap();
    let _ = ipc::write_request(
        &mut input,
        &Request::Start(Box::new(Start {
            descriptor: descriptor(),
            start_us: 0,
        })),
    );
    let mut replies = Vec::new();
    while let Ok(reply) = ipc::read_reply(&mut output, 1) {
        if let Reply::Need(index) = reply {
            let start = index as usize * CHUNK;
            let bytes = FIXTURE[start..(start + CHUNK).min(FIXTURE.len())].to_vec();
            ipc::write_request(&mut input, &Request::Chunk { index, bytes }).unwrap();
            continue;
        }
        replies.push(reply);
    }
    drop(input);
    (replies, child.wait().unwrap())
}

#[test]
fn helper_refuses_to_run_without_the_developer_switch() {
    let (replies, status) = session(false);
    assert!(!status.success());
    assert!(replies.is_empty());
}

#[cfg(windows)]
#[test]
fn windows_helper_refuses_before_decoding_without_a_memory_ceiling() {
    let (replies, status) = session(true);
    assert!(!status.success());
    assert!(replies.is_empty());
}

#[cfg(all(not(feature = "media"), not(windows)))]
#[test]
fn helper_without_the_decoder_reports_why_instead_of_ending_cleanly() {
    let (replies, status) = session(true);
    assert!(!status.success());
    assert!(matches!(
        replies.as_slice(),
        [Reply::Error(text)] if text.contains("developer-media")
    ));
}

#[cfg(all(feature = "media", not(windows)))]
#[test]
fn contained_helper_decodes_the_fixture_and_ends() {
    let (replies, status) = session(true);
    assert!(status.success(), "{replies:?}");
    let frames = replies
        .iter()
        .filter(|reply| matches!(reply, Reply::Output(Output::Video(frame)) if frame.width == 64))
        .count();
    assert_eq!(frames, 5);
    assert!(
        replies
            .iter()
            .any(|reply| matches!(reply, Reply::Output(Output::Audio(_))))
    );
    assert!(matches!(replies.last(), Some(Reply::Output(Output::End))));
}
