#!/usr/bin/env python3
"""Run a persona round: watch people use a build of the timegrapher.

    round.py setup --build v0.1.0 [--work DIR]     get the build, the corpus and the virtual screens ready
    round.py setup --build COMMIT [--work DIR]     same, building COMMIT from source instead
    round.py run --out DIR [--only ID ...] [--work DIR] [--model M]
                                                   run the personas (all of them by default) in parallel,
                                                   then write DIR/triage.md

See ../README.md for what a round is and how its findings are routed.
"""

import argparse
import concurrent.futures
import datetime
import glob
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import threading
import time
import tomllib
import urllib.request

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)  # personas/
REPO = os.path.dirname(ROOT)
CODE_REPO = "peterhudson/timegrapher"
CORPUS_REPO = "peterhudson/timegrapher-corpus"
APT = ["xvfb", "xdotool", "imagemagick", "pulseaudio", "pulseaudio-utils",
       "libasound2-plugins", "zenity", "libxkbcommon-x11-0", "libgl1",
       "libegl1", "libgl1-mesa-dri", "libxcursor1", "libxrandr2", "libxi6", "openbox"]
# The only variables a persona's `claude` inherits from the session running the
# round: network and sign-in. Anything else (the parent's session, project,
# memory, tools) would make the persona part of that session.
KEEP_ENV = ["PATH", "LANG", "ANTHROPIC_BASE_URL", "ANTHROPIC_API_KEY", "HTTPS_PROXY", "https_proxy",
            "HTTP_PROXY", "http_proxy", "NO_PROXY", "no_proxy", "SSL_CERT_FILE", "NODE_EXTRA_CA_CERTS",
            "REQUESTS_CA_BUNDLE", "CLAUDE_SESSION_INGRESS_TOKEN_FILE"]


def clean_env(**extra):
    env = {k: os.environ[k] for k in KEEP_ENV if k in os.environ}
    env.update(extra)
    return env
MAX_MESSAGES = 15
MIN_ACTIONS = 40  # app personas are sent back until they have done this much
print_lock = threading.Lock()


def log(who, msg):
    with print_lock:
        print(f"{time.strftime('%H:%M:%S')} [{who}] {msg}", flush=True)


def sh(cmd, **kw):
    return subprocess.run(cmd, check=True, text=True, capture_output=True, **kw).stdout


def prompt(name):
    with open(os.path.join(ROOT, "prompts", name + ".md")) as f:
        return f.read()


# ---------------------------------------------------------------- setup

def setup(args):
    work = os.path.abspath(args.work)
    os.makedirs(work, exist_ok=True)
    missing = [p for p in APT if subprocess.run(["dpkg", "-s", p], capture_output=True).returncode]
    if missing:
        log("setup", "installing " + " ".join(missing))
        subprocess.run(["apt-get", "update", "-q"], capture_output=True)
        sh(["apt-get", "install", "-y", "-q", *missing])

    build = os.path.join(work, "build")
    shutil.rmtree(build, ignore_errors=True)
    os.makedirs(build)
    if args.build.startswith("v"):
        get_release(args.build, build)
    else:
        build_commit(args.build, build, work)
    version = sh([os.path.join(build, "timegrapher"), "--version"]).strip()
    commit = re.search(r"\b([0-9a-f]{7,40})\b", version)
    commit = commit.group(1) if commit else args.build
    log("setup", f"build: {version}")

    # The watchmaker skill as it was at the build's commit, for the CLI helpers.
    skill = os.path.join(work, "skill")
    shutil.rmtree(skill, ignore_errors=True)
    os.makedirs(skill)
    try:
        subprocess.run(["git", "-C", REPO, "fetch", "-q", "origin", commit], capture_output=True)
        tar = subprocess.run(["git", "-C", REPO, "archive", commit, "skills/watchmaker"],
                             check=True, capture_output=True).stdout
        subprocess.run(["tar", "x", "-C", skill], input=tar, check=True)
    except subprocess.CalledProcessError:
        log("setup", f"could not read the skill at {commit}; using this checkout's")
        shutil.copytree(os.path.join(REPO, "skills"), os.path.join(skill, "skills"), dirs_exist_ok=True)

    corpus = os.path.join(work, "corpus")
    if os.path.isdir(corpus):
        subprocess.run(["git", "-C", corpus, "pull", "-q"], capture_output=True)
    else:
        sh(["git", "clone", "-q", "--depth", "1", f"https://github.com/{CORPUS_REPO}", corpus])

    with open(os.path.join(work, "build.json"), "w") as f:
        json.dump({"build": args.build, "version": version, "commit": commit,
                   "at": datetime.datetime.now(datetime.timezone.utc).isoformat(timespec="seconds")}, f)
    log("setup", f"ready in {work}")


def get_release(tag, dest):
    """Download a release's Linux tar.gz; drafts too, through the REST API."""
    releases = json.loads(sh(["gh", "api", f"repos/{CODE_REPO}/releases"]))
    rel = next((r for r in releases if r["tag_name"] == tag), None)
    if not rel:
        sys.exit(f"no release or draft tagged {tag}")
    asset = next(a for a in rel["assets"] if a["name"].endswith("linux-x86_64.tar.gz"))
    log("setup", f"downloading {asset['name']} from {'draft ' if rel['draft'] else ''}{rel['name']}")
    data = subprocess.run(["gh", "api", "-H", "Accept: application/octet-stream",
                           f"repos/{CODE_REPO}/releases/assets/{asset['id']}"],
                          check=True, capture_output=True).stdout
    subprocess.run(["tar", "xz", "-C", dest, "--strip-components=1"], input=data, check=True)


def build_commit(commit, dest, work):
    subprocess.run(["apt-get", "install", "-y", "-q", "libasound2-dev", "pkg-config"], capture_output=True)
    src = os.path.join(work, "src")
    if not os.path.isdir(src):
        sh(["git", "clone", "-q", f"https://github.com/{CODE_REPO}", src])
    subprocess.run(["git", "-C", src, "fetch", "-q", "origin", commit], capture_output=True)
    sh(["git", "-C", src, "checkout", "-q", "--detach", commit])
    log("setup", f"building {commit}")
    subprocess.run(["cargo", "build", "--release", "-p", "timegrapher-cli", "-p", "timegrapher-mcp",
                    "-p", "timegrapher-app"], cwd=src, check=True)
    for b in ["timegrapher", "timegrapher-mcp", "timegrapher-app"]:
        shutil.copy2(os.path.join(src, "target", "release", b), dest)


# ---------------------------------------------------------------- personas

def load_persona(path):
    text = open(path).read()
    _, front, body = text.split("+++", 2)
    p = tomllib.loads(front)
    p["dossier"] = body.strip()
    p["truth"] = (open(os.path.join(ROOT, "truth", p["id"] + ".md")).read() + "\n\n"
                  + open(os.path.join(ROOT, "truth", "README.md")).read().split("\n", 2)[2])
    return p


def personas():
    return [load_persona(f) for f in sorted(glob.glob(os.path.join(ROOT, "people", "*.md")))]


def fetch(work, wanted):
    """Download corpus files ("take/file" or "take/*") once; return their local paths."""
    corpus = json.load(open(os.path.join(work, "corpus", "corpus.json")))
    takes = {t["id"]: t for t in corpus["takes"]}
    out = {}
    for spec in wanted:
        take, name = spec.split("/", 1)
        t = takes[take]
        files = t["files"] if name == "*" else [f for f in t["files"] if f["name"] == name]
        if not files:
            raise SystemExit(f"{spec} is not in the corpus")
        paths = []
        for f in files:
            path = os.path.join(work, "audio", take, f["name"])
            os.makedirs(os.path.dirname(path), exist_ok=True)
            with fetch_lock:
                if not (os.path.exists(path) and os.path.getsize(path) == f["bytes"]):
                    url = f"https://github.com/{CORPUS_REPO}/releases/download/{t['release']}/{f['name']}"
                    with urllib.request.urlopen(url) as r, open(path + ".part", "wb") as w:
                        shutil.copyfileobj(r, w, 1 << 20)
                    h = hashlib.sha256(open(path + ".part", "rb").read()).hexdigest()
                    if h != f["sha256"]:
                        raise SystemExit(f"SHA-256 mismatch for {spec}")
                    os.replace(path + ".part", path)
            paths.append(path)
        out[spec] = os.path.dirname(paths[0]) if name == "*" else paths[0]
    return out


fetch_lock = threading.Lock()


class Persona:
    def __init__(self, p, work, out, model, display, shim):
        self.p, self.work, self.out, self.model, self.shim = p, work, out, model, shim
        self.id = p["id"]
        self.dir = os.path.join(work, "p", self.id)
        shutil.rmtree(self.dir, ignore_errors=True)
        self.home = os.path.join(self.dir, "home")
        self.stand_dir = os.path.join(self.dir, "stand")
        self.shots = os.path.join(out, self.id)
        shutil.rmtree(self.shots, ignore_errors=True)  # a rerun replaces the persona's screenshots
        self.display = display
        self.build = os.path.join(work, "build")
        self.cost = 0.0
        for d in (self.home, self.stand_dir, os.path.join(self.home, "Recordings")):
            os.makedirs(d, exist_ok=True)

    # The persona's own world: recordings, stand, sound server, screen.
    def prepare(self):
        stand, files = self.p.get("stand", {}), self.p.get("files", {})
        got = fetch(self.work, list(stand.values()) + list(files.values()))
        with open(os.path.join(self.stand_dir, "map.txt"), "w") as f:
            for pos, spec in stand.items():
                f.write(f"{pos}={got[spec]}\n")
        for name, spec in files.items():
            dest = os.path.join(self.home, name)
            os.makedirs(os.path.dirname(dest), exist_ok=True)
            if spec.endswith("/*"):
                os.makedirs(dest, exist_ok=True)
                for src in sorted(glob.glob(os.path.join(got[spec], "*"))):
                    os.symlink(src, os.path.join(dest, os.path.basename(src)))
            else:
                os.symlink(got[spec], dest)
        pulse = os.path.join(self.dir, "pulse")
        os.makedirs(pulse, exist_ok=True)
        script = os.path.join(pulse, "stand.pa")
        with open(script, "w") as f:
            f.write("load-module module-native-protocol-unix\n"
                    "load-module module-null-sink sink_name=stand rate=48000 channels=1 sink_properties=device.description=Stand\n"
                    "load-module module-remap-source master=stand.monitor source_name=standmic rate=48000 channels=1 "
                    "source_properties=device.description=USB_Stand_Mic\n"
                    "set-default-source standmic\n")
        env = dict(os.environ, PULSE_RUNTIME_PATH=pulse, HOME=self.dir)
        subprocess.run(["pulseaudio", "-n", "-F", script, "-D", "--exit-idle-time=-1",
                        "--disallow-exit"], env=env, check=True, capture_output=True)
        self.env = {"PULSE_SERVER": f"unix:{pulse}/native", "STAND_DIR": self.stand_dir}
        if self.p.get("on_stand"):
            self.stand("place", self.p["on_stand"])
        else:
            self.stand("remove")
        if self.p["surface"] == "app":
            self.xvfb = subprocess.Popen(["Xvfb", f":{self.display}", "-screen", "0", "1600x1000x24"],
                                         stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            time.sleep(1)
            # A window manager, as on a desktop: without one, keyboard focus and
            # the file dialogs misbehave in ways no user would see.
            self.wm = subprocess.Popen(["openbox"], env=dict(os.environ, DISPLAY=f":{self.display}"),
                                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            time.sleep(1)

    def stand(self, *args):
        r = subprocess.run([os.path.join(HERE, "stand.sh"), *args], env=dict(os.environ, **self.env),
                           text=True, capture_output=True)
        return (r.stdout + r.stderr).strip()

    def cleanup(self):
        self.stand("remove")
        subprocess.run(["pulseaudio", "--kill"], env=dict(os.environ, PULSE_RUNTIME_PATH=os.path.join(self.dir, "pulse"),
                                                          HOME=self.dir), capture_output=True)
        if getattr(self, "xvfb", None):
            subprocess.run([os.path.join(HERE, "app.sh"), "stop"], env=self.app_env(), capture_output=True)
            self.wm.terminate()
            self.xvfb.terminate()

    # One `claude -p` turn, as its own session.
    def claude(self, message, system, who, resume=None, tools=(), allowed=(), mcp=None,
               cwd=None, settings="", timeout=3600, extra_env=None, append=None):
        env = clean_env(HOME=self.home, **self.env)
        env.update(extra_env or {})
        cmd = ["claude", "-p", message, "--output-format", "stream-json", "--verbose",
               "--setting-sources", settings, "--tools", ",".join(tools) if tools else ""]
        if self.model:
            cmd += ["--model", self.model]
        if system is not None:
            cmd += ["--system-prompt", system]
        if append and not resume:
            cmd += ["--append-system-prompt", append]
        if allowed:
            cmd += ["--allowedTools", *allowed]
        cmd += ["--strict-mcp-config"] + (["--mcp-config", mcp] if mcp else [])
        if resume:
            cmd += ["--resume", resume]
        # Streamed to a file as it runs, so a long app session can be followed.
        stream = os.path.join(self.dir, f"{who}.jsonl")
        start = os.path.getsize(stream) if os.path.exists(stream) else 0
        with open(stream, "a") as out, open(os.path.join(self.dir, f"{who}.err"), "a") as err:
            subprocess.run(cmd, cwd=cwd or self.home, env=env, stdin=subprocess.DEVNULL,
                           stdout=out, stderr=err, timeout=timeout)
        with open(stream) as f:
            f.seek(start)
            events = [json.loads(l) for l in f if l.startswith("{")]
        result = next((e for e in reversed(events) if e.get("type") == "result"), None)
        if not result:
            raise RuntimeError(f"{who}: claude gave no result; see {stream}")
        self.cost += result.get("total_cost_usd", 0)
        return result.get("result", ""), events, result["session_id"]

    def app_env(self):
        return dict(os.environ, **self.env, DISPLAY=f":{self.display}",
                    APP_BIN=os.path.join(self.build, "timegrapher-app"),
                    APP_HOME=self.home, SHOTS=self.shots)

    # ------------------------------------------------------------ surfaces

    def run(self):
        started = time.time()
        log(self.id, f"preparing ({self.p['surface']})")
        self.prepare()
        try:
            if self.p["surface"] == "app":
                notes, check, transcript = self.run_app(), None, None
            else:
                transcript, notes = self.run_pair()
                check = self.review(transcript)
        finally:
            self.cleanup()
        self.write_report(notes, check, transcript, time.time() - started)
        log(self.id, f"done in {(time.time() - started) / 60:.0f} min, ${self.cost:.2f}")

    def run_app(self):
        files = ", ".join(self.p.get("files", {})) or "none"
        positions = " ".join(self.p.get("stand", {})) or "none (no stand)"
        # A deep-pass persona sets budget, min_actions and notes in its front matter.
        budget = self.p.get("budget", 80)
        min_actions = self.p.get("min_actions", MIN_ACTIONS)
        report_format = prompt("report-format").replace(
            "Ten notes at most", f"{self.p.get('notes', 10)} notes at most")
        system = (prompt("app-tester")
                  .replace("{app}", os.path.join(HERE, "app.sh"))
                  .replace("{stand}", os.path.join(HERE, "stand.sh"))
                  .replace("{positions}", positions).replace("{files}", files)
                  .replace("{home}", self.home).replace("{budget}", str(budget))
                  .replace("{report_format}", report_format)
                  + self.p["dossier"])
        # The tester's commands see the persona's screen and stand.
        kw = dict(tools=["Bash", "Read"],
                  allowed=[f"Bash({os.path.join(HERE, 'app.sh')}:*)", f"Bash({os.path.join(HERE, 'stand.sh')}:*)",
                           "Read"],
                  extra_env={k: v for k, v in self.app_env().items() if k in
                             ("DISPLAY", "APP_BIN", "APP_HOME", "SHOTS")},
                  timeout=3 * 3600)
        notes, events, sess = self.claude("Begin. Open the app and do what you came to do.", system, "tester", **kw)
        actions = count_actions(events)
        # Agents tend to stop long before a person would; send them back in.
        for _ in range(3 + min_actions // 80):
            if actions >= min_actions:
                break
            log(self.id, f"stopped after {actions} actions; sending back")
            notes, events, sess = self.claude(
                f"You stopped after {actions} actions; a person spends far longer with a new app. "
                "Open the app again if it is closed and keep going: everything in your dossier you did not do "
                "yet, and anything your notes say you skipped. Then write your complete notes again, "
                "including the earlier ones that still stand.", system, "tester", resume=sess, **kw)
            actions += count_actions(events)
        return notes

    def run_pair(self):
        surface = self.p["surface"]
        today = datetime.date.today().isoformat()
        if surface == "mcp-pair":
            mcp = os.path.join(self.dir, "mcp.json")
            with open(mcp, "w") as f:
                server = [os.path.join(self.build, "timegrapher-mcp")]
                if self.shim:
                    server.insert(0, os.path.join(HERE, "mcp-shim.py"))
                json.dump({"mcpServers": {"timegrapher": {
                    "command": server[0], "args": server[1:],
                    "env": {"TIMEGRAPHER_CLI": os.path.join(self.build, "timegrapher"), **self.env}}}}, f)
            helper = dict(system=prompt("helper-mcp").format(home=self.home, date=today),
                          mcp=mcp, allowed=["mcp__timegrapher"])
        else:
            skills = os.path.join(self.home, ".claude", "skills")
            os.makedirs(skills, exist_ok=True)
            shutil.copytree(os.path.join(self.work, "skill", "skills", "watchmaker"),
                            os.path.join(skills, "watchmaker"), dirs_exist_ok=True)
            tools = ["Bash", "Read", "Write", "Edit", "Glob", "Grep", "Skill"]
            helper = dict(system=None, tools=tools, allowed=tools, settings="user",
                          extra_env={"PATH": self.build + os.pathsep + os.environ["PATH"]})
            helper["append"] = prompt("helper-cli").format(home=self.home, date=today)
        collector_system = prompt("collector") + self.p["dossier"]
        name = self.p["name"]
        lines = [f"# Conversation: {name} and the helper ({surface})", ""]
        c_sess = h_sess = None
        msg = "Start the conversation: write your first message to the helper."
        for i in range(MAX_MESSAGES):
            said, _, c_sess = self.claude(msg, collector_system, "collector", resume=c_sess, timeout=900)
            done = "[DONE]" in said
            for d in re.findall(r"\[(PLACE \w+|REMOVE)\]", said):
                outcome = self.stand(*d.lower().split())
                lines += [f"*({name}: {d.title()}. {outcome})*", ""]
            lines += [f"**{name}:** {said.replace('[DONE]', '').strip()}", ""]
            log(self.id, f"{name} message {i + 1}")
            # A last message that still asks something gets its answer first.
            if done and "?" not in said:
                break
            h_msg = re.sub(r"\[(PLACE \w+|REMOVE|DONE)\]", "", said).strip()
            reply, events, h_sess = self.claude(
                h_msg, helper.get("system"), "helper", resume=h_sess, tools=helper.get("tools", ()),
                allowed=helper["allowed"], mcp=helper.get("mcp"), settings=helper.get("settings", ""),
                extra_env=helper.get("extra_env"), append=helper.get("append"), timeout=3600)
            lines += tool_lines(events) + [f"**Helper:** {reply.strip()}", ""]
            if done:
                break
            msg = reply
        transcript = "\n".join(lines)
        with open(os.path.join(self.out, f"{self.id}-conversation.md"), "w") as f:
            f.write(transcript)
        notes, _, _ = self.claude(prompt("debrief") + "\n\n" + prompt("report-format"),
                                  collector_system, "collector", resume=c_sess, timeout=900)
        return transcript, notes

    def review(self, transcript):
        surface = {"mcp-pair": "the MCP server in a chat app",
                   "cli-pair": "the command line and the watchmaker skill"}[self.p["surface"]]
        system = prompt("reviewer").replace("{surface}", surface).replace("{report_format}", prompt("report-format"))
        log_path = os.path.join(self.stand_dir, "log.txt")
        stand_log = open(log_path).read() if os.path.exists(log_path) else "(no stand used)\n"
        message = (f"# The person's dossier\n\n{self.p['dossier']}\n\n# Truth sheet\n\n{self.p['truth']}\n\n"
                   f"# Stand log (UTC)\n\n```\n{stand_log}```\n\n{transcript}")
        check, _, _ = self.claude(message, system, "reviewer", timeout=900)
        return check

    def write_report(self, notes, check, transcript, seconds):
        b = json.load(open(os.path.join(self.work, "build.json")))
        surface = {"app": "the desktop app", "mcp-pair": "the MCP server, with an AI helper",
                   "cli-pair": "the command line and watchmaker skill, with an AI helper"}[self.p["surface"]]
        head = [f"# {self.p['name']}: {self.p['dossier'].splitlines()[0].lstrip('# ').split(', ', 1)[-1]}", "",
                f"Build {b['version']} ({b['build']}). Surface: {surface}. "
                f"{seconds / 60:.0f} min, ${self.cost:.2f}."
                + (" The MCP server ran behind mcp-shim.py (protocol 2026-07-28 hidden)."
                   if self.shim and self.p["surface"] == "mcp-pair" else ""), ""]
        if transcript:
            head += [f"The whole conversation: [{self.id}-conversation.md]({self.id}-conversation.md).", ""]
        body = ["## Notes", "", notes.strip(), ""]
        if check:
            body += ["## Watchmaker's check", "", check.strip(), ""]
        with open(os.path.join(self.out, f"{self.id}.md"), "w") as f:
            f.write("\n".join(head + body))


def count_actions(events):
    """Commands run through app.sh and stand.sh, however they were batched."""
    n = 0
    for e in events:
        message = e.get("message")
        content = message.get("content") if isinstance(message, dict) else None
        if e.get("type") == "assistant" and isinstance(content, list):
            for c in content:
                if c.get("type") == "tool_use" and c.get("name") == "Bash":
                    cmd = c.get("input", {}).get("command", "")
                    n += len(re.findall(r"(?:app|stand)\.sh\s+\w+", cmd))
    return n


def tool_lines(events):
    """The helper's tool calls and what came back, shortened, for the transcript."""
    calls, out = {}, []
    for e in events:
        message = e.get("message")
        content = message.get("content") if isinstance(message, dict) else None
        if not isinstance(content, list):
            continue
        for c in content:
            if c.get("type") == "tool_use":
                calls[c["id"]] = c
            elif c.get("type") == "tool_result" and c.get("tool_use_id") in calls:
                use = calls[c["tool_use_id"]]
                res = c.get("content")
                if isinstance(res, list):
                    res = "\n".join(x.get("text", "") for x in res if isinstance(x, dict))
                res = str(res or "")
                short = res if len(res) < 1200 else res[:1200] + f"\n… ({len(res)} characters)"
                args = json.dumps(use.get("input", {}), ensure_ascii=False)
                out += [f"<details><summary>tool: {use['name']} {args[:200]}</summary>", "", "```",
                        short, "```", "</details>", ""]
    return out


def run(args):
    work = os.path.abspath(args.work)
    out = os.path.abspath(args.out)
    os.makedirs(out, exist_ok=True)
    chosen = [p for p in personas() if not args.only or p["id"] in args.only]
    if args.notes:
        # What changed since the last round: every persona has read it, and the triage checks it was covered.
        with open(args.notes) as f:
            notes = f.read().strip()
        with open(os.path.join(out, "notes.md"), "w") as f:
            f.write(notes + "\n")
        for p in chosen:
            p["dossier"] += ("\n\n## What is new in this version\n\nThe person read these release notes "
                             "before starting and will try what is new where it fits what they came to do:\n\n"
                             + notes)
    with concurrent.futures.ThreadPoolExecutor(len(chosen)) as pool:
        jobs = {pool.submit(Persona(p, work, out, args.model, 90 + i, args.mcp_shim).run): p["id"]
                for i, p in enumerate(chosen)}
        for j in concurrent.futures.as_completed(jobs):
            if j.exception():
                log(jobs[j], f"FAILED: {j.exception()}")
    if not args.no_triage:
        triage(out, work, args.model)


def triage(out, work, model):
    log("triage", "writing triage.md")
    env = clean_env(HOME=os.path.join(work, "triage-home"))
    os.makedirs(env["HOME"], exist_ok=True)
    message = prompt("triage").replace("{round_dir}", out)
    if os.path.exists(os.path.join(out, "notes.md")):
        message += prompt("triage-notes").replace("{round_dir}", out)
    cmd = ["claude", "-p", message, "--output-format", "json",
           "--setting-sources", "", "--strict-mcp-config", "--tools", "Read,Glob,Write",
           "--allowedTools", "Read", "Glob", f"Write({out}/**)", "--add-dir", out]
    if model:
        cmd += ["--model", model]
    r = subprocess.run(cmd, cwd=out, env=env, text=True, capture_output=True, timeout=1800,
                       stdin=subprocess.DEVNULL)
    if not os.path.exists(os.path.join(out, "triage.md")):
        log("triage", f"no triage.md written: {r.stdout[-500:]} {r.stderr[-500:]}")


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    s = sub.add_parser("setup")
    s.add_argument("--build", required=True, help="a release tag (drafts too), e.g. v0.1.0, or a commit")
    s.add_argument("--work", default=os.path.expanduser("~/persona-round"))
    r = sub.add_parser("run")
    r.add_argument("--out", required=True, help="where the reports go, e.g. /mnt/project-files/testing/2026-10-11")
    r.add_argument("--only", nargs="*", help="persona ids (default: all)")
    r.add_argument("--work", default=os.path.expanduser("~/persona-round"))
    r.add_argument("--model", help="model for every agent (default: claude's default)")
    r.add_argument("--no-triage", action="store_true")
    r.add_argument("--notes", help="a markdown file of what changed since the last round (release notes)")
    r.add_argument("--no-mcp-shim", dest="mcp_shim", action="store_false",
                   help="talk to timegrapher-mcp directly (see mcp-shim.py for why the shim exists)")
    t = sub.add_parser("triage")
    t.add_argument("--out", required=True)
    t.add_argument("--work", default=os.path.expanduser("~/persona-round"))
    t.add_argument("--model")
    a = ap.parse_args()
    if a.cmd == "setup":
        setup(a)
    elif a.cmd == "run":
        run(a)
    else:
        triage(os.path.abspath(a.out), a.work, a.model)


if __name__ == "__main__":
    main()
