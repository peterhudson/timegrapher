"""Offline artifact-smoke controls; process doubles never analyze audio."""

import copy
import io
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import unittest
from contextlib import redirect_stderr, redirect_stdout
from unittest.mock import patch
import wave

import artifact_smoke as smoke


def document():
    return {"schema": "timegrapher.analyze/2", "watch_heard": True,
            "input": {"settings": {"bph": 28800, "lift_deg": 52}},
            "overall": {"rate_s_per_day": 6.01}, "amplitude_deg": 279.5,
            "beat_error_unlock_ms": 0.4}


def profile():
    return {"schema": "timegrapher.profile/1", "input": document()["input"],
            **{name: {"beats": 20, "median": [1.0], "p10": [0.5], "p90": [1.5]}
               for name in ("a", "b")}}


def fake_mcp(mode):
    """A real subprocess exercising pipes/IDs/EOF, with constructed JSON."""
    initialized = False
    for line in sys.stdin:
        message = json.loads(line)
        if "id" not in message:
            initialized = message.get("method") == "notifications/initialized"
            continue
        if mode == "hang":
            time.sleep(60)
        if mode == "garbage":
            print("not JSON", flush=True)
            continue
        if mode == "early-exit":
            return 3
        method = message["method"]
        if method == "initialize":
            result = {"protocolVersion": "2025-06-18", "capabilities": {},
                      "serverInfo": {"name": "timegrapher", "version": "test"}}
            if mode == "bad-init":
                result = {}
        elif method == "tools/list":
            assert initialized, "initialized notification must precede tools/list"
            result = {"tools": [] if mode == "missing-tool" else [{"name": "analyze_recording"}]}
        else:
            assert method == "tools/call"
            assert message["params"]["name"] == "analyze_recording"
            assert message["params"]["arguments"]["bph"] == 28800
            assert message["params"]["arguments"]["lift_deg"] == 52
            result = {"content": [{"type": "text", "text": "constructed test double"}],
                      "structuredContent": document(),
                      "_meta": {"timegrapher/charts": {"profile": profile()}}}
            if mode == "tool-error":
                result["isError"] = True
            elif mode == "null-amplitude":
                result["structuredContent"]["amplitude_deg"] = None
            elif mode == "missing-text":
                result["content"] = []
            elif mode == "numeric-error":
                result["isError"] = 0
            elif mode == "missing-profile":
                del result["_meta"]
        response = {"jsonrpc": "2.0", "id": message["id"], "result": result}
        if mode == "rpc-error":
            response = {"jsonrpc": "2.0", "id": message["id"], "error": {"code": -1}}
        elif mode == "wrong-id":
            response["id"] += 1
        elif mode == "boolean-id":
            response["id"] = True
        elif mode == "float-id":
            response["id"] = float(message["id"])
        print("fixture stderr", file=sys.stderr, flush=True)
        print(json.dumps({"jsonrpc": "2.0", "method": "notifications/message", "params": {}}), flush=True)
        print(json.dumps(response), flush=True)
    return 3 if mode == "bad-eof-exit" else 0


class ArtifactSmokeTests(unittest.TestCase):
    def test_analysis_requires_heard_schema_profile_and_all_finite_quantities(self):
        self.assertEqual(smoke.validate_analysis(document())["rate_s_per_day"], 6.01)
        for field, value in [("schema", "old"), ("watch_heard", False)]:
            doc = document()
            doc[field] = value
            with self.assertRaises(smoke.SmokeError):
                smoke.validate_analysis(doc)
        for quantity in smoke.QUANTITIES:
            for bad in [None, True, "6", float("nan"), float("inf")]:
                doc = document()
                if quantity == "rate_s_per_day":
                    doc["overall"][quantity] = bad
                else:
                    doc[quantity] = bad
                with self.subTest(quantity=quantity, bad=bad):
                    with self.assertRaises(smoke.SmokeError):
                        smoke.validate_analysis(doc)
        for bph, lift in [(21600, 52), (28800, 50)]:
            doc = document()
            doc["input"]["settings"] = {"bph": bph, "lift_deg": lift}
            with self.assertRaises(smoke.SmokeError):
                smoke.validate_analysis(doc)
        doc = document()
        doc["overall"]["rate_s_per_day"] = 7
        with self.assertRaises(smoke.SmokeError):
            smoke.validate_analysis(doc)

    def test_missing_extracted_binary_fails_before_execution(self):
        with tempfile.TemporaryDirectory() as temp:
            suffix = ".exe" if os.name == "nt" else ""
            for present in [None, "timegrapher", "timegrapher-mcp"]:
                with self.subTest(present=present):
                    path = Path(temp) / (str(present) + suffix)
                    if present:
                        path.write_text("constructed file", encoding="utf-8")
                    with patch.object(smoke, "run_command") as run:
                        with self.assertRaisesRegex(smoke.SmokeError, "Required extracted executable"):
                            smoke.smoke(temp, "missing")
                        run.assert_not_called()
                    if present:
                        path.unlink()

    def test_commands_fail_on_nonzero_and_timeout(self):
        with self.assertRaisesRegex(smoke.SmokeError, "fixture failure"):
            smoke.run_command([sys.executable, "-c",
                               "import sys; print('fixture failure',file=sys.stderr); sys.exit(4)"],
                              dict(os.environ))
        with self.assertRaisesRegex(smoke.SmokeError, "timed out"):
            smoke.run_command([sys.executable, "-c", "import time; time.sleep(60)"],
                              dict(os.environ), timeout=0.2)

    def test_owned_process_group_and_bounded_windows_tree_cleanup_contract(self):
        self.assertEqual(smoke.process_options(False), {"start_new_session": True})
        self.assertEqual(smoke.process_options(True)["creationflags"], 0x200)
        from unittest.mock import Mock
        child = Mock(pid=12345)
        child.poll.return_value = None
        with patch.object(smoke.subprocess, "run") as taskkill:
            smoke.terminate_tree(child, windows=True)
        args, kwargs = taskkill.call_args
        self.assertEqual(args[0][1:], ["/PID", "12345", "/T", "/F"])
        self.assertEqual(kwargs["timeout"], 10)
        child.kill.assert_called_once()
        child.wait.assert_called_once_with(timeout=10)
        child.reset_mock()
        with patch.object(smoke.subprocess, "run", side_effect=OSError("taskkill unavailable")):
            smoke.terminate_tree(child, windows=True)
        child.kill.assert_called_once()
        child.wait.assert_called_once_with(timeout=10)

    def test_cleanup_errors_do_not_abandon_other_pipe_closes(self):
        from unittest.mock import Mock
        client = smoke.McpClient.__new__(smoke.McpClient)
        client.child = Mock()
        client.child.poll.return_value = 3
        client.readers = [Mock(), Mock()]
        for reader in client.readers:
            reader.is_alive.return_value = False
        streams = [client.child.stdin, client.child.stdout, client.child.stderr]
        for stream in streams:
            stream.closed = False
        client.child.stdin.close.side_effect = BrokenPipeError("fixture stdin close")
        with patch.object(smoke, "terminate_tree", return_value="fixture group termination"):
            with self.assertRaises(smoke.SmokeError) as error:
                client.close(False)
        self.assertIn("fixture group termination", str(error.exception))
        self.assertIn("fixture stdin close", str(error.exception))
        for stream in streams:
            stream.close.assert_called_once()

    def test_real_portable_stdio_transport_success_and_failures_reap_children(self):
        for mode in ["success", "garbage", "rpc-error", "wrong-id", "boolean-id",
                     "bad-init", "missing-tool", "tool-error", "null-amplitude",
                     "missing-text", "numeric-error", "missing-profile", "float-id",
                     "bad-eof-exit", "early-exit", "hang"]:
            with self.subTest(mode=mode):
                clients = []
                real_client = smoke.McpClient

                def client(*args, **kwargs):
                    instance = real_client(*args, **kwargs)
                    clients.append(instance)
                    return instance

                with patch.object(smoke, "McpClient", side_effect=client):
                    command = [sys.executable, str(Path(__file__).resolve()), "--fake-mcp", mode]
                    if mode == "success":
                        self.assertEqual(smoke.mcp_analysis(command, dict(os.environ),
                                                           Path("/constructed.wav"), timeout=10), document())
                    else:
                        with self.assertRaises(smoke.SmokeError) as error:
                            smoke.mcp_analysis(command, dict(os.environ), Path("/constructed.wav"),
                                               timeout=0.2 if mode == "hang" else 10)
                        if mode == "rpc-error":
                            self.assertIn("fixture stderr", str(error.exception))
                self.assertEqual(len(clients), 1)
                self.assertIsNotNone(clients[0].child.poll())
                self.assertTrue(all(not thread.is_alive() for thread in clients[0].readers))
                self.assertTrue(all(stream.closed for stream in
                                    (clients[0].child.stdin, clients[0].child.stdout, clients[0].child.stderr)),
                                f"unclosed pipes after {mode}")

    def test_profile_requires_both_populated_finite_sides_and_settings(self):
        result = {"_meta": {"timegrapher/charts": {"profile": profile()}}}
        smoke.validate_profile(result)
        for defect in ["schema", "settings", "side", "beats", "empty", "nan", "bool", "length"]:
            bad = copy.deepcopy(result)
            p = bad["_meta"]["timegrapher/charts"]["profile"]
            if defect == "schema":
                p["schema"] = "old"
            elif defect == "settings":
                p["input"]["settings"]["lift_deg"] = 50
            elif defect == "side":
                p["b"] = None
            elif defect == "beats":
                p["a"]["beats"] = False
            elif defect == "empty":
                p["b"]["median"] = []
            elif defect == "nan":
                p["a"]["p10"] = [float("nan")]
            elif defect == "bool":
                p["a"]["p10"] = [True]
            else:
                p["a"]["p90"] = [1, 2]
            with self.subTest(defect=defect):
                with self.assertRaises(smoke.SmokeError):
                    smoke.validate_profile(bad)

    def test_smoke_recipe_sibling_environment_parity_and_cleanup(self):
        with tempfile.TemporaryDirectory() as temp:
            binary_dir = Path(temp) / "extracted"
            binary_dir.mkdir()
            suffix = ".exe" if os.name == "nt" else ""
            for name in ["timegrapher", "timegrapher-mcp"]:
                (binary_dir / (name + suffix)).write_text("constructed executable", encoding="utf-8")
            fixtures = []
            environments = []

            def run(command, env):
                environments.append(copy.deepcopy(env))
                if command[1] == "synth":
                    self.assertEqual(tuple(command[3:]), smoke.RECIPE)
                    path = Path(command[2])
                    fixtures.append(path)
                    with wave.open(str(path), "wb") as wav:
                        wav.setparams((1, 2, 48000, 0, "NONE", "not compressed"))
                        wav.writeframes(b"\0" * (40 * 48000 * 2))
                    return subprocess.CompletedProcess(command, 0, "", "")
                self.assertIn("--card-ppm=0", command)
                return subprocess.CompletedProcess(command, 0, json.dumps(document()), "")

            with patch.dict(os.environ, {"TIMEGRAPHER_CLI": "wrong-cli",
                                         "TIMEGRAPHER_MCP_TEST_INPUT": "wrong-input",
                                         "PATH": str(binary_dir) + os.pathsep + os.defpath}):
                with patch.object(smoke, "run_command", side_effect=run):
                    with patch.object(smoke, "mcp_analysis", return_value=document()) as mcp:
                        report = smoke.smoke(binary_dir, "constructed")
            self.assertEqual(report["quantities"], smoke.validate_analysis(document()))
            self.assertEqual(report["fixture_default_seed"], 1)
            self.assertEqual(len(report["fixture_sha256"]), 64)
            self.assertTrue(all(not p.exists() for p in fixtures))
            for env in environments:
                self.assertNotIn("TIMEGRAPHER_CLI", env)
                self.assertNotIn("TIMEGRAPHER_MCP_TEST_INPUT", env)
                self.assertNotIn(str(binary_dir), env["PATH"].split(os.pathsep))
                self.assertIn("TIMEGRAPHER_CONFIG_DIR", env)
            self.assertEqual(Path(mcp.call_args[0][0][0]).parent, binary_dir.resolve())

    def test_malformed_fixture_json_and_quantity_parity_fail(self):
        with tempfile.TemporaryDirectory() as temp:
            suffix = ".exe" if os.name == "nt" else ""
            for name in ["timegrapher", "timegrapher-mcp"]:
                (Path(temp) / (name + suffix)).write_text("fake", encoding="utf-8")
            for defect in ["empty", "json", "parity"]:
                def run(command, env):
                    if command[1] == "synth":
                        with wave.open(command[2], "wb") as wav:
                            wav.setparams((1, 2, 48000, 0, "NONE", "not compressed"))
                            if defect != "empty":
                                wav.writeframes(b"\0" * (40 * 48000 * 2))
                        return subprocess.CompletedProcess(command, 0, "", "")
                    return subprocess.CompletedProcess(command, 0,
                                                       "not JSON" if defect == "json" else json.dumps(document()), "")
                remote = document()
                remote["amplitude_deg"] += 1
                with patch.object(smoke, "run_command", side_effect=run):
                    with patch.object(smoke, "mcp_analysis", return_value=remote):
                        with self.assertRaises((smoke.SmokeError, ValueError)):
                            smoke.smoke(temp, defect)

    def test_entrypoint_reports_pass_and_failure(self):
        for failure in [False, True]:
            with patch.object(sys, "argv", ["artifact_smoke.py", "extracted", "--label", "offline"]):
                with patch.dict(os.environ, {"GITHUB_STEP_SUMMARY": ""}):
                    with patch.object(smoke, "smoke", side_effect=smoke.SmokeError("fixture failure") if failure else None,
                                      return_value={"result": "constructed pass"}):
                        with redirect_stdout(io.StringIO()) as out, redirect_stderr(io.StringIO()) as err:
                            self.assertEqual(smoke.main(), int(failure))
                    self.assertIn("FAILED" if failure else "constructed pass",
                                  err.getvalue() if failure else out.getvalue())


if __name__ == "__main__":
    if len(sys.argv) > 1 and sys.argv[1] == "--fake-mcp":
        sys.exit(fake_mcp(sys.argv[2]))
    unittest.main()
