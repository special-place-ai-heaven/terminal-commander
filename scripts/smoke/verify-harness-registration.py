#!/usr/bin/env python3
"""Verify generated MCP launches with an owned daemon and temporary homes.

Requires Python 3.11+, Node, and sibling built MCP/daemon binaries.
Usage: python scripts/smoke/verify-harness-registration.py <mcp-binary>
"""
import json
import os
from pathlib import Path
import queue
import shutil
import subprocess
import sys
import tempfile
import threading
import time
import tomllib
import uuid

REPO = Path(__file__).resolve().parents[2]
GENERATE = r"""
const fs = require('node:fs'), path = require('node:path');
const job = JSON.parse(fs.readFileSync(0, 'utf8'));
const {writeProvider} = require(path.join(job.repo, 'packages/terminal-commander/lib/harness/write_all.js'));
const ids = ['cursor','codex-cli','claude-code','claude-desktop','gemini','grok','kilo-code','omp'];
const outputs = ids.map(id => {
  const home = path.join(job.root,id);
  const result = writeProvider(id, {platform:process.platform, providerFilter:id, force:true,
    exePath:job.binary, machineKey:job.root,
    env:{HOME:home,USERPROFILE:home,APPDATA:path.join(home,'roaming')}});
  if (result.status !== 'ok') throw new Error(id + ': ' + result.status);
  return {id,path:result.path};
});
process.stdout.write(JSON.stringify(outputs));
"""


def stop(process):
    if process.poll() is None:
        process.terminate()
    try:
        process.wait(timeout=5)
    except subprocess.TimeoutExpired:
        process.kill()
        process.wait(timeout=5)


def probe(command, args, env):
    child = subprocess.Popen([command, *args], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                             stderr=subprocess.DEVNULL, text=True, encoding="utf-8", env=env,
                             creationflags=subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0)
    messages = queue.Queue()

    def read():
        for line in child.stdout:
            try:
                messages.put(json.loads(line))
            except ValueError:
                pass

    reader = threading.Thread(target=read, daemon=True)
    reader.start()

    def request(identifier, method, params):
        child.stdin.write(json.dumps({"jsonrpc": "2.0", "id": identifier, "method": method, "params": {**params, "_meta": {
            "io.modelcontextprotocol/protocolVersion": "2026-07-28",
            "io.modelcontextprotocol/clientCapabilities": {},
            "io.modelcontextprotocol/clientInfo": {"name": "harness-registration-smoke", "version": "1"},
        }}}) + "\n")
        child.stdin.flush()
        deadline = time.monotonic() + 20
        while time.monotonic() < deadline:
            response = messages.get(timeout=max(0.1, deadline - time.monotonic()))
            if response.get("id") == identifier:
                assert "error" not in response and "result" in response, method + " failed"
                assert not response["result"].get("isError"), method + " returned a tool error"
                return response["result"]
        raise TimeoutError(method)

    try:
        request(1, "server/discover", {})
        tools = request(2, "tools/list", {})["tools"]
        assert any(tool["name"] == "health" for tool in tools), "health missing"
        request(3, "tools/call", {"name": "health", "arguments": {}})
        return len(tools)
    finally:
        child.stdin.close()
        stop(child)
        reader.join(timeout=2)
        child.stdout.close()


def main():
    source = Path(sys.argv[1]).resolve(strict=True)
    suffix = ".exe" if os.name == "nt" else ""
    daemon_source = source.with_name("terminal-commanderd" + suffix)
    assert daemon_source.is_file(), "sibling daemon binary missing"
    with tempfile.TemporaryDirectory(prefix="tc-harness-smoke-") as directory:
        root = Path(directory)
        stable = root / "stable bin"
        stable.mkdir()
        binary = stable / source.name
        daemon_binary = stable / daemon_source.name
        shutil.copy2(source, binary)
        shutil.copy2(daemon_source, daemon_binary)
        output = subprocess.run(["node", "-e", GENERATE], input=json.dumps({"repo": str(REPO),
                                "root": str(root), "binary": str(binary)}), text=True,
                                stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, check=True)
        configs = json.loads(output.stdout)
        env = os.environ.copy()
        env.update(TC_DATA=str(root / "data"), TC_SOCKET=(r"\\.\pipe\tc-harness-smoke-" + uuid.uuid4().hex
                   if os.name == "nt" else str(root / "daemon.sock")), TC_IDLE_TTL_SECS="0",
                   TC_SUPERVISOR_ALLOW_SPAWN="0")
        daemon = subprocess.Popen([str(daemon_binary), "start"], env=env, stdout=subprocess.DEVNULL,
                                  stderr=subprocess.DEVNULL,
                                  creationflags=subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0)
        try:
            time.sleep(1)
            assert daemon.poll() is None, "owned daemon exited before probe"
            for config in configs:
                target = Path(config["path"])
                data = tomllib.loads(target.read_text()) if target.suffix == ".toml" else json.loads(target.read_text())
                servers = data.get("mcpServers", data.get("mcp_servers", data.get("mcp")))
                entry = next(iter(servers.values()))
                command = entry["command"]
                args = command[1:] if isinstance(command, list) else entry["args"]
                command = command[0] if isinstance(command, list) else command
                assert command == str(binary) and args == [], "incoherent native launch"
                client_env = {**env, **entry.get("env", entry.get("environment", {}))}
                for attempt in range(2):
                    count = probe(command, args, client_env)
                    print(config["id"], "first" if attempt == 0 else "reconnect", "PASS", count, "tools", flush=True)
        finally:
            stop(daemon)


if __name__ == "__main__":
    main()
