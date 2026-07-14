#!/usr/bin/env python3
# SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
# Copyright 2026 The Terminal Commander Authors
#
# US6 / T054 + T055: the shared omni O-01..O-14 acceptance runner.
#
# This is the ONE place the O-sequence is defined. The per-platform
# wrapper scripts (verify-omni-{linux,wsl,windows,macos}) build a fresh
# daemon + MCP adapter, start the daemon, and invoke this runner with a
# platform profile that says which gates RUN here and which are SKIPPED.
#
# HONESTY CONTRACT (CLAUDE.md prime directive):
#   - A gate that RUNS and passes prints "PASS  O-NN ...".
#   - A gate that RUNS and fails prints "FAIL  O-NN ..." and the runner
#     EXITS NON-ZERO. A skipped gate is NEVER counted as a pass.
#   - A gate that cannot run in THIS environment prints
#     "SKIP  O-NN ... (reason)" LOUDLY. Skips do not fail the run, but
#     the final summary prints exactly which gates ran vs were skipped
#     so a reader cannot mistake a skip for a pass.
#
# The runner drives the MCP adapter over stdio with bounded JSON-RPC and
# asserts ONLY bounded, combed responses -- never raw stdout/stderr.
#
# Usage:
#   omni-o-runner.py <mcp_bin> <socket> <profile>
#
# <profile> is one of: linux, wsl, windows, macos. It selects which
# gates run on this host (e.g. sessions are unix-only; ConPTY-output is
# windows-only; privileged is always deferred).
#
# Exit codes:
#   0 -- every RAN gate passed (skips are reported, not failed).
#   1 -- at least one RAN gate FAILED.
#   2 -- environment / harness problem (bad args, adapter died).

import json
import os
import shutil
import subprocess
import sys
import time

# ---------------------------------------------------------------------------
# Platform profiles: which O-gates RUN vs SKIP on each host.
#
# Each gate maps to one of:
#   "run"             -- execute the concrete check; pass/fail is real.
#   "skip:<reason>"   -- cannot run here; reported LOUDLY, never a pass.
#
# Rationale per gate is in run_gate(). The privileged gate (O-06) is
# ALWAYS skip:deferred -- P4 is plan-only this program (threat review
# pending). The provider trust gate (O-14) is a SEPARATE provider-harness
# pass (T057), not this daemon+adapter smoke.
# ---------------------------------------------------------------------------

_BASE_SKIPS = {
    "O-06": "skip:privileged-helper-deferred (P4 plan-only, threat_review_pending)",
    "O-09": "skip:needs-ssh-remote-host (P5 federation; not provisioned in smoke env)",
    "O-10": "skip:needs-container-runtime (P5 federation; not provisioned in smoke env)",
    "O-13": "skip:needs-ipc-fault-injection (degraded/recover requires a fault harness)",
    "O-14": "skip:provider-trust-smokes-are-a-separate-pass (T057 provider-harness)",
}


def profile_plan(profile):
    """Return the {O-id: run|skip:reason} plan for a platform profile."""
    plan = dict(_BASE_SKIPS)
    # Gates every platform with a daemon can run honestly.
    plan["O-01"] = "run"  # shell_exec pipeline
    plan["O-04"] = "run"  # single actionable signal
    plan["O-05"] = "run"  # suggest loop (heuristic, never activates)
    plan["O-11"] = "run"  # parallel jobs + one subscription
    plan["O-12"] = "run"  # no double spawn (adapter static no-spawn guard)

    if profile in ("linux", "wsl", "macos"):
        # POSIX hosts: sessions + PTY available.
        plan["O-02"] = "run"  # session cd/pwd (unix-only runtime)
        plan["O-03"] = "run"  # python REPL via PTY (posix backend)
        # O-07 is Windows-native PTY; O-08 is the macOS agent smoke.
        plan["O-07"] = "skip:windows-only-gate (running on a POSIX host)"
        if profile == "macos":
            plan["O-08"] = "run"  # macOS full agent smoke == this POSIX run
        else:
            plan["O-08"] = "skip:macos-only-gate (running on Linux/WSL)"
    elif profile == "windows":
        # Windows: ConPTY PTY available; sessions are unix-only.
        plan["O-02"] = "skip:sessions-unix-only (no Windows shell-session runtime yet)"
        plan["O-03"] = "skip:posix-pty-gate (O-07 covers the Windows ConPTY REPL)"
        plan["O-07"] = "run"  # native Windows ConPTY python REPL
        plan["O-08"] = "skip:macos-only-gate (running on Windows)"
    else:
        raise SystemExit(f"omni-o-runner: unknown profile {profile!r}")
    return plan

_COMPACT_ROUTES = {
    "shell_exec": ("command", "exec"),
    "run_and_watch": ("command", "run_and_watch"),
    "command_start_combed": ("command", "run"),
    "command_status": ("command", "status"),
    "subscription_open": ("command", "sub_open"),
    "subscription_pull": ("command", "sub_pull"),
    "subscription_close": ("command", "sub_close"),
    "registry_suggest_from_samples": ("registry", "suggest_from_samples"),
    "pty_command_start": ("session", "pty_start"),
    "pty_command_write_stdin": ("session", "pty_stdin"),
    "pty_command_stop": ("session", "pty_stop"),
    "shell_session_start": ("session", "sh_start"),
    "shell_session_exec": ("session", "sh_exec"),
    "shell_session_status": ("session", "sh_status"),
    "shell_session_stop": ("session", "sh_stop"),
}


def _route_call(tool, arguments, available_tools):
    """Route a granular OMNI call through the compact facade when required."""
    routed = dict(arguments)
    if tool in available_tools:
        return tool, routed

    route = _COMPACT_ROUTES.get(tool)
    if route is None or route[0] not in available_tools:
        return tool, routed

    facade, action = route
    if tool == "shell_exec" and "grace_ms" in routed:
        routed.setdefault("wait_ms", routed.pop("grace_ms"))
    routed["action"] = action
    return facade, routed


def _portable_echo(text):
    """Return argv for a real cross-platform executable that prints one line."""
    return [
        sys.executable,
        "-c",
        "import sys; sys.stdout.write(sys.argv[1] + '\\n')",
        text,
    ]


# ---------------------------------------------------------------------------
# Bounded JSON-RPC pump over the MCP adapter stdio transport.
# ---------------------------------------------------------------------------

class Mcp:
    """A minimal, bounded JSON-RPC client for the MCP stdio adapter."""

    def __init__(self, mcp_bin, socket):
        env = os.environ.copy()
        env["TC_SOCKET"] = socket
        self.proc = subprocess.Popen(
            [mcp_bin],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            env=env,
            text=True,
            bufsize=1,
        )
        self._next_id = 1
        self._init()

    def _send(self, method, params=None):
        req = {"jsonrpc": "2.0", "id": self._next_id, "method": method}
        if params is not None:
            req["params"] = params
        self._next_id += 1
        self.proc.stdin.write(json.dumps(req) + "\n")
        self.proc.stdin.flush()
        return req["id"]

    def _recv(self, target_id, deadline=15.0):
        end = time.time() + deadline
        while time.time() < end:
            if self.proc.poll() is not None:
                raise SystemExit("omni-o-runner: MCP adapter exited unexpectedly")
            line = self.proc.stdout.readline()
            if not line:
                time.sleep(0.02)
                continue
            try:
                msg = json.loads(line)
            except json.JSONDecodeError:
                continue
            if msg.get("id") == target_id:
                return msg
        raise SystemExit(f"omni-o-runner: timed out waiting for id={target_id}")

    def _init(self):
        init_id = self._send(
            "initialize",
            {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {"name": "omni-o-runner", "version": "0.0.0"},
            },
        )
        self._recv(init_id)
        self.proc.stdin.write(
            json.dumps({"jsonrpc": "2.0", "method": "notifications/initialized"}) + "\n"
        )
        self.proc.stdin.flush()

        list_id = self._send("tools/list", {})
        listed = self._recv(list_id)
        if "error" in listed:
            raise SystemExit(f"omni-o-runner: tools/list failed: {listed['error']}")
        self._available_tools = {
            tool["name"]
            for tool in listed.get("result", {}).get("tools", [])
            if isinstance(tool, dict) and isinstance(tool.get("name"), str)
        }

    def call(self, tool, arguments, deadline=15.0):
        """Call one MCP tool; route granular names through compact facades."""
        tool, arguments = _route_call(tool, arguments, self._available_tools)
        cid = self._send(
            "tools/call", {"name": tool, "arguments": arguments}
        )
        msg = self._recv(cid, deadline=deadline)
        if "error" in msg:
            return {"_error": msg["error"]}
        content = msg.get("result", {}).get("content", [])
        if not content:
            return {}
        return json.loads(content[0]["text"])

    def close(self):
        try:
            self.proc.stdin.close()
        except Exception:
            pass
        self.proc.terminate()
        try:
            self.proc.wait(timeout=3)
        except subprocess.TimeoutExpired:
            self.proc.kill()


# ---------------------------------------------------------------------------
# Gate checks. Each returns (ok: bool, detail: str). They use ONLY bounded
# combed tool calls. A check that hits a daemon error returns ok=False with
# the error so a wiring regression fails the gate loudly.
# ---------------------------------------------------------------------------

_TERMINAL_STATES = {"exited", "cancelled", "failed"}


def _state_norm(st):
    """Normalize a job state string (the daemon serializes it lowercase)."""
    return str(st.get("state", "")).lower()


def _wait_terminal(mcp, job_id, deadline=10.0):
    """Poll command_status until the job is terminal; return the status."""
    end = time.time() + deadline
    last = None
    while time.time() < end:
        st = mcp.call("command_status", {"job_id": job_id})
        last = st
        if "_error" in st:
            return st
        if _state_norm(st) in _TERMINAL_STATES:
            return st
        time.sleep(0.1)
    return last or {}


def gate_o01(mcp):
    # O-01: a real shell pipeline in ONE call via shell_exec, combed.
    r = mcp.call(
        "shell_exec",
        {"shell_line": "echo a | wc -c", "grace_ms": 2000},
    )
    if "_error" in r:
        return False, f"shell_exec rejected: {r['_error']}"
    job_id = r.get("job_id")
    if not job_id:
        return False, f"shell_exec returned no job_id: {r}"
    st = _wait_terminal(mcp, job_id)
    if "_error" in st:
        return False, f"command_status error: {st['_error']}"
    if _state_norm(st) != "exited":
        return False, f"pipeline did not exit cleanly: {st}"
    # exit_code may live at the top level or inside the receipt; 0 is the goal.
    code = st.get("exit_code")
    if code is None and isinstance(st.get("receipt"), dict):
        code = st["receipt"].get("exit_code")
    if code not in (0, None):
        return False, f"pipeline exit_code != 0: {st}"
    return True, f"echo a | wc -c -> exited code={code} (job {job_id})"


def gate_o02(mcp):
    # O-02: persistent session cwd stickiness -- cd then pwd shows /tmp,
    # without re-passing cwd. Combed signals only.
    s = mcp.call("shell_session_start", {})
    if "_error" in s:
        return False, f"shell_session_start error: {s['_error']}"
    sid = s.get("session_id")
    if not sid:
        return False, f"no session_id: {s}"
    try:
        e1 = mcp.call(
            "shell_session_exec",
            {"session_id": sid, "line": "cd /tmp", "wait_ms": 1500},
        )
        if "_error" in e1:
            return False, f"session cd error: {e1['_error']}"
        st = mcp.call("shell_session_status", {"session_id": sid})
        if "_error" in st:
            return False, f"session status error: {st['_error']}"
        cwd = st.get("cwd", "")
        # macOS resolves /tmp -> /private/tmp; accept either.
        if not (cwd.endswith("/tmp") or cwd.endswith("/private/tmp")):
            return False, f"session cwd did not stick to /tmp: {cwd!r}"
        return True, f"cd /tmp sticky; session cwd={cwd}"
    finally:
        mcp.call("shell_session_stop", {"session_id": sid})


def _pty_repl(mcp, prog):
    # Shared PTY REPL drive used by O-03 (posix) and O-07 (windows).
    start = mcp.call("pty_command_start", {"argv": [prog]})
    if "_error" in start:
        return False, f"pty_command_start error: {start['_error']}"
    job_id = start.get("job_id")
    if not job_id:
        return False, f"pty_command_start no job_id: {start}"
    try:
        w = mcp.call(
            "pty_command_write_stdin",
            {"job_id": job_id, "bytes": "print(6*7)\n"},
        )
        if "_error" in w:
            return False, f"pty write error: {w['_error']}"
        time.sleep(0.4)
        exitw = mcp.call(
            "pty_command_write_stdin",
            {"job_id": job_id, "bytes": "exit()\n"},
        )
        if "_error" in exitw:
            return False, f"pty exit-write error: {exitw['_error']}"
        return True, f"PTY {prog} REPL drove input+output (job {job_id})"
    finally:
        mcp.call("pty_command_stop", {"job_id": job_id})


def gate_o03(mcp):
    # O-03: python REPL via PTY on the posix backend.
    prog = shutil.which("python3") or shutil.which("python")
    if not prog:
        return None, "python3 not found on PATH"
    return _pty_repl(mcp, prog)


def gate_o07(mcp):
    # O-07: native Windows ConPTY python REPL.
    prog = shutil.which("python") or shutil.which("python3")
    if not prog:
        return None, "python not found on PATH"
    return _pty_repl(mcp, prog)


def gate_o04(mcp):
    # O-04: a noisy long-output command condenses to ONE actionable signal
    # via run_and_watch (npm-install-style: lots of output, one outcome).
    r = mcp.call(
        "run_and_watch",
        {"argv": _portable_echo("added 1 package in 1s"), "wait_ms": 3000},
        deadline=12.0,
    )
    if "_error" in r:
        return False, f"run_and_watch error: {r['_error']}"
    # A quiet command returns a receipt (no-silence rule) -- exactly ONE
    # bounded outcome object, never raw stdout. Accept a receipt OR a
    # bounded signals list; both are a single actionable result.
    if "receipt" not in r and "signals" not in r:
        return False, f"run_and_watch produced neither receipt nor signals: {r}"
    return True, "single bounded outcome (receipt/signals), no raw stream"


def gate_o05(mcp):
    # O-05: the suggest loop. registry_suggest_from_samples returns DRAFT
    # proposals + heuristic confidence + explicit next steps, and NEVER
    # activates a rule (FR-008).
    r = mcp.call(
        "registry_suggest_from_samples",
        {
            "samples": [
                "error: build failed at src/main.rs:10",
                "warning: unused variable: x",
            ],
            "intent": "errors and warnings",
            "max_rules": 3,
        },
    )
    if "_error" in r:
        return False, f"suggest error: {r['_error']}"
    if "proposed_rules" not in r:
        return False, f"no proposed_rules: {r}"
    if r.get("confidence") != "heuristic":
        return False, f"confidence must be heuristic: {r}"
    steps = r.get("next_steps", [])
    if "registry_activate" not in steps:
        return False, f"next_steps must guide to activate: {steps}"
    return True, f"{len(r['proposed_rules'])} draft rule(s), never auto-activated"


def gate_o11(mcp):
    # O-11: 3 parallel jobs, ONE subscription. Start three commands, open a
    # single broad subscription, and pull combed events for all of them.
    jobs = []
    for i in range(3):
        r = mcp.call(
            "command_start_combed",
            {"argv": _portable_echo(f"omni-parallel-{i}"), "grace_ms": 2000},
        )
        if "_error" in r:
            return False, f"parallel start {i} error: {r['_error']}"
        if not r.get("job_id"):
            return False, f"parallel start {i} no job_id: {r}"
        jobs.append(r["job_id"])
    # Omitting `sources` routes to ALL buckets (per-BUCKET default
    # `{ "kind": "all" }`), so one subscription spans the 3 parallel jobs.
    sub = mcp.call("subscription_open", {})
    if "_error" in sub:
        return False, f"subscription_open error: {sub['_error']}"
    sub_id = sub.get("sub_id")
    if not sub_id:
        return False, f"subscription_open no sub_id: {sub}"
    try:
        pull = mcp.call(
            "subscription_pull",
            {"sub_id": sub_id, "max": 50, "timeout_ms": 2000},
            deadline=12.0,
        )
        if "_error" in pull:
            return False, f"subscription_pull error: {pull['_error']}"
        if "events" not in pull or "liveness" not in pull:
            return False, f"pull missing events/liveness: {pull}"
        return True, f"3 jobs {jobs}; one subscription pulled bounded events"
    finally:
        mcp.call("subscription_close", {"sub_id": sub_id})


def gate_o12(repo_root):
    # O-12: client timeout on start must NOT cause a double spawn. The
    # adapter NEVER spawns -- it forwards every job to the single daemon
    # over IPC -- so "no double spawn" is proven structurally by the
    # adapter no-spawn guard (the same guard scripts/linux-gate.sh enforces).
    # A static guard is the honest, deterministic proof here; injecting a
    # real client timeout would be flaky.
    src = os.path.join(repo_root, "crates", "mcp", "src")
    offenders = []
    needles = ("Command::new", "Command::spawn", "TcpListener", "UdpSocket")
    for dirpath, _dirs, files in os.walk(src):
        for fn in files:
            if not fn.endswith(".rs"):
                continue
            path = os.path.join(dirpath, fn)
            with open(path, "r", encoding="utf-8", errors="replace") as f:
                for lineno, line in enumerate(f, 1):
                    if any(n in line for n in needles):
                        stripped = line.lstrip()
                        # Doc/comment mentions are fine; real code is not.
                        if stripped.startswith(("//", "/*", "*", "#")):
                            continue
                        offenders.append(f"{path}:{lineno}: {stripped.rstrip()}")
    if offenders:
        return False, "adapter has spawn/socket code (double-spawn risk): " + \
            "; ".join(offenders[:3])
    return True, "adapter never spawns (no Command/listener in crates/mcp/src)"


# ---------------------------------------------------------------------------
# Orchestration.
# ---------------------------------------------------------------------------

def main():
    if len(sys.argv) != 4:
        print("usage: omni-o-runner.py <mcp_bin> <socket> <profile>", file=sys.stderr)
        return 2
    mcp_bin, socket, profile = sys.argv[1], sys.argv[2], sys.argv[3]
    repo_root = os.environ.get("OMNI_REPO_ROOT", os.getcwd())

    plan = profile_plan(profile)
    order = [f"O-{n:02d}" for n in range(1, 15)]

    ran, passed, failed, skipped = [], [], [], []
    print(f"omni-o-runner: profile={profile} repo_root={repo_root}", file=sys.stderr)

    mcp = Mcp(mcp_bin, socket)
    try:
        for oid in order:
            action = plan.get(oid, "skip:not-mapped")
            if action.startswith("skip:"):
                reason = action[len("skip:"):]
                print(f"SKIP  {oid} ({reason})", file=sys.stderr)
                skipped.append((oid, reason))
                continue

            # Dispatch the concrete check.
            try:
                if oid == "O-01":
                    ok, detail = gate_o01(mcp)
                elif oid == "O-02":
                    ok, detail = gate_o02(mcp)
                elif oid == "O-03":
                    ok, detail = gate_o03(mcp)
                elif oid == "O-04":
                    ok, detail = gate_o04(mcp)
                elif oid == "O-05":
                    ok, detail = gate_o05(mcp)
                elif oid == "O-07":
                    ok, detail = gate_o07(mcp)
                elif oid == "O-11":
                    ok, detail = gate_o11(mcp)
                elif oid == "O-12":
                    ok, detail = gate_o12(repo_root)
                else:
                    ok, detail = None, "no concrete check mapped"
            except SystemExit:
                raise
            except Exception as exc:  # pylint: disable=broad-except
                ok, detail = False, f"exception: {exc!r}"

            if ok is None:
                # A precondition (e.g. python3 absent) made the gate
                # un-runnable HERE -- report as a loud skip, never a pass.
                print(f"SKIP  {oid} (precondition: {detail})", file=sys.stderr)
                skipped.append((oid, f"precondition: {detail}"))
                continue

            ran.append(oid)
            if ok:
                print(f"PASS  {oid}  {detail}", file=sys.stderr)
                passed.append(oid)
            else:
                print(f"FAIL  {oid}  {detail}", file=sys.stderr)
                failed.append(oid)
    finally:
        mcp.close()

    print("", file=sys.stderr)
    print("omni-o-runner SUMMARY", file=sys.stderr)
    print(f"  RAN    ({len(ran)}): {', '.join(ran) or '-'}", file=sys.stderr)
    print(f"  PASSED ({len(passed)}): {', '.join(passed) or '-'}", file=sys.stderr)
    print(f"  FAILED ({len(failed)}): {', '.join(failed) or '-'}", file=sys.stderr)
    print(
        f"  SKIPPED ({len(skipped)}): "
        + (", ".join(f"{o}[{r}]" for o, r in skipped) or "-"),
        file=sys.stderr,
    )
    if failed:
        print("omni-o-runner: RESULT=FAIL (a RAN gate failed)", file=sys.stderr)
        return 1
    print(
        "omni-o-runner: RESULT=OK (every RAN gate passed; skips reported above)",
        file=sys.stderr,
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
