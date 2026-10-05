// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! TCE-ERG-2 falsifiability gate. The MCP server `instructions`
//! string and the command tool descriptions are the surface an agent
//! reads when deciding whether to use Terminal Commander. This test
//! pins the agent-selfish contract so a future edit cannot silently
//! drop the signal model, the argv-first routing rule, or the no-silence
//! receipt promise. `get_info()` does not touch the daemon, so a
//! lazy client pointed at a nonexistent socket is sufficient.

use rmcp::ServerHandler;
use terminal_commander_mcp::daemon_client::McpDaemonClient;
use terminal_commander_mcp::tools::TerminalCommanderMcpServer;

fn server() -> TerminalCommanderMcpServer {
    TerminalCommanderMcpServer::new(McpDaemonClient::new("/nonexistent-tc-test.sock"))
}

#[test]
fn instructions_name_signal_model_pitch_and_routing() {
    let info = server().get_info();
    let instr = info
        .instructions
        .expect("server must advertise instructions");
    let lower = instr.to_lowercase();

    // (a) no-output-by-default signal model named up front.
    assert!(
        lower.contains("structured signal"),
        "instructions must name the structured-signal model: {instr}"
    );
    // (b) agent-selfish pitch (tokens / context).
    assert!(
        lower.contains("token") || lower.contains("context"),
        "instructions must carry the agent-selfish pitch: {instr}"
    );
    // (c) argv-first routing: ordinary commands stay on argv tools.
    assert!(
        lower.contains("argv"),
        "instructions must name argv as the primary path: {instr}"
    );
    assert!(
        !lower.contains("plain shell"),
        "instructions must not steer agents to plain shell: {instr}"
    );
    // (d) the no-silence receipt promise.
    assert!(
        lower.contains("receipt"),
        "instructions must promise a receipt instead of silence: {instr}"
    );
    // (e) the quick / long / more-output routing triad.
    for route in ["run_and_watch", "bucket_wait", "command_output_tail"] {
        assert!(
            instr.contains(route),
            "instructions must name {route}: {instr}"
        );
    }
    // Clients may prepend this to every tool, so its length is multiplied
    // by the tool count.
    assert!(
        instr.chars().count() <= 850,
        "instructions must stay <= 850 chars, got {}",
        instr.chars().count()
    );
}
