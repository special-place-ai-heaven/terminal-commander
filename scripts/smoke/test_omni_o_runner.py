import importlib.util
from pathlib import Path
import subprocess
import unittest


RUNNER = Path(__file__).with_name("omni-o-runner.py")
SPEC = importlib.util.spec_from_file_location("omni_o_runner", RUNNER)
omni = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(omni)


class CompactRoutingTests(unittest.TestCase):
    def test_every_legacy_omni_call_routes_through_compact_facades(self):
        compact_tools = {"command", "files", "registry", "session", "status"}
        expected = {
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

        for legacy, (facade, action) in expected.items():
            with self.subTest(legacy=legacy):
                tool, arguments = omni._route_call(legacy, {}, compact_tools)
                self.assertEqual(tool, facade)
                self.assertEqual(arguments["action"], action)

    def test_full_surface_call_is_preserved(self):
        arguments = {"argv": ["python", "--version"]}
        tool, routed = omni._route_call(
            "command_start_combed", arguments, {"command_start_combed"}
        )
        self.assertEqual(tool, "command_start_combed")
        self.assertEqual(routed, arguments)

    def test_shell_grace_is_translated_to_compact_wait(self):
        tool, arguments = omni._route_call(
            "shell_exec",
            {"shell_line": "echo a | wc -c", "grace_ms": 2000},
            {"command"},
        )
        self.assertEqual(tool, "command")
        self.assertEqual(arguments["action"], "exec")
        self.assertEqual(arguments["wait_ms"], 2000)
        self.assertNotIn("grace_ms", arguments)


class PortableFixtureTests(unittest.TestCase):
    def test_portable_echo_is_a_real_executable_and_emits_exact_text(self):
        result = subprocess.run(
            omni._portable_echo("omni portable"),
            check=False,
            capture_output=True,
            text=True,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, "omni portable\n")


if __name__ == "__main__":
    unittest.main()
