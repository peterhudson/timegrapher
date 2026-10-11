#!/usr/bin/env python3
"""Run timegrapher-mcp behind a stdio relay that hides MCP protocol revision
2026-07-28 from the client.

Why: draft 0.1.0 (rmcp 3.5.1) offers 2026-07-28 in its server/discover answer
but leaves the required `resultType` out of tools/list, prompts/list and
resources/list, so a client on that revision (Claude Code 2.1.296) drops every
tool. Hiding the revision makes the client fall back to `initialize` on an
older one, where the lists are fine. Reported to the MCP thread 2026-10-11;
round.py uses this only while `--mcp-shim` is on, and each report says so.

    mcp-shim.py SERVER [ARGS...]
"""
import json
import subprocess
import sys
import threading

HIDDEN = "2026-07-28"

server = subprocess.Popen(sys.argv[1:], stdin=subprocess.PIPE, stdout=subprocess.PIPE)


def to_server():
    for line in sys.stdin.buffer:
        server.stdin.write(line)
        server.stdin.flush()
    server.stdin.close()


threading.Thread(target=to_server, daemon=True).start()
for line in server.stdout:
    try:
        msg = json.loads(line)
        versions = (msg.get("result") or {}).get("supportedVersions")
        if isinstance(versions, list) and HIDDEN in versions:
            msg["result"]["supportedVersions"] = [v for v in versions if v != HIDDEN]
            line = (json.dumps(msg) + "\n").encode()
    except (ValueError, AttributeError):
        pass
    sys.stdout.buffer.write(line)
    sys.stdout.buffer.flush()
sys.exit(server.wait())
