// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! Serve a client that skips the `initialize` handshake.
//!
//! A harness that reconnects to a freshly started adapter (for example after
//! an upgrade stopped the previous process) can send its first request, such
//! as `tools/list`, without `initialize`. rmcp treats a connection whose first
//! request is not `initialize` as a 2026-07-28 stateless connection: every
//! request must carry the `_meta` protocol fields, and a request without them
//! is rejected and ends the session, so the harness reports "fetching tools
//! failed" and Terminal Commander is unusable until a manual reconnect.
//!
//! [`bridge_stdio`] sits between stdio and rmcp. Pre-init `ping`s pass
//! through untouched (rmcp answers those). The first other message decides:
//! when it is not `initialize` and carries no 2026-07-28 protocol `_meta`, the
//! bridge first feeds rmcp an `initialize` for the newest pre-2026 revision
//! plus the `initialized` notification, exactly what the client should have
//! sent, and drops rmcp's reply to that synthetic `initialize` so the client
//! only sees answers to its own requests. Clients that initialize, and
//! stateless 2026-07-28 clients that send the `_meta` fields, are untouched.

use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader, DuplexStream};
use tokio::task::JoinHandle;

/// JSON-RPC id of the synthetic `initialize`; its response is never forwarded.
pub const IMPLICIT_INIT_ID: &str = "tc-implicit-initialize";

/// Protocol revision the synthetic `initialize` asks for: the newest one that
/// still has an `initialize` handshake.
pub const IMPLICIT_INIT_PROTOCOL: &str = "2025-11-25";

const META_PROTOCOL_VERSION: &str = "io.modelcontextprotocol/protocolVersion";

/// What the bridge does with a client line seen before the session is decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FirstMessage {
    /// A pre-init `ping` (or a blank line): forward it and keep watching.
    Undecided,
    /// `initialize`, a stateless 2026-07-28 request with `_meta`, or a line
    /// that is not a JSON-RPC message: forward it and stop watching.
    PassThrough,
    /// A request or notification with no handshake: inject one first.
    InjectHandshake,
}

/// Classify one client line that arrives before the session is decided.
#[must_use]
pub fn classify_first_message(line: &str) -> FirstMessage {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return FirstMessage::Undecided;
    }
    let Ok(msg) = serde_json::from_str::<Value>(trimmed) else {
        return FirstMessage::PassThrough;
    };
    let Some(method) = msg.get("method").and_then(Value::as_str) else {
        return FirstMessage::PassThrough;
    };
    if method == "initialize" {
        return FirstMessage::PassThrough;
    }
    if method == "ping" && msg.get("id").is_some() {
        return FirstMessage::Undecided;
    }
    let has_protocol_meta = msg
        .get("params")
        .and_then(|p| p.get("_meta"))
        .and_then(|m| m.get(META_PROTOCOL_VERSION))
        .is_some();
    if has_protocol_meta {
        FirstMessage::PassThrough
    } else {
        FirstMessage::InjectHandshake
    }
}

/// The two lines the client should have sent first.
#[must_use]
pub fn implicit_handshake() -> String {
    let initialize = serde_json::json!({
        "jsonrpc": "2.0",
        "id": IMPLICIT_INIT_ID,
        "method": "initialize",
        "params": {
            "protocolVersion": IMPLICIT_INIT_PROTOCOL,
            "capabilities": {},
            "clientInfo": { "name": "terminal-commander-implicit-init", "version": "0" }
        }
    });
    let initialized = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "notifications/initialized"
    });
    format!("{initialize}\n{initialized}\n")
}

/// True when `line` is rmcp's reply to the synthetic `initialize`.
#[must_use]
pub fn is_implicit_init_reply(line: &str) -> bool {
    serde_json::from_str::<Value>(line.trim())
        .ok()
        .and_then(|msg| msg.get("id").cloned())
        .is_some_and(|id| id == Value::String(IMPLICIT_INIT_ID.to_owned()))
}

/// Copy client -> rmcp, injecting the handshake when the first real message
/// needs one. Returns whether it injected. Closing `to_rmcp` on client EOF is
/// what tells rmcp the client went away.
async fn pump_client<R, W>(from_client: R, mut to_rmcp: W) -> std::io::Result<bool>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut reader = BufReader::new(from_client);
    let mut injected = false;
    let mut line = String::new();
    loop {
        line.clear();
        if reader.read_line(&mut line).await? == 0 {
            to_rmcp.shutdown().await?;
            return Ok(injected);
        }
        match classify_first_message(&line) {
            FirstMessage::Undecided => to_rmcp.write_all(line.as_bytes()).await?,
            decided => {
                if decided == FirstMessage::InjectHandshake {
                    to_rmcp.write_all(implicit_handshake().as_bytes()).await?;
                    injected = true;
                }
                to_rmcp.write_all(line.as_bytes()).await?;
                break;
            }
        }
    }
    tokio::io::copy_buf(&mut reader, &mut to_rmcp).await?;
    to_rmcp.shutdown().await?;
    Ok(injected)
}

/// Copy rmcp -> client, dropping rmcp's reply to the synthetic `initialize`.
async fn pump_server<R, W>(from_rmcp: R, mut to_client: W) -> std::io::Result<()>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut reader = BufReader::new(from_rmcp);
    let mut line = String::new();
    loop {
        line.clear();
        if reader.read_line(&mut line).await? == 0 {
            return to_client.flush().await;
        }
        if is_implicit_init_reply(&line) {
            continue;
        }
        to_client.write_all(line.as_bytes()).await?;
        to_client.flush().await?;
    }
}

/// Stdio wired to rmcp through the handshake bridge.
pub struct StdioBridge {
    /// What rmcp reads (client messages, plus any injected handshake).
    pub reader: DuplexStream,
    /// What rmcp writes (forwarded to stdout).
    pub writer: DuplexStream,
    /// Finishes once everything rmcp wrote has reached stdout. Await it after
    /// the service ends: a client that pipes one request and closes stdin
    /// must still receive the reply before the process exits.
    pub output_done: JoinHandle<()>,
}

/// Wire stdio to rmcp through the handshake bridge.
#[must_use]
pub fn bridge_stdio() -> StdioBridge {
    const BUF: usize = 1 << 20;
    let (to_rmcp, reader) = tokio::io::duplex(BUF);
    let (writer, from_rmcp) = tokio::io::duplex(BUF);
    tokio::spawn(async move {
        let _ = pump_client(tokio::io::stdin(), to_rmcp).await;
    });
    let output_done = tokio::spawn(async move {
        let _ = pump_server(from_rmcp, tokio::io::stdout()).await;
    });
    StdioBridge {
        reader,
        writer,
        output_done,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_first_messages() {
        use FirstMessage::{InjectHandshake, PassThrough, Undecided};
        let cases = [
            (
                r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
                PassThrough,
            ),
            (
                r#"{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}"#,
                InjectHandshake,
            ),
            (
                r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#,
                InjectHandshake,
            ),
            (
                r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
                InjectHandshake,
            ),
            (r#"{"jsonrpc":"2.0","id":7,"method":"ping"}"#, Undecided),
            ("   \n", Undecided),
            (
                r#"{"jsonrpc":"2.0","id":1,"method":"server/discover","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{}}}}"#,
                PassThrough,
            ),
            ("not json", PassThrough),
            (r#"{"jsonrpc":"2.0","id":1,"result":{}}"#, PassThrough),
        ];
        for (line, want) in cases {
            assert_eq!(classify_first_message(line), want, "{line}");
        }
    }

    #[test]
    fn recognises_only_the_synthetic_reply() {
        assert!(is_implicit_init_reply(
            r#"{"jsonrpc":"2.0","id":"tc-implicit-initialize","result":{}}"#
        ));
        assert!(!is_implicit_init_reply(
            r#"{"jsonrpc":"2.0","id":1,"result":{}}"#
        ));
        assert!(!is_implicit_init_reply(
            r#"{"jsonrpc":"2.0","id":"other","result":{}}"#
        ));
    }

    #[tokio::test]
    async fn pump_client_injects_once_before_the_first_real_message() {
        let input = concat!(
            r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#,
            "\n",
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
            "\n",
            r#"{"jsonrpc":"2.0","id":3,"method":"tools/list"}"#,
            "\n",
        );
        let mut out = Vec::new();
        let injected = pump_client(input.as_bytes(), &mut out).await.unwrap();
        assert!(injected);
        let out = String::from_utf8(out).unwrap();
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 5, "{out}");
        assert!(lines[0].contains("\"ping\""));
        assert!(lines[1].contains(IMPLICIT_INIT_ID) && lines[1].contains("\"initialize\""));
        assert!(lines[2].contains("notifications/initialized"));
        assert!(lines[3].contains("\"id\":2"));
        assert!(lines[4].contains("\"id\":3"));
    }

    #[tokio::test]
    async fn pump_client_leaves_an_initializing_client_alone() {
        let input = concat!(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
            "\n",
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
            "\n",
        );
        let mut out = Vec::new();
        let injected = pump_client(input.as_bytes(), &mut out).await.unwrap();
        assert!(!injected);
        assert_eq!(String::from_utf8(out).unwrap(), input);
    }

    #[tokio::test]
    async fn pump_server_drops_only_the_synthetic_reply() {
        let input = concat!(
            r#"{"jsonrpc":"2.0","id":"tc-implicit-initialize","result":{}}"#,
            "\n",
            r#"{"jsonrpc":"2.0","id":2,"result":{"tools":[]}}"#,
            "\n",
        );
        let mut out = Vec::new();
        pump_server(input.as_bytes(), &mut out).await.unwrap();
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "{\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{\"tools\":[]}}\n"
        );
    }
}
