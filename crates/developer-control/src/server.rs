//! The loopback listener: authenticates each request line and queues it for the game loop.

use std::{
    io::{self, BufRead, BufReader, Read, Write},
    net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream},
    path::{Path, PathBuf},
    sync::Arc,
    thread,
};

use crossbeam_channel::{Receiver, Sender};
use serde_json::{Value, json};

use crate::{
    endpoint::{Endpoint, random_token, token_matches},
    protocol::{Command, Request},
};

/// Longest accepted request line; commands are small JSON objects.
const MAX_LINE_BYTES: usize = 1 << 20;

/// One authenticated command awaiting the game loop's reply.
pub struct Pending {
    pub command: Command,
    pub reply: Reply,
}

/// Answers one request; dropping it unanswered tells the caller the command was dropped.
pub struct Reply(Sender<Result<Value, String>>);

impl Reply {
    /// A reply and the receiver its outcome arrives on, for callers outside a connection.
    pub fn channel() -> (Self, Receiver<Result<Value, String>>) {
        let (sender, receiver) = crossbeam_channel::bounded(1);
        (Self(sender), receiver)
    }

    pub fn send(self, outcome: Result<Value, String>) {
        let _ = self.0.send(outcome);
    }
}

/// Owns the listener thread and the endpoint file, which it removes on drop.
pub struct ControlServer {
    endpoint_path: PathBuf,
    pending: Receiver<Pending>,
}

impl ControlServer {
    /// Binds 127.0.0.1 on an OS-chosen port and publishes a fresh token at `endpoint_path`.
    pub fn start(endpoint_path: &Path) -> io::Result<Self> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
        let token: Arc<str> = random_token().into();
        Endpoint {
            port: listener.local_addr()?.port(),
            token: token.to_string(),
            pid: std::process::id(),
        }
        .write(endpoint_path)?;
        let (sender, pending) = crossbeam_channel::unbounded();
        thread::Builder::new()
            .name("developer-control".into())
            .spawn(move || accept(listener, token, sender))?;
        Ok(Self {
            endpoint_path: endpoint_path.to_owned(),
            pending,
        })
    }

    /// Commands received since the last drain, in arrival order.
    pub fn drain(&self) -> impl Iterator<Item = Pending> + '_ {
        self.pending.try_iter()
    }
}

impl Drop for ControlServer {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.endpoint_path);
    }
}

fn accept(listener: TcpListener, token: Arc<str>, sender: Sender<Pending>) {
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        if !stream.peer_addr().is_ok_and(|peer| is_loopback(&peer)) {
            continue;
        }
        let (token, sender) = (Arc::clone(&token), sender.clone());
        let _ = thread::Builder::new()
            .name("developer-control-connection".into())
            .spawn(move || serve(stream, &token, &sender));
    }
}

fn is_loopback(peer: &SocketAddr) -> bool {
    peer.ip().is_loopback()
}

fn serve(stream: TcpStream, token: &str, sender: &Sender<Pending>) {
    let Ok(mut writer) = stream.try_clone() else {
        return;
    };
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    loop {
        line.clear();
        match (&mut reader)
            .take(MAX_LINE_BYTES as u64 + 1)
            .read_line(&mut line)
        {
            Ok(0) | Err(_) => return,
            Ok(read) if read > MAX_LINE_BYTES => {
                let _ = respond(&mut writer, &Value::Null, Err("request too long".into()));
                return;
            }
            Ok(_) => {}
        }
        if line.trim().is_empty() {
            continue;
        }
        let (id, outcome, close) = handle_line(&line, token, sender);
        if respond(&mut writer, &id, outcome).is_err() || close {
            return;
        }
    }
}

/// The request id, its outcome, and whether to drop the connection.
pub(crate) fn handle_line(
    line: &str,
    token: &str,
    sender: &Sender<Pending>,
) -> (Value, Result<Value, String>, bool) {
    let request = match Request::parse(line) {
        Ok(request) => request,
        Err(error) => return (error.id, Err(error.message), error.unauthenticated),
    };
    if !token_matches(token, &request.token) {
        return (request.id, Err("unauthorized".into()), true);
    }
    let id = request.id.clone();
    let command = match request.command() {
        Ok(command) => command,
        Err(message) => return (id, Err(message), false),
    };
    let (reply, outcome) = crossbeam_channel::bounded(1);
    let reply = Reply(reply);
    if sender.send(Pending { command, reply }).is_err() {
        return (id, Err("client is shutting down".into()), true);
    }
    let outcome = outcome
        .recv()
        .unwrap_or_else(|_| Err("client dropped the command".into()));
    (id, outcome, false)
}

fn respond(writer: &mut TcpStream, id: &Value, outcome: Result<Value, String>) -> io::Result<()> {
    let reply = match outcome {
        Ok(result) => json!({ "id": id, "ok": true, "result": result }),
        Err(error) => json!({ "id": id, "ok": false, "error": error }),
    };
    writeln!(writer, "{reply}")?;
    writer.flush()
}

#[cfg(test)]
mod tests;
