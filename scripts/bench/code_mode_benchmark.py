#!/usr/bin/env python3
# SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
# Copyright 2026 The Terminal Commander Authors
"""Paired deterministic end-to-end benchmark for Terminal Commander surfaces.

Compares `TC_SURFACE=full` (60 tools), `TC_SURFACE=compact` (6 action-union facades),
and `TC_SURFACE=code` (3 tools: `code_search`, `code_execute`, `status`, exposing all
60 granular ops inside the pure-Rust Monty VM) against a live isolated
`terminal-commanderd` + `terminal-commander-mcp` runtime across 5 multi-step
terminal workflows.

Protocol opener uses MCP `2026-07-28` `server/discover` + `_meta` (never legacy
handshake openers). Token counts use `tiktoken` `o200k_base` when installed,
falling back to a deterministic UTF-8/4 BPE estimator only if `tiktoken` is absent.
"""

from __future__ import annotations

import argparse
import json
import os
import pathlib
import subprocess
import sys
import tempfile
import time
from typing import Any

try:
    import tiktoken

    _ENC = tiktoken.get_encoding("o200k_base")

    def count_tokens(text: str) -> int:
        return len(_ENC.encode(text, disallowed_special=()))

    TOKENIZER_NAME = "tiktoken:o200k_base"
except Exception:  # pragma: no cover - fallback when tiktoken is unavailable

    def count_tokens(text: str) -> int:
        return max(1, (len(text.encode("utf-8")) + 3) // 4) if text else 0

    TOKENIZER_NAME = "utf8_div4_fallback"


PROTOCOL_VERSION = "2026-07-28"
MCP_META = {
    "io.modelcontextprotocol/protocolVersion": "2026-07-28",
    "io.modelcontextprotocol/clientCapabilities": {},
    "io.modelcontextprotocol/clientInfo": {
        "name": "tc-code-mode-bench",
        "version": "1.0.0",
    },
}


class McpStdioClient:
    """Persistent stdio JSON-RPC client speaking MCP 2026-07-28 (`server/discover`)."""

    def __init__(
        self,
        mcp_bin: pathlib.Path,
        daemon_bin: pathlib.Path,
        socket_path: str,
        state_dir: pathlib.Path,
        surface: str,
    ) -> None:
        self.surface = surface
        self._next_id = 1
        env = os.environ.copy()
        env["TC_SURFACE"] = surface
        env["TC_SOCKET"] = socket_path
        self.proc = subprocess.Popen(
            [
                str(mcp_bin),
                "--socket",
                socket_path,
                "--daemon-binary",
                str(daemon_bin),
                "--state-dir",
                str(state_dir),
            ],
            env=env,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )

    def close(self) -> None:
        if self.proc.poll() is None:
            try:
                if self.proc.stdin:
                    self.proc.stdin.close()
                self.proc.wait(timeout=3.0)
            except Exception:
                self.proc.kill()
                self.proc.wait()

    def rpc(self, method: str, params: dict[str, Any] | None = None) -> dict[str, Any]:
        assert self.proc.stdin is not None and self.proc.stdout is not None
        req_id = self._next_id
        self._next_id += 1
        body_params = dict(params or {})
        meta = dict(body_params.get("_meta") or {})
        for k, v in MCP_META.items():
            meta.setdefault(k, v)
        body_params["_meta"] = meta
        req = {
            "jsonrpc": "2.0",
            "id": req_id,
            "method": method,
            "params": body_params,
        }
        wire = json.dumps(req, separators=(",", ":")) + "\n"
        self.proc.stdin.write(wire.encode("utf-8"))
        self.proc.stdin.flush()
        while True:
            raw = self.proc.stdout.readline()
            if not raw:
                stderr_tail = ""
                if self.proc.stderr:
                    stderr_tail = self.proc.stderr.read().decode("utf-8", errors="replace")[-800:]
                raise RuntimeError(
                    f"EOF waiting for {method} (id={req_id}) on TC_SURFACE={self.surface}: {stderr_tail}"
                )
            msg = json.loads(raw.decode("utf-8", errors="replace"))
            if msg.get("id") == req_id:
                return msg

    def discover_and_measure_surface(self) -> dict[str, Any]:
        disc = self.rpc("server/discover", {})
        if "error" in disc:
            raise RuntimeError(f"server/discover failed on {self.surface}: {disc['error']}")
        disc_res = disc.get("result", {})
        instructions = (
            disc_res.get("serverConfig", {}).get("instructions")
            or disc_res.get("instructions")
            or ""
        )
        listed = self.rpc("tools/list", {})
        if "error" in listed:
            raise RuntimeError(f"tools/list failed on {self.surface}: {listed['error']}")
        tools = listed["result"].get("tools", [])
        tools_json = json.dumps({"tools": tools}, separators=(",", ":"))
        return {
            "surface": self.surface,
            "tool_count": len(tools),
            "tool_names": [t.get("name", "") for t in tools],
            "tools_list_bytes": len(tools_json.encode("utf-8")),
            "tools_list_tokens": count_tokens(tools_json),
            "instructions_bytes": len(instructions.encode("utf-8")),
            "instructions_tokens": count_tokens(instructions),
        }

    def call_tool(self, name: str, arguments: dict[str, Any]) -> tuple[dict[str, Any] | str, str, str]:
        req_json = json.dumps({"name": name, "arguments": arguments}, separators=(",", ":"))
        resp = self.rpc("tools/call", {"name": name, "arguments": arguments})
        resp_json = json.dumps(resp, separators=(",", ":"))
        if "error" in resp:
            raise RuntimeError(f"tools/call {name} JSON-RPC error on {self.surface}: {resp['error']}")
        result = resp["result"]
        if result.get("isError"):
            raise RuntimeError(f"tools/call {name} tool error on {self.surface}: {resp_json}")
        blocks = result.get("content", [])
        text = blocks[0].get("text", "") if blocks else ""
        try:
            parsed: dict[str, Any] | str = json.loads(text)
        except Exception:
            parsed = text
        return parsed, req_json, text


def seed_fixtures(work_dir: pathlib.Path) -> dict[str, str]:
    src_dir = work_dir / "fixture_repo" / "src"
    src_dir.mkdir(parents=True, exist_ok=True)
    for idx in range(8):
        (src_dir / f"mod_{idx}.rs").write_text(
            "\n".join(
                f"pub fn helper_{idx}_{line}() -> usize {{ {line} }}"
                for line in range(1, 35)
            )
            + "\n",
            encoding="utf-8",
        )
    target_rs = src_dir / "widget.rs"
    widget_lines = [f"// header line {i}" for i in range(1, 38)] + [
        "pub fn validate_frame_budget(bytes: usize) -> bool {",
        "    // CRITICAL_INVARIANT_MARKER: frame_budget_non_zero",
        "    let limit: u32 = \"not_an_int\"; // line 40 deliberate type mismatch",
        "    bytes > 0 && limit > 0",
        "}",
    ] + [f"// trailing line {i}" for i in range(43, 80)]
    target_rs.write_text("\n".join(widget_lines) + "\n", encoding="utf-8")

    emitter = work_dir / "emit_sim.py"
    emitter.write_text(
        """import sys, time
mode = sys.argv[1]
if mode == "failing_build":
    for i in range(1, 120):
        print(f"   Compiling crate_{i:03d} v0.1.0")
        if i == 45:
            print("error[E0308]: mismatched types at src/widget.rs:40:22")
            print("  --> src/widget.rs:40:22")
            print("   | expected `u32`, found `&str`")
        if i == 90:
            print("error[E0425]: cannot find value `missing_ident` in src/mod_2.rs:12:5")
    print("error: could not compile `fixture_repo` due to 2 previous errors")
    sys.exit(101)
elif mode == "phased_job":
    for i in range(1, 40):
        print(f"progress tick {i}/40")
    print("MILESTONE: PHASE_ALPHA_READY")
    time.sleep(0.05)
    for i in range(41, 80):
        print(f"progress tick {i}/80")
    print("MILESTONE: PHASE_COMPLETE_OK")
    sys.exit(0)
elif mode == "custom_daemon_log":
    for i in range(1, 25):
        print(f"2026-11-23T12:00:{i:02d}Z INFO worker={i%4} heartbeat ok")
    print("2026-11-23T12:00:26Z ERROR subsystem=spool code=E_SPOOL_FULL dropped=17")
    print("2026-11-23T12:00:27Z ERROR subsystem=spool code=E_SPOOL_FULL dropped=9")
    print("2026-11-23T12:00:28Z WARN subsystem=ipc slow_frame_ms=412")
    sys.exit(1)
""",
        encoding="utf-8",
    )
    return {
        "python": sys.executable,
        "emitter": str(emitter.resolve()),
        "repo_dir": str((work_dir / "fixture_repo").resolve()),
        "widget_rs": str(target_rs.resolve()),
        "scratch_out": str((work_dir / "fixture_repo" / "triage_note.txt").resolve()),
    }


def grade_substrings(payload_text: str, required: list[str]) -> tuple[bool, list[str]]:
    missing = [needle for needle in required if needle not in payload_text]
    return (len(missing) == 0, missing)


def run_arm_turns(
    surface_meta: dict[str, Any],
    turns: list[tuple[str, str]],
    final_answer_text: str,
    required_facts: list[str],
    elapsed_ms: float,
) -> dict[str, Any]:
    """Compute realistic cumulative LLM context input + output tokens across N turns.

    On every LLM turn `t` (0..N-1), an MCP-connected model pays:
      - `schema_tokens + instructions_tokens` (upfront tool definitions in prompt)
      - all prior tool call arguments & tool response texts accumulated in context.
    """
    schema_tok = surface_meta["tools_list_tokens"] + surface_meta["instructions_tokens"]
    n_turns = len(turns)
    history_tok = 0
    cumulative_input_tok = 0
    tool_io_tok = 0
    for req_text, resp_text in turns:
        req_tok = count_tokens(req_text)
        resp_tok = count_tokens(resp_text)
        cumulative_input_tok += schema_tok + history_tok
        history_tok += req_tok + resp_tok
        tool_io_tok += req_tok + resp_tok
    # Final synthesis turn reading the last tool output:
    cumulative_input_tok += schema_tok + history_tok
    passed, missing = grade_substrings(final_answer_text, required_facts)
    return {
        "turns": n_turns,
        "passed": passed,
        "missing_facts": missing,
        "tool_io_tokens": tool_io_tok,
        "single_turn_schema_tokens": schema_tok,
        "cumulative_prompt_tokens": cumulative_input_tok,
        "wall_ms": round(elapsed_ms, 2),
    }


def run_workflows_for_surface(
    client: McpStdioClient,
    surface_meta: dict[str, Any],
    fx: dict[str, str],
) -> dict[str, dict[str, Any]]:
    py = fx["python"]
    em = fx["emitter"]
    surface = client.surface
    out: dict[str, dict[str, Any]] = {}

    # ------------------------------------------------------------------
    # W1: Failing build triage + event_context + source read (4 turns vs 1)
    # ------------------------------------------------------------------
    req_w1 = ["101", "E0308", "widget.rs", "CRITICAL_INVARIANT_MARKER"]
    t0 = time.perf_counter()
    turns: list[tuple[str, str]] = []
    if surface == "full":
        r1, q1, a1 = client.call_tool(
            "run_and_watch",
            {
                "argv": [py, em, "failing_build"],
                "rules": [{"pattern": r"error(\[E\d+\])?:", "severity": "error"}],
                "wait_ms": 10000,
            },
        )
        turns.append((q1, a1))
        assert isinstance(r1, dict)
        r2, q2, a2 = client.call_tool(
            "bucket_events_since",
            {"bucket_id": r1["bucket_id"], "cursor": 0, "severity_min": "high"},
        )
        turns.append((q2, a2))
        assert isinstance(r2, dict)
        ev0 = r2["events"][0]["event_id"]
        r3, q3, a3 = client.call_tool("event_context", {"event_id": ev0, "before": 1, "after": 3})
        turns.append((q3, a3))
        r4, q4, a4 = client.call_tool(
            "file_read_window",
            {"path": fx["widget_rs"], "start_line": 38, "max_lines": 6},
        )
        turns.append((q4, a4))
        final_w1 = json.dumps({"exit": r1["exit_code"], "ctx": r3, "src": r4})
    elif surface == "compact":
        r1, q1, a1 = client.call_tool(
            "command",
            {
                "action": "run_and_watch",
                "argv": [py, em, "failing_build"],
                "rules": [{"pattern": r"error(\[E\d+\])?:", "severity": "error"}],
                "wait_ms": 10000,
            },
        )
        turns.append((q1, a1))
        assert isinstance(r1, dict)
        r2, q2, a2 = client.call_tool(
            "command",
            {
                "action": "events",
                "bucket_id": r1["bucket_id"],
                "cursor": 0,
                "severity_min": "high",
            },
        )
        turns.append((q2, a2))
        assert isinstance(r2, dict)
        ev0 = r2["events"][0]["event_id"]
        r3, q3, a3 = client.call_tool(
            "command",
            {"action": "event_context", "event_id": ev0, "before": 1, "after": 3},
        )
        turns.append((q3, a3))
        r4, q4, a4 = client.call_tool(
            "files",
            {"action": "read", "path": fx["widget_rs"], "start_line": 38, "max_lines": 6},
        )
        turns.append((q4, a4))
        final_w1 = json.dumps({"exit": r1["exit_code"], "ctx": r3, "src": r4})
    else:
        code_w1 = f"""
run = run_and_watch(
    argv=[{py!r}, {em!r}, "failing_build"],
    rules=[{{"pattern": r"error(\\[E\\d+\\])?:", "severity": "error"}}],
    wait_ms=10000,
)
ev = bucket_events_since(bucket_id=run["bucket_id"], cursor=0, severity_min="high")
ctx = event_context(event_id=ev["events"][0]["event_id"], before=1, after=3)
win = file_read_window(path={fx["widget_rs"]!r}, start_line=38, max_lines=6)
{{
    "exit_code": run["exit_code"],
    "first_error": ev["events"][0]["summary"],
    "context_lines": [f["text"] for f in ctx["frames"]],
    "source_lines": [l["text"] for l in win["lines"]],
}}
"""
        rc, qc, ac = client.call_tool("code_execute", {"code": code_w1})
        turns.append((qc, ac))
        final_w1 = ac
    out["W1_failing_build_triage"] = run_arm_turns(
        surface_meta, turns, final_w1, req_w1, (time.perf_counter() - t0) * 1000.0
    )

    # ------------------------------------------------------------------
    # W2: Long combed watcher (start -> wait -> status -> summary)
    # ------------------------------------------------------------------
    req_w2 = ["exited", "PHASE_ALPHA_READY", "PHASE_COMPLETE_OK"]
    t0 = time.perf_counter()
    turns = []
    if surface == "full":
        r1, q1, a1 = client.call_tool(
            "command_start_combed",
            {
                "argv": [py, em, "phased_job"],
                "rules": [{"keywords": ["MILESTONE:"], "severity": "medium"}],
            },
        )
        turns.append((q1, a1))
        assert isinstance(r1, dict)
        bkt, job, cur = r1["bucket_id"], r1["job_id"], r1["cursor"]
        events_acc = []
        for _ in range(5):
            rw, qw, aw = client.call_tool(
                "bucket_wait", {"bucket_id": bkt, "cursor": cur, "timeout_ms": 2000}
            )
            turns.append((qw, aw))
            assert isinstance(rw, dict)
            events_acc.extend(rw["events"])
            cur = rw["next_cursor"]
            rs, qs, as_ = client.call_tool("command_status", {"job_id": job})
            turns.append((qs, as_))
            assert isinstance(rs, dict)
            if rs["state"] in ("exited", "failed", "cancelled"):
                break
        rsum, qsum, asum = client.call_tool("bucket_summary", {"bucket_id": bkt})
        turns.append((qsum, asum))
        final_w2 = json.dumps({"status": rs, "events": events_acc, "summary": rsum})
    elif surface == "compact":
        r1, q1, a1 = client.call_tool(
            "command",
            {
                "action": "run",
                "argv": [py, em, "phased_job"],
                "rules": [{"keywords": ["MILESTONE:"], "severity": "medium"}],
            },
        )
        turns.append((q1, a1))
        assert isinstance(r1, dict)
        bkt, job, cur = r1["bucket_id"], r1["job_id"], r1["cursor"]
        events_acc = []
        for _ in range(5):
            rw, qw, aw = client.call_tool(
                "command",
                {"action": "wait", "bucket_id": bkt, "cursor": cur, "timeout_ms": 2000},
            )
            turns.append((qw, aw))
            assert isinstance(rw, dict)
            events_acc.extend(rw["events"])
            cur = rw["next_cursor"]
            rs, qs, as_ = client.call_tool("command", {"action": "status", "job_id": job})
            turns.append((qs, as_))
            assert isinstance(rs, dict)
            if rs["state"] in ("exited", "failed", "cancelled"):
                break
        rsum, qsum, asum = client.call_tool("command", {"action": "summary", "bucket_id": bkt})
        turns.append((qsum, asum))
        final_w2 = json.dumps({"status": rs, "events": events_acc, "summary": rsum})
    else:
        code_w2 = f"""
started = command_start_combed(
    argv=[{py!r}, {em!r}, "phased_job"],
    rules=[{{"keywords": ["MILESTONE:"], "severity": "medium"}}],
)
cur = started["cursor"]
milestones = []
state = "running"
for _ in range(10):
    w = bucket_wait(bucket_id=started["bucket_id"], cursor=cur, timeout_ms=2000)
    for e in w["events"]:
        milestones.append(e["summary"])
    cur = w["next_cursor"]
    st = command_status(job_id=started["job_id"])
    state = st["state"]
    if state in ("exited", "failed", "cancelled"):
        break
w_final = bucket_events_since(bucket_id=started["bucket_id"], cursor=0)
milestones = [e["summary"] for e in w_final["events"]]
summary = bucket_summary(bucket_id=started["bucket_id"])
{{"state": state, "milestones": milestones, "event_count": summary["event_count"]}}
"""
        rc, qc, ac = client.call_tool("code_execute", {"code": code_w2})
        turns.append((qc, ac))
        final_w2 = ac
    out["W2_long_combed_watcher"] = run_arm_turns(
        surface_meta, turns, final_w2, req_w2, (time.perf_counter() - t0) * 1000.0
    )

    # ------------------------------------------------------------------
    # W3: Exploratory tail -> rule suggest -> upsert -> registry_test
    # ------------------------------------------------------------------
    req_w3 = ["E_SPOOL_FULL", "matches"]
    t0 = time.perf_counter()
    turns = []
    if surface == "full":
        r1, q1, a1 = client.call_tool(
            "run_and_watch", {"argv": [py, em, "custom_daemon_log"], "wait_ms": 10000}
        )
        turns.append((q1, a1))
        assert isinstance(r1, dict)
        r2, q2, a2 = client.call_tool(
            "command_output_tail", {"job_id": r1["job_id"], "max_lines": 30, "strip_ansi": True}
        )
        turns.append((q2, a2))
        assert isinstance(r2, dict)
        r3, q3, a3 = client.call_tool(
            "registry_suggest_from_samples", {"sample_lines": r2["lines"], "max_rules": 2}
        )
        turns.append((q3, a3))
        assert isinstance(r3, dict)
        prop = r3["proposed_rules"][0]
        r4, q4, a4 = client.call_tool(
            "registry_upsert", {"definition_json": json.dumps(prop)}
        )
        turns.append((q4, a4))
        assert isinstance(r4, dict)
        samples = [{"text": ln} for ln in r2["lines"][-5:]]
        r5, q5, a5 = client.call_tool(
            "registry_test",
            {"rule_id": r4["rule_id"], "version": r4["version"], "samples": samples},
        )
        turns.append((q5, a5))
        final_w3 = json.dumps({"rule": r4, "test": r5, "tail": r2["lines"][-4:]})
    elif surface == "compact":
        r1, q1, a1 = client.call_tool(
            "command",
            {"action": "run_and_watch", "argv": [py, em, "custom_daemon_log"], "wait_ms": 10000},
        )
        turns.append((q1, a1))
        assert isinstance(r1, dict)
        r2, q2, a2 = client.call_tool(
            "command",
            {
                "action": "output_tail",
                "job_id": r1["job_id"],
                "max_lines": 30,
                "strip_ansi": True,
            },
        )
        turns.append((q2, a2))
        assert isinstance(r2, dict)
        r3, q3, a3 = client.call_tool(
            "registry",
            {"action": "suggest_from_samples", "sample_lines": r2["lines"], "max_rules": 2},
        )
        turns.append((q3, a3))
        assert isinstance(r3, dict)
        prop = r3["proposed_rules"][0]
        r4, q4, a4 = client.call_tool(
            "registry", {"action": "upsert", "definition_json": json.dumps(prop)}
        )
        turns.append((q4, a4))
        assert isinstance(r4, dict)
        samples = [{"text": ln} for ln in r2["lines"][-5:]]
        r5, q5, a5 = client.call_tool(
            "registry",
            {
                "action": "test",
                "rule_id": r4["rule_id"],
                "version": r4["version"],
                "samples": samples,
            },
        )
        turns.append((q5, a5))
        final_w3 = json.dumps({"rule": r4, "test": r5, "tail": r2["lines"][-4:]})
    else:
        code_w3 = f"""
import json
run = run_and_watch(argv=[{py!r}, {em!r}, "custom_daemon_log"], wait_ms=10000)
tail = command_output_tail(job_id=run["job_id"], max_lines=30, strip_ansi=True)
sug = registry_suggest_from_samples(sample_lines=tail["lines"], max_rules=2)
prop = sug["proposed_rules"][0]
up = registry_upsert(definition_json=json.dumps(prop))
samples = [{{"text": ln}} for ln in tail["lines"][-5:]]
tested = registry_test(rule_id=up["rule_id"], version=up["version"], samples=samples)
{{"rule_id": up["rule_id"], "matches": tested["matches"], "sample_error": tail["lines"][-3]}}
"""
        rc, qc, ac = client.call_tool("code_execute", {"code": code_w3})
        turns.append((qc, ac))
        final_w3 = ac
    out["W3_rule_suggest_upsert_test"] = run_arm_turns(
        surface_meta, turns, final_w3, req_w3, (time.perf_counter() - t0) * 1000.0
    )

    # ------------------------------------------------------------------
    # W4: Directory file search -> bounded window read -> write report
    # ------------------------------------------------------------------
    req_w4 = ["CRITICAL_INVARIANT_MARKER", "validate_frame_budget", "bytes_written"]
    t0 = time.perf_counter()
    turns = []
    if surface == "full":
        r1, q1, a1 = client.call_tool(
            "file_search", {"path": fx["repo_dir"], "query": "CRITICAL_INVARIANT_MARKER"}
        )
        turns.append((q1, a1))
        assert isinstance(r1, dict)
        hit = r1["matches"][0]
        hit_path = hit.get("file") or hit.get("path") or fx["widget_rs"]
        r2, q2, a2 = client.call_tool(
            "file_read_window",
            {"path": hit_path, "start_line": max(1, hit["line"] - 2), "max_lines": 6},
        )
        turns.append((q2, a2))
        assert isinstance(r2, dict)
        note = "\n".join(ln["text"] for ln in r2["lines"])
        r3, q3, a3 = client.call_tool(
            "file_write", {"path": fx["scratch_out"], "content": note, "create_dirs": True}
        )
        turns.append((q3, a3))
        final_w4 = json.dumps({"window": r2["lines"], "write": r3})
    elif surface == "compact":
        r1, q1, a1 = client.call_tool(
            "files",
            {"action": "search", "path": fx["repo_dir"], "query": "CRITICAL_INVARIANT_MARKER"},
        )
        turns.append((q1, a1))
        assert isinstance(r1, dict)
        hit = r1["matches"][0]
        hit_path = hit.get("file") or hit.get("path") or fx["widget_rs"]
        r2, q2, a2 = client.call_tool(
            "files",
            {
                "action": "read",
                "path": hit_path,
                "start_line": max(1, hit["line"] - 2),
                "max_lines": 6,
            },
        )
        turns.append((q2, a2))
        assert isinstance(r2, dict)
        note = "\n".join(ln["text"] for ln in r2["lines"])
        r3, q3, a3 = client.call_tool(
            "files",
            {
                "action": "write",
                "path": fx["scratch_out"],
                "content": note,
                "create_dirs": True,
            },
        )
        turns.append((q3, a3))
        final_w4 = json.dumps({"window": r2["lines"], "write": r3})
    else:
        code_w4 = f"""
srch = file_search(path={fx["repo_dir"]!r}, query="CRITICAL_INVARIANT_MARKER")
hit = srch["matches"][0]
hit_path = hit.get("file") or hit.get("path") or {fx["widget_rs"]!r}
win = file_read_window(path=hit_path, start_line=max(1, hit["line"] - 2), max_lines=6)
lines_txt = [l["text"] for l in win["lines"]]
wr = file_write(path={fx["scratch_out"]!r}, content="\\n".join(lines_txt), create_dirs=True)
{{"lines": lines_txt, "bytes_written": wr["bytes_written"]}}
"""
        rc, qc, ac = client.call_tool("code_execute", {"code": code_w4})
        turns.append((qc, ac))
        final_w4 = ac
    out["W4_file_search_window_write"] = run_arm_turns(
        surface_meta, turns, final_w4, req_w4, (time.perf_counter() - t0) * 1000.0
    )

    # ------------------------------------------------------------------
    # W5: Policy + Runtime State + Audit Trail Aggregation
    # ------------------------------------------------------------------
    req_w5 = ["FullAccess", "command_start"]
    t0 = time.perf_counter()
    turns = []
    if surface == "full":
        r1, q1, a1 = client.call_tool("policy_status", {})
        turns.append((q1, a1))
        r2, q2, a2 = client.call_tool("runtime_state", {"limit": 10})
        turns.append((q2, a2))
        r3, q3, a3 = client.call_tool("audit_since", {"cursor": 0, "limit": 50})
        turns.append((q3, a3))
        final_w5 = json.dumps({"policy": r1, "runtime": r2, "audit": r3})
    elif surface == "compact":
        r1, q1, a1 = client.call_tool("status", {"action": "policy_status"})
        turns.append((q1, a1))
        r2, q2, a2 = client.call_tool("status", {"action": "runtime_state", "limit": 10})
        turns.append((q2, a2))
        r3, q3, a3 = client.call_tool("status", {"action": "audit_since", "cursor": 0, "limit": 50})
        turns.append((q3, a3))
        final_w5 = json.dumps({"policy": r1, "runtime": r2, "audit": r3})
    else:
        code_w5 = """
pol = policy_status()
rt = runtime_state(limit=10)
aud = audit_since(cursor=0, limit=50)
by_action = {}
for row in aud["rows"]:
    act = row["action"]
    by_action[act] = by_action.get(act, 0) + 1
{
    "profile": pol["profile"],
    "bucket_count": rt["bucket_count"],
    "audit_counts": by_action,
}
"""
        rc, qc, ac = client.call_tool("code_execute", {"code": code_w5})
        turns.append((qc, ac))
        final_w5 = ac
    out["W5_policy_runtime_audit_digest"] = run_arm_turns(
        surface_meta, turns, final_w5, req_w5, (time.perf_counter() - t0) * 1000.0
    )

    return out


def resolve_default_bin(repo_root: pathlib.Path, name: str) -> pathlib.Path:
    exe = f"{name}.exe" if os.name == "nt" else name
    target_roots = []
    if os.environ.get("CARGO_TARGET_DIR"):
        target_roots.append(pathlib.Path(os.environ["CARGO_TARGET_DIR"]))
    target_roots.extend([repo_root / "target", pathlib.Path(r"C:\Target")])
    for troot in target_roots:
        for profile in ("debug", "release"):
            cand = (troot / profile / exe).resolve()
            if cand.exists():
                return cand
    raise FileNotFoundError(f"binary {exe} not found under {target_roots}")


def main() -> int:
    parser = argparse.ArgumentParser(description="Terminal Commander Code Mode paired benchmark")
    repo_root = pathlib.Path(__file__).resolve().parents[2]
    parser.add_argument("--mcp-bin", type=pathlib.Path, default=None)
    parser.add_argument("--daemon-bin", type=pathlib.Path, default=None)
    parser.add_argument("--json-out", type=pathlib.Path, default=None)
    args = parser.parse_args()

    mcp_bin = args.mcp_bin.resolve() if args.mcp_bin else resolve_default_bin(repo_root, "terminal-commander-mcp")
    daemon_bin = args.daemon_bin.resolve() if args.daemon_bin else resolve_default_bin(repo_root, "terminal-commanderd")

    with tempfile.TemporaryDirectory(prefix="tc-code-bench-", ignore_cleanup_errors=True) as tmp_raw:
        tmp = pathlib.Path(tmp_raw)
        state_dir = tmp / "state"
        state_dir.mkdir(parents=True, exist_ok=True)
        if os.name == "nt":
            sock = rf"\\.\pipe\tc-code-bench-{os.getpid()}-{time.time_ns()}"
        else:
            sock = str(state_dir / "terminal-commanderd.sock")

        daemon_env = os.environ.copy()
        daemon_env["TC_DATA"] = str(state_dir)
        daemon_env["TC_SOCKET"] = sock
        daemon_proc = subprocess.Popen(
            [
                str(daemon_bin),
                "start",
                "--mode",
                "ipc-server",
                "--data-dir",
                str(state_dir),
                "--socket",
                sock,
            ],
            env=daemon_env,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        try:
            fx = seed_fixtures(tmp)
            surfaces = ("full", "compact", "code")
            surface_schemas: dict[str, dict[str, Any]] = {}
            workflow_results: dict[str, dict[str, dict[str, Any]]] = {}

            for surf in surfaces:
                client = McpStdioClient(mcp_bin, daemon_bin, sock, state_dir, surf)
                try:
                    meta = client.discover_and_measure_surface()
                    surface_schemas[surf] = meta
                    workflow_results[surf] = run_workflows_for_surface(client, meta, fx)
                finally:
                    client.close()
        finally:
            daemon_proc.terminate()
            try:
                daemon_proc.wait(timeout=5.0)
            except Exception:
                daemon_proc.kill()
                daemon_proc.wait()
    summary_by_surface: dict[str, dict[str, Any]] = {}
    full_schema_bytes = surface_schemas["full"]["tools_list_bytes"]
    for surf in surfaces:
        wfs = workflow_results[surf]
        total_turns = sum(v["turns"] for v in wfs.values())
        total_prompt_tok = sum(v["cumulative_prompt_tokens"] for v in wfs.values())
        total_io_tok = sum(v["tool_io_tokens"] for v in wfs.values())
        total_wall_ms = round(sum(v["wall_ms"] for v in wfs.values()), 2)
        passed = sum(1 for v in wfs.values() if v["passed"])
        sb = surface_schemas[surf]["tools_list_bytes"]
        summary_by_surface[surf] = {
            "tool_count": surface_schemas[surf]["tool_count"],
            "tools_list_bytes": sb,
            "tools_list_byte_delta_vs_full_pct": round((sb - full_schema_bytes) * 100.0 / full_schema_bytes, 2),
            "tools_list_tokens": surface_schemas[surf]["tools_list_tokens"],
            "instructions_bytes": surface_schemas[surf]["instructions_bytes"],
            "pass_count": passed,
            "workflow_count": len(wfs),
            "pass_rate_pct": round(passed * 100.0 / len(wfs), 1),
            "total_turns_across_5_workflows": total_turns,
            "total_tool_io_tokens": total_io_tok,
            "total_cumulative_prompt_tokens": total_prompt_tok,
            "total_wall_ms": total_wall_ms,
        }

    full_prompt_tok = summary_by_surface["full"]["total_cumulative_prompt_tokens"]
    for surf in surfaces:
        cur = summary_by_surface[surf]["total_cumulative_prompt_tokens"]
        summary_by_surface[surf]["prompt_token_delta_vs_full_pct"] = round(
            (cur - full_prompt_tok) * 100.0 / full_prompt_tok, 2
        )

    report = {
        "benchmark": "terminal-commander-code-mode-v1",
        "protocol_version": PROTOCOL_VERSION,
        "tokenizer": TOKENIZER_NAME,
        "surfaces": summary_by_surface,
        "workflows": workflow_results,
    }

    if args.json_out:
        args.json_out.parent.mkdir(parents=True, exist_ok=True)
        args.json_out.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")

    print(json.dumps(report["surfaces"], indent=2))
    all_passed = all(s["pass_count"] == s["workflow_count"] for s in summary_by_surface.values())
    code_bytes_ok = summary_by_surface["code"]["tools_list_bytes"] <= 4500
    if not all_passed:
        print("FAIL: one or more benchmark workflows failed oracle grading", file=sys.stderr)
        return 1
    if not code_bytes_ok:
        print(
            f"FAIL: TC_SURFACE=code tools/list bytes ({summary_by_surface['code']['tools_list_bytes']}) > 4500 B",
            file=sys.stderr,
        )
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
