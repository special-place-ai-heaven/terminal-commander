#!/usr/bin/env python3
# SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
"""Exercise every terminal-commander MCP tool over stdio (Windows-native path)."""

from __future__ import annotations

import json
import os
import subprocess
import sys
import tempfile
import time
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
MCP_SHIM = REPO / "packages" / "terminal-commander" / "bin" / "terminal-commander-mcp.js"
NODE = os.environ.get("NODE", "node")


def main() -> int:
    if not MCP_SHIM.is_file():
        print(f"missing shim: {MCP_SHIM}", file=sys.stderr)
        return 2

    tmp = Path(tempfile.mkdtemp(prefix="tc-mcp-tools-"))
    probe_file = tmp / "probe.txt"
    probe_file.write_text("terminal-commander smoke line\nsecond line\n", encoding="utf-8")

    env = os.environ.copy()
    env.pop("TC_WSL_DISTRO", None)
    env["TC_DEBUG_SESSION"] = env.get("TC_DEBUG_SESSION", "1")

    proc = subprocess.Popen(
        [NODE, str(MCP_SHIM)],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        env=env,
        text=True,
        bufsize=1,
    )

    nid = 1
    results: dict[str, dict] = {}

    def send(method: str, params=None) -> int:
        nonlocal nid
        req = {"jsonrpc": "2.0", "id": nid, "method": method}
        if params is not None:
            req["params"] = params
        rid = nid
        nid += 1
        assert proc.stdin is not None
        proc.stdin.write(json.dumps(req) + "\n")
        proc.stdin.flush()
        return rid

    def recv(target_id: int, deadline: float = 30.0) -> dict:
        assert proc.stdout is not None
        end = time.time() + deadline
        while time.time() < end:
            line = proc.stdout.readline()
            if not line:
                time.sleep(0.02)
                continue
            try:
                msg = json.loads(line)
            except json.JSONDecodeError:
                continue
            if msg.get("id") == target_id:
                return msg
        raise TimeoutError(f"timed out waiting for id={target_id}")

    def call_tool(name: str, arguments: dict | None = None) -> dict:
        rid = send("tools/call", {"name": name, "arguments": arguments or {}})
        msg = recv(rid, deadline=45.0 if name == "bucket_wait" else 30.0)
        results[name] = msg
        return msg

    def tool_text(msg: dict) -> dict:
        if msg.get("error"):
            return {"_error": msg["error"]}
        content = msg.get("result", {}).get("content") or []
        if not content:
            return {}
        text = content[0].get("text", "{}")
        try:
            return json.loads(text)
        except json.JSONDecodeError:
            return {"_raw": text[:500]}

    def ok(name: str, msg: dict) -> bool:
        if msg.get("error"):
            return False
        if msg.get("result", {}).get("isError"):
            return False
        return True

    try:
        init_id = send(
            "initialize",
            {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {"name": "tc-all-tools-smoke", "version": "0"},
            },
        )
        init = recv(init_id)
        if init.get("error"):
            print("initialize failed:", init, file=sys.stderr)
            return 1
        assert proc.stdin is not None
        proc.stdin.write(
            json.dumps({"jsonrpc": "2.0", "method": "notifications/initialized"}) + "\n"
        )
        proc.stdin.flush()

        # Session supervisor starts the daemon concurrently; allow IPC bind.
        time.sleep(2.0)

        tl_id = send("tools/list")
        tl = recv(tl_id)
        tools = tl.get("result", {}).get("tools", [])
        names = sorted(t.get("name") for t in tools if t.get("name"))
        print(f"tools/list: {len(names)} tools")

        # --- stateless / list tools ---
        for name in (
            "system_discover",
            "health",
            "policy_status",
            "self_check",
            "registry_list_active",
            "file_watch_list",
            "pty_command_list",
            "runtime_state",
            "probe_list",
        ):
            call_tool(name)

        call_tool("audit_since", {"cursor": 0, "limit": 5})
        call_tool("registry_search", {"query": "smoke", "limit": 5})

        rule_def = {
            "id": "smoke.test.rule",
            "version": 1,
            "kind": "keyword",
            "status": "active",
            "severity": "low",
            "event_kind": "keyword_hit",
            "summary_template": "smoke hit",
            "keywords": ["smoke"],
        }
        call_tool("registry_upsert", {"definition_json": json.dumps(rule_def)})
        call_tool("registry_get", {"rule_id": "smoke.test.rule"})
        call_tool(
            "registry_test",
            {
                "rule_id": "smoke.test.rule",
                "version": 1,
                "samples": [{"text": "this is smoke test", "stream": "stdout"}],
            },
        )
        call_tool(
            "registry_activate",
            {
                "rule_id": "smoke.test.rule",
                "version": 1,
                "scope": {"kind": "global"},
            },
        )

        if os.name == "nt":
            argv = [r"C:\Windows\System32\hostname.exe"]
        else:
            argv = ["/bin/hostname"]

        start_msg = call_tool("command_start_combed", {"argv": argv, "grace_ms": 2000})
        start = tool_text(start_msg)
        job_id = start.get("job_id")
        bucket_id = start.get("bucket_id")
        probe_id = start.get("probe_id")
        cursor = start.get("cursor", 0)

        if job_id:
            time.sleep(0.5)
            call_tool("command_status", {"job_id": job_id})
        if bucket_id is not None:
            call_tool(
                "bucket_events_since",
                {"bucket_id": bucket_id, "cursor": int(cursor or 0), "limit": 20},
            )
            call_tool("bucket_summary", {"bucket_id": bucket_id})
            call_tool(
                "bucket_wait",
                {"bucket_id": bucket_id, "cursor": int(cursor or 0), "timeout_ms": 500},
            )
        if probe_id:
            call_tool("probe_status", {"probe_id": probe_id})

        # Param names must match the Mcp* structs in crates/mcp/src/tools.rs.
        call_tool(
            "file_read_window",
            {"path": str(probe_file), "start_line": 1, "max_lines": 5},
        )
        call_tool(
            "file_search",
            {"path": str(probe_file), "query": "smoke", "max_matches": 5},
        )
        fw = call_tool(
            "file_watch_start",
            {"path": str(probe_file)},
        )
        watch = tool_text(fw)
        watch_id = watch.get("watch_id")
        if watch_id:
            call_tool("file_watch_stop", {"watch_id": watch_id})

        # PTY — may be unsupported on Windows; record outcome either way
        pty_start = call_tool(
            "pty_command_start",
            {"argv": argv},
        )
        pty = tool_text(pty_start)
        pty_job = pty.get("job_id")
        if pty_job:
            call_tool("pty_command_write_stdin", {"job_id": pty_job, "bytes": "x\n"})
            call_tool("pty_command_stop", {"job_id": pty_job})

        call_tool(
            "registry_deactivate",
            {
                "rule_id": "smoke.test.rule",
                "version": 1,
                "scope": {"kind": "global"},
            },
        )

        # event_context only if we got an event id
        if bucket_id is not None:
            ev_msg = results.get("bucket_events_since", {})
            ev_body = tool_text(ev_msg)
            events = ev_body.get("events") or []
            if events:
                eid = events[0].get("event_id") or events[0].get("id")
                if eid:
                    call_tool(
                        "event_context",
                        {"bucket_id": bucket_id, "event_id": str(eid), "before": 1, "after": 1},
                    )

    finally:
        try:
            if proc.stdin:
                proc.stdin.close()
        except Exception:
            pass
        proc.terminate()
        try:
            proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            proc.kill()
        err = proc.stderr.read() if proc.stderr else ""
        if err.strip():
            print("--- stderr ---", file=sys.stderr)
            print(err[-4000:], file=sys.stderr)

    # Report
    passed = []
    failed = []
    skipped = []
    expected_fail = set()

    if os.name == "nt":
        expected_fail.add("pty_command_start")
        expected_fail.add("pty_command_write_stdin")
        expected_fail.add("pty_command_stop")

    expected = sorted(
        t.get("name")
        for t in (tl.get("result", {}).get("tools") or [])
        if t.get("name")
    )
    for name in expected:
        msg = results.get(name)
        if msg is None:
            skipped.append(name)
            continue
        if ok(name, msg):
            passed.append(name)
        elif name in expected_fail:
            passed.append(f"{name} (expected platform limitation)")
        else:
            failed.append((name, msg.get("error") or tool_text(msg)))

    print("\n=== MCP tool smoke ===")
    print(f"PASS ({len(passed)}): {', '.join(passed)}")
    if failed:
        print(f"FAIL ({len(failed)}):")
        for name, detail in failed:
            print(f"  - {name}: {json.dumps(detail)[:300]}")
    if skipped:
        print(f"NOT CALLED ({len(skipped)}): {', '.join(skipped)}")

    return 0 if not failed and not skipped else 1


if __name__ == "__main__":
    raise SystemExit(main())
