"""Exercise extracted CLI/MCP binaries, without hardware or release operations.

The deterministic recipe and rate margin reuse tests/stdio.rs. This is an
operability test of shipped programs, not an independent acoustic calibration.
"""

import argparse
from collections import deque
import hashlib
import json
import math
import os
from pathlib import Path
import queue
import signal
import subprocess
import sys
import tempfile
import threading
import time
import wave


RECIPE = ("--duration", "40", "--bph", "28800", "--rate=6",
          "--amplitude", "280", "--beat-error", "0.4", "--snr", "30")
QUANTITIES = ("rate_s_per_day", "amplitude_deg", "beat_error_unlock_ms")


class SmokeError(Exception):
    pass


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def validate_analysis(document):
    if not isinstance(document, dict) or document.get("schema") != "timegrapher.analyze/2":
        raise SmokeError("Expected timegrapher.analyze/2 document")
    if document.get("watch_heard") is not True:
        raise SmokeError("Synthetic watch was not heard")
    settings = document.get("input", {}).get("settings", {})
    if settings.get("bph") != 28800 or settings.get("lift_deg") != 52:
        raise SmokeError("Analysis did not retain explicit BPH/lift settings")
    values = {
        "rate_s_per_day": document.get("overall", {}).get("rate_s_per_day"),
        "amplitude_deg": document.get("amplitude_deg"),
        "beat_error_unlock_ms": document.get("beat_error_unlock_ms"),
    }
    for name, value in values.items():
        if isinstance(value, bool) or not isinstance(value, (int, float)) or not math.isfinite(value):
            raise SmokeError(f"Missing/nonfinite synthetic {name}: {value!r}")
    if abs(values["rate_s_per_day"] - 6.0) >= 1.0:
        raise SmokeError("Synthetic rate outside existing MCP stdio test margin (+6 +/-1 s/day)")
    return values


def validate_profile(result):
    profile = result.get("_meta", {}).get("timegrapher/charts", {}).get("profile")
    if not isinstance(profile, dict) or profile.get("schema") != "timegrapher.profile/1":
        raise SmokeError("MCP omitted the CLI-backed profile document")
    settings = profile.get("input", {}).get("settings", {})
    if settings.get("bph") != 28800 or settings.get("lift_deg") != 52:
        raise SmokeError("MCP profile lost explicit BPH/lift settings")
    for name in ("a", "b"):
        side = profile.get(name)
        if (not isinstance(side, dict) or type(side.get("beats")) is not int
                or side["beats"] <= 0):
            raise SmokeError(f"MCP profile side {name} has no beats")
        for series in ("median", "p10", "p90"):
            values = side.get(series)
            if (not isinstance(values, list) or not values
                    or any(isinstance(v, bool) or not isinstance(v, (int, float))
                           or not math.isfinite(v) for v in values)):
                raise SmokeError(f"MCP profile side {name} has invalid {series}")
        if not len(side["median"]) == len(side["p10"]) == len(side["p90"]):
            raise SmokeError(f"MCP profile side {name} has mismatched point counts")


def process_options(windows=os.name == "nt"):
    if windows:
        return {"creationflags": getattr(subprocess, "CREATE_NEW_PROCESS_GROUP", 0x200)}
    return {"start_new_session": True}


def terminate_tree(child, windows=os.name == "nt"):
    """Only terminate the group/tree created for this smoke invocation."""
    detail = None
    if windows:
        taskkill = str(Path(os.environ.get("SystemRoot", r"C:\Windows")) / "System32" / "taskkill.exe")
        try:
            result = subprocess.run([taskkill, "/PID", str(child.pid), "/T", "/F"],
                                    stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                                    stderr=subprocess.PIPE, timeout=10, check=False)
            if result.returncode:
                detail = f"Windows tree termination exited {result.returncode}: {result.stderr!r}"
        except (OSError, subprocess.TimeoutExpired) as error:
            detail = f"Windows tree termination unavailable: {error}"
    else:
        try:
            os.killpg(child.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        except OSError as error:
            detail = f"Unix group termination failed: {error}"
    if child.poll() is None:
        child.kill()
    child.wait(timeout=10)
    return detail


def run_command(command, env, timeout=120):
    child = None
    drained = False
    try:
        child = subprocess.Popen(command, env=env, stdin=subprocess.DEVNULL,
                                 stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                 text=True, encoding="utf-8", errors="replace", **process_options())
        try:
            stdout, stderr = child.communicate(timeout=timeout)
        except subprocess.TimeoutExpired:
            cleanup = terminate_tree(child)
            # Bound draining too: a failed OS tree kill must not hang this job.
            stdout, stderr = child.communicate(timeout=10)
            drained = True
            raise SmokeError(f"Command timed out: {command!r}; stderr: {stderr}; cleanup: {cleanup}")
        drained = True
        result = subprocess.CompletedProcess(command, child.returncode, stdout, stderr)
    except (OSError, subprocess.TimeoutExpired) as error:
        raise SmokeError(f"Command failed: {command!r}: {error}; "
                         f"stderr: {getattr(error, 'stderr', None)}") from error
    finally:
        if child is not None and child.poll() is None:
            terminate_tree(child)
        if child is not None and drained:
            for stream in (child.stdout, child.stderr):
                stream.close()
    if result.returncode:
        raise SmokeError(f"Command {command!r} exited {result.returncode}: {result.stderr}")
    return result


class McpClient:
    """Bounded newline JSON-RPC transport using portable pipe-reader threads."""

    def __init__(self, command, env, timeout=120):
        self.timeout = timeout
        self.next_id = 1
        self.messages = queue.Queue(maxsize=128)
        self.stderr = deque(maxlen=16)
        self.overflow = threading.Event()
        self.child = subprocess.Popen(command, env=env, stdin=subprocess.PIPE,
                                      stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                      text=True, encoding="utf-8", errors="replace", bufsize=1,
                                      **process_options())
        self.readers = [threading.Thread(target=self._stdout, daemon=True),
                        threading.Thread(target=self._stderr, daemon=True)]
        for reader in self.readers:
            reader.start()

    def _stdout(self):
        while True:
            line = self.child.stdout.readline(1024 * 1024 + 1)
            try:
                self.messages.put(line, timeout=0.1)
            except queue.Full:
                self.overflow.set()
                return
            if not line or len(line) > 1024 * 1024:
                return

    def _stderr(self):
        while True:
            text = self.child.stderr.read(4096)
            if not text:
                return
            self.stderr.append(text)

    def diagnostic(self):
        return "".join(self.stderr)

    def send(self, message):
        try:
            self.child.stdin.write(json.dumps(message, allow_nan=False) + "\n")
            self.child.stdin.flush()
        except (OSError, ValueError) as error:
            raise SmokeError(f"MCP send failed: {error}") from error

    def request(self, method, params):
        request_id = self.next_id
        self.next_id += 1
        self.send({"jsonrpc": "2.0", "id": request_id, "method": method, "params": params})
        deadline = time.monotonic() + self.timeout
        while True:
            remaining = deadline - time.monotonic()
            if remaining <= 0 or self.overflow.is_set():
                raise SmokeError(f"MCP {method}: response timeout/output overflow")
            try:
                line = self.messages.get(timeout=remaining)
            except queue.Empty as error:
                raise SmokeError(f"MCP {method}: response timeout") from error
            if not line:
                raise SmokeError(f"MCP {method}: server closed stdout")
            if len(line) > 1024 * 1024:
                raise SmokeError("MCP response exceeds smoke-test size bound")
            try:
                message = json.loads(line)
            except ValueError as error:
                raise SmokeError(f"MCP stdout is not JSON: {line[:200]!r}") from error
            if not isinstance(message, dict) or message.get("jsonrpc") != "2.0":
                raise SmokeError("Malformed MCP JSON-RPC response")
            if "id" not in message:
                if not isinstance(message.get("method"), str):
                    raise SmokeError("Malformed MCP notification")
                continue
            if type(message["id"]) is not int or message["id"] != request_id:
                raise SmokeError(f"MCP {method}: unexpected response ID {message['id']!r}")
            if "error" in message or not isinstance(message.get("result"), dict):
                raise SmokeError(f"MCP {method}: error or missing result: {message}")
            return message["result"]

    def close(self, successful=False):
        # EOF is the protocol shutdown used by the stdio transport. On failure,
        # terminate promptly; always reap before releasing pipe handles.
        errors = []

        def stop():
            try:
                detail = terminate_tree(self.child)
                if detail:
                    errors.append(detail)
            except Exception as error:
                errors.append(str(error))

        try:
            if successful:
                self.child.stdin.close()
                try:
                    code = self.child.wait(timeout=10)
                except subprocess.TimeoutExpired as error:
                    raise SmokeError("MCP did not exit after stdin EOF") from error
                if code:
                    raise SmokeError(f"MCP exited {code} after smoke calls")
        except Exception as error:
            errors.append(str(error))
        finally:
            if not successful or self.child.poll() is None:
                stop()
            for reader in self.readers:
                reader.join(timeout=2)
            alive = [reader.is_alive() for reader in self.readers]
            if any(alive):
                stop()
                for reader in self.readers:
                    reader.join(timeout=2)
                alive = [reader.is_alive() for reader in self.readers]
            # Closing a pipe from another thread while it holds a read lock
            # can block forever if a surviving descendant retains that pipe.
            for stream, reading in [(self.child.stdin, False),
                                    (self.child.stdout, alive[0]), (self.child.stderr, alive[1])]:
                if not reading and not stream.closed:
                    try:
                        stream.close()
                    except OSError as error:
                        errors.append(f"MCP pipe close failed: {error}")
            if any(alive):
                errors.append("MCP pipe readers did not stop after bounded process-tree cleanup")
        if errors:
            raise SmokeError("; ".join(errors))


def mcp_analysis(command, env, recording, timeout=120):
    client = McpClient(command, env, timeout)
    successful = False
    failure = None
    try:
        initialized = client.request("initialize", {
            "protocolVersion": "2025-06-18", "capabilities": {},
            "clientInfo": {"name": "release-artifact-smoke", "version": "1"},
        })
        if (initialized.get("protocolVersion") != "2025-06-18"
                or initialized.get("serverInfo", {}).get("name") != "timegrapher"
                or not isinstance(initialized.get("capabilities"), dict)):
            raise SmokeError("MCP initialize identity/version/capabilities missing")
        client.send({"jsonrpc": "2.0", "method": "notifications/initialized"})
        listing = client.request("tools/list", {})
        tools = listing.get("tools")
        if not isinstance(tools, list) or not any(
                isinstance(tool, dict) and tool.get("name") == "analyze_recording" for tool in tools):
            raise SmokeError("MCP tools/list omitted analyze_recording")
        result = client.request("tools/call", {"name": "analyze_recording", "arguments": {
            "recording": str(recording), "bph": 28800, "lift_deg": 52,
        }})
        if result.get("isError") is not None and result.get("isError") is not False:
            raise SmokeError(f"MCP analysis failed: {result}")
        content = result.get("content")
        if not isinstance(content, list) or not any(
                isinstance(item, dict) and item.get("type") == "text"
                and isinstance(item.get("text"), str) and item["text"] for item in content):
            raise SmokeError("MCP analysis omitted text content")
        document = result.get("structuredContent")
        validate_analysis(document)
        validate_profile(result)
        successful = True
    except Exception as error:
        failure = error
    finally:
        try:
            client.close(successful)
        except Exception as error:
            failure = SmokeError(f"{failure}; cleanup: {error}") if failure else error
    if failure is not None:
        raise SmokeError(f"{failure}\nMCP stderr: {client.diagnostic()}") from failure
    return document


def smoke(binary_dir, label):
    binary_dir = Path(binary_dir).resolve()
    suffix = ".exe" if os.name == "nt" else ""
    cli, server = [binary_dir / (name + suffix) for name in ("timegrapher", "timegrapher-mcp")]
    for executable in (cli, server):
        if not executable.is_file():
            raise SmokeError(f"Required extracted executable missing: {executable}")
    report = {"label": label, "binary_dir": str(binary_dir),
              "executables": {p.name: digest(p) for p in (cli, server)},
              "recipe": ["synth", "fixture.wav", *RECIPE],
              "fixture_default_seed": 1, "sample_rate": 48000}
    print("Artifact smoke inputs: " + json.dumps(report), file=sys.stderr, flush=True)
    with tempfile.TemporaryDirectory(prefix="timegrapher-artifact-smoke-") as temp:
        work = Path(temp)
        env = dict(os.environ)
        for name in ("TIMEGRAPHER_CLI", "TIMEGRAPHER_MCP_TEST_INPUT"):
            env.pop(name, None)
        env["TIMEGRAPHER_CONFIG_DIR"] = str(work / "config")
        # Keep OS runtime lookup intact, but exclude directories that could
        # supply a different timegrapher. The required shipped sibling remains.
        env["PATH"] = os.pathsep.join(entry for entry in env.get("PATH", "").split(os.pathsep)
                                      if entry and not any((Path(entry) / (name + suffix)).is_file()
                                                           for name in ("timegrapher", "timegrapher-mcp")))
        fixture = work / "fixture.wav"
        run_command([str(cli), "synth", str(fixture), *RECIPE], env)
        with wave.open(str(fixture), "rb") as wav:
            if (wav.getnchannels(), wav.getframerate(), wav.getnframes()) != (1, 48000, 40 * 48000):
                raise SmokeError("Synthetic fixture is missing/empty or has the wrong duration/rate")
        report["fixture_sha256"] = digest(fixture)
        print("Artifact smoke fixture SHA-256: " + report["fixture_sha256"], file=sys.stderr, flush=True)
        direct = run_command([str(cli), "analyze", str(fixture), "--bph", "28800",
                              "--lift", "52", "--card-ppm=0", "--json"], env)
        try:
            document = json.loads(direct.stdout)
            values = validate_analysis(document)
        except (ValueError, SmokeError) as error:
            raise SmokeError(f"CLI analysis invalid: {error}; stderr: {direct.stderr}; "
                             f"stdout: {direct.stdout[:1000]}") from error
        remote = mcp_analysis([str(server)], env, fixture)
        remote_values = validate_analysis(remote)
        if remote_values != values:
            raise SmokeError(f"Packaged CLI/MCP raw quantities differ: {values} versus {remote_values}")
        report["quantities"] = values
        report["mcp"] = "initialize, tools/list, analyze_recording with CLI-backed profile, clean EOF exit: passed"
        report["result"] = "passed (synthetic operability, not acoustic calibration or host UI mounting)"
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary_dir", type=Path)
    parser.add_argument("--label", required=True)
    args = parser.parse_args()
    try:
        report = smoke(args.binary_dir, args.label)
    except Exception as error:
        message = f"Artifact smoke FAILED ({args.label}): {error}"
        print(message, file=sys.stderr)
        if os.environ.get("GITHUB_STEP_SUMMARY"):
            with open(os.environ["GITHUB_STEP_SUMMARY"], "a", encoding="utf-8") as output:
                output.write(message + "\n")
        return 1
    print(json.dumps(report, indent=2, allow_nan=False))
    if os.environ.get("GITHUB_STEP_SUMMARY"):
        with open(os.environ["GITHUB_STEP_SUMMARY"], "a", encoding="utf-8") as output:
            output.write(f"### Artifact smoke: {args.label}\n\n```json\n"
                         + json.dumps(report, indent=2, allow_nan=False) + "\n```\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
