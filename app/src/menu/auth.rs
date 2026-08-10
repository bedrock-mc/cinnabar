use std::{
    io::Read,
    path::Path,
    process::{Child, Command, Stdio},
    thread::{self, JoinHandle},
};

use anyhow::{Context, Result};
use crossbeam_channel::{Receiver, Sender, TryRecvError, bounded};
use serde::Deserialize;
use url::Url;

const MAX_LINE_BYTES: usize = 4096;
const EVENT_CAPACITY: usize = 8;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum AuthState {
    SignedOut,
    Checking,
    AwaitingCode { uri: String, code: String },
    Authenticated,
    Failed(String),
}

#[derive(Debug, Deserialize)]
#[serde(tag = "event", deny_unknown_fields)]
enum WireEvent {
    #[serde(rename = "checking_cache")]
    CheckingCache { v: u8 },
    #[serde(rename = "device_code")]
    DeviceCode {
        v: u8,
        verification_uri: String,
        user_code: String,
    },
    #[serde(rename = "authenticated")]
    Authenticated { v: u8, method: AuthMethod },
    #[serde(rename = "error")]
    Error {
        v: u8,
        stage: String,
        message: String,
    },
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum AuthMethod {
    Cached,
    DeviceCode,
}

#[derive(Debug)]
enum ReaderMessage {
    Line(Vec<u8>),
    Invalid(&'static str),
    End,
}

#[derive(Default)]
struct LineDecoder {
    pending: Vec<u8>,
    discarding: bool,
}

impl LineDecoder {
    fn push(&mut self, bytes: &[u8], sender: &Sender<ReaderMessage>) -> bool {
        for &byte in bytes {
            if self.discarding {
                if byte == b'\n' {
                    self.discarding = false;
                }
                continue;
            }
            if byte == b'\n' {
                if self.pending.last() == Some(&b'\r') {
                    self.pending.pop();
                }
                let line = std::mem::take(&mut self.pending);
                if sender.try_send(ReaderMessage::Line(line)).is_err() {
                    return false;
                }
            } else if self.pending.len() == MAX_LINE_BYTES {
                self.pending.clear();
                self.discarding = true;
                if sender
                    .try_send(ReaderMessage::Invalid(
                        "authentication event exceeds 4096 bytes",
                    ))
                    .is_err()
                {
                    return false;
                }
            } else {
                self.pending.push(byte);
            }
        }
        true
    }

    fn finish(&mut self, sender: &Sender<ReaderMessage>) {
        if !self.discarding && !self.pending.is_empty() {
            let _ = sender.try_send(ReaderMessage::Invalid(
                "authentication event stream ended mid-line",
            ));
        }
        let _ = sender.try_send(ReaderMessage::End);
    }
}

#[derive(Debug)]
pub(crate) struct AuthSupervisor {
    child: Child,
    receiver: Receiver<ReaderMessage>,
    reader: Option<JoinHandle<()>>,
    state: AuthState,
    terminal: bool,
    checking_seen: bool,
}

impl AuthSupervisor {
    pub(crate) fn spawn(executable: &Path, cache: &Path) -> Result<Self> {
        let child = Command::new(executable)
            .arg("-auth-events")
            .arg("-auth-cache")
            .arg(cache)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .with_context(|| format!("start sign-in helper {}", executable.display()))?;
        Self::from_child(child)
    }

    fn from_child(mut child: Child) -> Result<Self> {
        let stdout = child
            .stdout
            .take()
            .context("sign-in helper stdout was not piped")?;
        let (sender, receiver) = bounded(EVENT_CAPACITY);
        let reader = thread::spawn(move || read_events(stdout, sender));
        Ok(Self {
            child,
            receiver,
            reader: Some(reader),
            state: AuthState::Checking,
            terminal: false,
            checking_seen: false,
        })
    }

    pub(crate) fn state(&self) -> &AuthState {
        &self.state
    }

    pub(crate) fn poll(&mut self) {
        for _ in 0..EVENT_CAPACITY {
            match self.receiver.try_recv() {
                Ok(ReaderMessage::Line(line)) => self.apply_line(&line),
                Ok(ReaderMessage::Invalid(message)) => self.fail(message),
                Ok(ReaderMessage::End) => {
                    if !self.terminal {
                        self.fail("Sign-in helper stopped before authentication completed.");
                    }
                    break;
                }
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => break,
            }
        }
        if !self.terminal && matches!(self.child.try_wait(), Ok(Some(_))) {
            self.fail("Sign-in helper stopped before authentication completed.");
        }
    }

    fn apply_line(&mut self, line: &[u8]) {
        apply_event(
            &mut self.state,
            &mut self.terminal,
            &mut self.checking_seen,
            line,
        );
    }

    fn fail(&mut self, message: &str) {
        fail_state(&mut self.state, &mut self.terminal, message);
    }

    pub(crate) fn cancel(&mut self) {
        if !self.terminal {
            self.state = AuthState::SignedOut;
            self.terminal = true;
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

fn apply_event(state: &mut AuthState, terminal: &mut bool, checking_seen: &mut bool, line: &[u8]) {
    if *terminal {
        fail_state(
            state,
            terminal,
            "Sign-in helper sent an event after completion.",
        );
        return;
    }
    let Ok(event) = serde_json::from_slice::<WireEvent>(line) else {
        fail_state(state, terminal, "Sign-in helper returned malformed data.");
        return;
    };
    let version = match &event {
        WireEvent::CheckingCache { v }
        | WireEvent::DeviceCode { v, .. }
        | WireEvent::Authenticated { v, .. }
        | WireEvent::Error { v, .. } => *v,
    };
    if version != 1 {
        fail_state(
            state,
            terminal,
            "Sign-in helper returned an unsupported event version.",
        );
        return;
    }
    match event {
        WireEvent::CheckingCache { .. } if *state == AuthState::Checking && !*checking_seen => {
            *checking_seen = true;
        }
        WireEvent::DeviceCode {
            verification_uri,
            user_code,
            ..
        } if *state == AuthState::Checking && *checking_seen => {
            if !valid_https_uri(&verification_uri) || !valid_code(&user_code) {
                fail_state(
                    state,
                    terminal,
                    "Sign-in helper returned an unsafe sign-in prompt.",
                );
            } else {
                *state = AuthState::AwaitingCode {
                    uri: verification_uri,
                    code: user_code,
                };
            }
        }
        WireEvent::Authenticated { method, .. }
            if (*checking_seen
                && *state == AuthState::Checking
                && matches!(method, AuthMethod::Cached))
                || (matches!(state, AuthState::AwaitingCode { .. })
                    && matches!(method, AuthMethod::DeviceCode)) =>
        {
            *state = AuthState::Authenticated;
            *terminal = true;
        }
        WireEvent::Error { stage, message, .. } => {
            let _ = stage;
            fail_state(state, terminal, &safe_message(&message));
        }
        _ => fail_state(
            state,
            terminal,
            "Sign-in helper returned events out of order.",
        ),
    }
}

fn fail_state(state: &mut AuthState, terminal: &mut bool, message: &str) {
    *state = AuthState::Failed(safe_message(message));
    *terminal = true;
}

impl Drop for AuthSupervisor {
    fn drop(&mut self) {
        self.cancel();
    }
}

fn read_events(mut reader: impl Read, sender: Sender<ReaderMessage>) {
    let mut decoder = LineDecoder::default();
    let mut chunk = [0u8; 512];
    loop {
        match reader.read(&mut chunk) {
            Ok(0) => break,
            Ok(count) if decoder.push(&chunk[..count], &sender) => {}
            Ok(_) | Err(_) => break,
        }
    }
    decoder.finish(&sender);
}

fn valid_https_uri(value: &str) -> bool {
    Url::parse(value).is_ok_and(|uri| {
        uri.scheme() == "https"
            && uri.host_str().is_some()
            && uri.username().is_empty()
            && uri.password().is_none()
    })
}

fn valid_code(value: &str) -> bool {
    !value.is_empty() && value.len() <= 64 && value.bytes().all(|byte| byte.is_ascii_graphic())
}

fn safe_message(value: &str) -> String {
    let printable: String = value
        .chars()
        .filter(|character| !character.is_control())
        .take(160)
        .collect();
    if printable.is_empty() {
        "Microsoft sign-in failed. Try again.".to_owned()
    } else {
        printable
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn channel() -> (Sender<ReaderMessage>, Receiver<ReaderMessage>) {
        bounded(EVENT_CAPACITY)
    }

    #[test]
    fn fragmented_lines_are_reassembled() {
        let (sender, receiver) = channel();
        let mut decoder = LineDecoder::default();
        assert!(decoder.push(br#"{"v":1,"event":"check"#, &sender));
        assert!(decoder.push(b"ing_cache\"}\r\n", &sender));
        let ReaderMessage::Line(line) = receiver.try_recv().unwrap() else {
            panic!("expected line")
        };
        assert_eq!(line, br#"{"v":1,"event":"checking_cache"}"#);
    }

    #[test]
    fn oversized_line_is_reported_once_and_next_line_survives() {
        let (sender, receiver) = channel();
        let mut decoder = LineDecoder::default();
        let mut input = vec![b'x'; MAX_LINE_BYTES + 8];
        input.extend_from_slice(b"\n{}\n");
        assert!(decoder.push(&input, &sender));
        assert!(matches!(receiver.try_recv(), Ok(ReaderMessage::Invalid(_))));
        let ReaderMessage::Line(line) = receiver.try_recv().unwrap() else {
            panic!("expected recovery line")
        };
        assert_eq!(line, b"{}");
    }

    #[test]
    fn wire_schema_rejects_unknown_fields_and_bad_values() {
        assert!(
            serde_json::from_slice::<WireEvent>(
                br#"{"v":1,"event":"checking_cache","token":"sentinel"}"#
            )
            .is_err()
        );
        assert!(!valid_https_uri("http://example.test"));
        assert!(!valid_https_uri("https://user@example.test"));
        assert!(!valid_code("bad code"));
        assert!(valid_code("ABCD-1234"));
    }

    #[test]
    fn safe_errors_are_printable_and_bounded() {
        let message = safe_message(&format!("bad\n{}", "x".repeat(300)));
        assert!(!message.contains('\n'));
        assert_eq!(message.chars().count(), 160);
    }

    #[test]
    fn cached_and_first_time_sequences_are_distinct() {
        let mut state = AuthState::Checking;
        let mut terminal = false;
        let mut checking_seen = false;
        apply_event(
            &mut state,
            &mut terminal,
            &mut checking_seen,
            br#"{"v":1,"event":"checking_cache"}"#,
        );
        apply_event(
            &mut state,
            &mut terminal,
            &mut checking_seen,
            br#"{"v":1,"event":"authenticated","method":"cached"}"#,
        );
        assert_eq!(state, AuthState::Authenticated);
        assert!(terminal);

        let mut state = AuthState::Checking;
        let mut terminal = false;
        let mut checking_seen = false;
        apply_event(
            &mut state,
            &mut terminal,
            &mut checking_seen,
            br#"{"v":1,"event":"checking_cache"}"#,
        );
        apply_event(
            &mut state,
            &mut terminal,
            &mut checking_seen,
            br#"{"v":1,"event":"device_code","verification_uri":"https://example.test/device","user_code":"ABCD-1234"}"#,
        );
        assert!(matches!(state, AuthState::AwaitingCode { .. }));
        apply_event(
            &mut state,
            &mut terminal,
            &mut checking_seen,
            br#"{"v":1,"event":"authenticated","method":"device_code"}"#,
        );
        assert_eq!(state, AuthState::Authenticated);
        assert!(terminal);
    }

    #[test]
    fn malformed_out_of_order_and_duplicate_events_fail_closed() {
        for lines in [
            vec![br#"not-json"#.as_slice()],
            vec![br#"{"v":1,"event":"authenticated","method":"device_code"}"#.as_slice()],
            vec![
                br#"{"v":1,"event":"checking_cache"}"#.as_slice(),
                br#"{"v":1,"event":"checking_cache"}"#.as_slice(),
            ],
        ] {
            let mut state = AuthState::Checking;
            let mut terminal = false;
            let mut checking_seen = false;
            for line in lines {
                apply_event(&mut state, &mut terminal, &mut checking_seen, line);
            }
            assert!(matches!(state, AuthState::Failed(_)));
            assert!(terminal);
        }
    }

    #[test]
    fn cancellation_reaps_a_running_helper() {
        let mut command = if cfg!(windows) {
            let mut command = Command::new("cmd");
            command.args(["/C", "ping -n 30 127.0.0.1 >NUL"]);
            command
        } else {
            let mut command = Command::new("sh");
            command.args(["-c", "sleep 30"]);
            command
        };
        let child = command.stdout(Stdio::piped()).spawn().unwrap();
        let mut supervisor = AuthSupervisor::from_child(child).unwrap();
        supervisor.cancel();
        assert!(supervisor.child.try_wait().unwrap().is_some());
        assert_eq!(supervisor.state, AuthState::SignedOut);
    }

    #[test]
    fn signed_out_profile_exposes_sign_in_without_gating_offline_servers() {
        use super::super::{MenuAction, MenuRuntime, MenuScreen};

        let mut menu = MenuRuntime::new(true, 2, "Offline Player".to_owned());
        menu.activate(MenuAction::Navigate(MenuScreen::Profile));
        let view = menu.view();
        assert_eq!(view.auth_state, AuthState::SignedOut);
        assert!(!view.catalog_loading);
        assert!(menu.focus_actions().contains(&MenuAction::StartSignIn));
        menu.activate(MenuAction::Navigate(MenuScreen::Servers));
        assert!(menu.focus_actions().contains(&MenuAction::PlayAddServer));
    }
}
