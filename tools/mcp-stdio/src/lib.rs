//! The stdio MCP wire shared by the repo's tool servers: newline-delimited JSON-RPC 2.0
//! with `initialize`, `ping`, `tools/list` and `tools/call`.

use std::io::{BufRead, Write};

use serde_json::{Value, json};

const PROTOCOL_VERSIONS: &[&str] = &["2025-06-18", "2025-03-26", "2024-11-05"];

/// A set of MCP tools behind one server.
pub trait ToolServer {
    /// `(name, version)` reported from `initialize`.
    fn info(&self) -> (&'static str, &'static str);
    fn definitions(&self) -> Value;
    /// The `tools/call` result: MCP content plus `isError`.
    fn call(&mut self, name: &str, arguments: &Value) -> Value;
}

/// Serves stdin until it closes or stdout fails.
pub fn serve(server: &mut impl ToolServer) {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let Ok(line) = line else {
            break;
        };
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Value>(&line) {
            Ok(message) => handle(server, &message),
            Err(error) => Some(failure(Value::Null, -32700, &error.to_string())),
        };
        if let Some(reply) = reply
            && (writeln!(stdout, "{reply}").is_err() || stdout.flush().is_err())
        {
            break;
        }
    }
}

/// The reply to one message; notifications get none.
pub fn handle(server: &mut impl ToolServer, message: &Value) -> Option<Value> {
    let id = message.get("id").cloned()?;
    let method = message.get("method").and_then(Value::as_str).unwrap_or("");
    let params = message.get("params").cloned().unwrap_or(Value::Null);
    let result = match method {
        "initialize" => {
            let requested = params
                .get("protocolVersion")
                .and_then(Value::as_str)
                .filter(|version| PROTOCOL_VERSIONS.contains(version))
                .unwrap_or(PROTOCOL_VERSIONS[0]);
            let (name, version) = server.info();
            json!({
                "protocolVersion": requested,
                "capabilities": { "tools": {} },
                "serverInfo": { "name": name, "version": version },
            })
        }
        "ping" => json!({}),
        "tools/list" => json!({ "tools": server.definitions() }),
        "tools/call" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or("");
            let arguments = params.get("arguments").cloned().unwrap_or(json!({}));
            server.call(name, &arguments)
        }
        _ => return Some(failure(id, -32601, &format!("unknown method `{method}`"))),
    };
    Some(json!({ "jsonrpc": "2.0", "id": id, "result": result }))
}

fn failure(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

/// One text item holding `value` as JSON.
pub fn text_content(value: &Value, error: bool) -> Value {
    json!({ "content": [{ "type": "text", "text": value.to_string() }], "isError": error })
}

/// An inline PNG content item.
pub fn png_content(png: &[u8]) -> Value {
    json!({ "type": "image", "mimeType": "image/png", "data": base64(png) })
}

pub fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, b)| n | u32::from(*b) << (16 - 8 * i));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(char::from(TABLE[(n >> (18 - 6 * i) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Echo;

    impl ToolServer for Echo {
        fn info(&self) -> (&'static str, &'static str) {
            ("echo", "1")
        }

        fn definitions(&self) -> Value {
            json!([{ "name": "echo" }])
        }

        fn call(&mut self, name: &str, arguments: &Value) -> Value {
            text_content(&json!({ "name": name, "arguments": arguments }), false)
        }
    }

    #[test]
    fn requests_get_replies_and_notifications_do_not() {
        let reply = handle(
            &mut Echo,
            &json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize",
                     "params": { "protocolVersion": "2024-11-05" } }),
        )
        .unwrap();
        assert_eq!(reply["result"]["protocolVersion"], "2024-11-05");
        assert_eq!(reply["result"]["serverInfo"]["name"], "echo");
        let unsupported = handle(
            &mut Echo,
            &json!({ "id": 2, "method": "initialize", "params": { "protocolVersion": "1999" } }),
        )
        .unwrap();
        assert_eq!(
            unsupported["result"]["protocolVersion"],
            PROTOCOL_VERSIONS[0]
        );
        assert!(handle(&mut Echo, &json!({ "method": "notifications/initialized" })).is_none());
        let call = handle(
            &mut Echo,
            &json!({ "id": 3, "method": "tools/call",
                     "params": { "name": "echo", "arguments": { "x": 1 } } }),
        )
        .unwrap();
        assert_eq!(call["id"], 3);
        assert_eq!(call["result"]["isError"], false);
        let unknown = handle(&mut Echo, &json!({ "id": 4, "method": "nope" })).unwrap();
        assert_eq!(unknown["error"]["code"], -32601);
    }

    #[test]
    fn base64_matches_the_standard_alphabet() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }
}
