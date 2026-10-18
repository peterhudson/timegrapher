#!/usr/bin/env python3
"""Make the user guide's screenshots from their descriptions.

    python3 guide-figures/make.py [--corpus DIR] [--clips] [--keep] [FIGURE ...]

Each figure is a TOML file in guide-figures/figures/: the recording it
opens, the window's size, the app's settings and the panes and where they
sit. For each one this script starts a virtual 1920x1080 screen with a
white background, writes the settings into a fresh home folder, opens the
app on the recording, waits until the picture stops changing, and saves
the window with a soft shadow as docs/user-guide/images/<output>.

Most figures need only a few seconds of a corpus take: their [data] names
the take, the file, the start and the seconds, and the clip is kept in
guide-figures/data/<figure>/, so making the figures needs no download.
--clips cuts the clips again from the corpus. A figure that needs a whole
take (no `seconds`) fetches it.

With no FIGURE, every figure is made. --corpus names a checkout of
peterhudson/timegrapher-corpus (default: guide-figures/.cache/corpus, cloned
at CORPUS_REVISION); the audio is fetched with its scripts/fetch.py.
--keep leaves each figure's raw screen and home folder in .cache/ to look at.

Needs: a built timegrapher-app (target/release, or APP_BIN), Xvfb, openbox,
xdotool, ImageMagick and pngquant; ffmpeg for --clips. Linux only. Standard library only.
"""
import hashlib
import json
import os
import shlex
import shutil
import signal
import subprocess
import sys
import time
import tomllib

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
CACHE = os.path.join(HERE, ".cache")
DATA = os.path.join(HERE, "data")
# The home folder the app sees, so the path in the Watch pane reads like a
# reader's own; a scratch one under .cache when /home can't take it.
GUIDE_HOME = "/home/watchmaker"
MARK = ".guide-figures"
IMAGES = os.path.join(ROOT, "docs", "user-guide", "images")
SCREEN = (1920, 1080)
DISPLAY = ":42"

# Every pane the app has (app.rs, `Pane::ALL`). A layout must hold each
# exactly once, or the app sets it aside for its starting one, so the
# panes a figure leaves out are kept in a hidden tab bar.
PANES = [
    "Watch", "PositionsTest", "Strip", "Sound", "Scope", "Isochronism",
    "Rate", "Amplitude", "BeatError", "RateHistogram", "AmplitudeHistogram",
    "BeatErrorHistogram", "SteadyReadings", "SteadyPeriods", "SteadyCycle",
    "SteadyAutocorrelation", "SteadyAllan", "SteadyCusum", "SteadyFft",
]
# egui's id for the tree named "panes", as the app saves it.
TREE_ID = 8333874272848016587

# The app's settings for every figure, before the figure's own: light,
# no update check, the strip across.
BASE_SETTINGS = {
    "theme": "Light",
    "check_updates": False,
    "strip": {"half_width_ms": 10.0, "span_s": 30.0, "horizontal": True},
}


# The corpus commit the figures are made from. It runs ahead of the one
# CI pins when a figure needs a take regress doesn't read.
CORPUS_REVISION = "39b0d45e8999121e166ff69ccaa60d28d56de72d"


def ensure_corpus(path):
    if not os.path.isdir(os.path.join(path, "takes")):
        subprocess.run(["git", "clone", "-q", "https://github.com/peterhudson/timegrapher-corpus", path], check=True)
    head = subprocess.run(["git", "-C", path, "rev-parse", "HEAD"], capture_output=True, text=True).stdout.strip()
    if head != CORPUS_REVISION:
        subprocess.run(["git", "-C", path, "fetch", "-q", "origin"], check=True)
        subprocess.run(["git", "-C", path, "checkout", "-q", CORPUS_REVISION], check=True)
    return path


def fetch_take(corpus, take, file=None):
    dest = os.path.join(CACHE, "audio")
    if file and os.path.exists(os.path.join(dest, take, file)):
        return os.path.join(dest, take)
    subprocess.run([sys.executable, "-I", os.path.join(corpus, "scripts", "fetch.py"), dest, take], check=True)
    return os.path.join(dest, take)


# A clip larger than this stays in .cache/clips and is cut again from the
# corpus on a fresh checkout, so the repository holds no large audio.
KEEP_BYTES = 5_000_000


def cached_clip(data):
    """Where a large clip is kept: named by its [data], so figures that
    show the same stretch of a take share one."""
    key = hashlib.sha1(json.dumps(data, sort_keys=True).encode()).hexdigest()[:12]
    return os.path.join(CACHE, "clips", key)


def clip_name(data):
    """The clip's file name: the take's file it starts in."""
    return data.get("files", [data.get("file")])[0]


def clip(name, data, corpus):
    """Cut the figure's seconds from its corpus take into data/<figure>/ (or
    .cache/clips/ when large), with a session.toml carrying the
    take's watch, so the app shows it. [data.session] adds to or replaces
    the take's own (a take with no session.toml needs one)."""
    sources = data.get("files", [data.get("file")])
    take = fetch_take(corpus(), data["corpus"], sources[-1])
    out = cached_clip(data)
    shutil.rmtree(out, ignore_errors=True)
    os.makedirs(out)
    start, secs = data.get("start", 0), data["seconds"]
    # Several files of a take are one run: joined, then cut.
    listing = os.path.join(out, "files.txt")
    with open(listing, "w") as f:
        f.writelines(f"file '{os.path.join(take, s)}'\n" for s in sources)
    subprocess.run(["ffmpeg", "-v", "error", "-f", "concat", "-safe", "0", "-i", listing,
                    "-ss", str(start), "-t", str(secs), "-map_metadata", "-1",
                    "-c:a", "flac", "-compression_level", "12", os.path.join(out, clip_name(data))], check=True)
    os.remove(listing)
    with open(os.path.join(out, "source.json"), "w") as f:
        json.dump(data, f, sort_keys=True, indent=1)
        f.write("\n")
    session = {}
    if os.path.exists(os.path.join(take, "session.toml")):
        with open(os.path.join(take, "session.toml"), "rb") as f:
            session = tomllib.load(f)
    session.update(data.get("session", {}))
    position = session.get("position") or next(
        (r["position"] for r in session.get("recording", []) if r.get("file") == clip_name(data)),
        session.get("recording", [{}])[0].get("position", "DU"))
    lines = [f"{k} = {json.dumps(session[k], ensure_ascii=False)}" for k in ("watch", "calibre", "bph", "lift") if k in session]
    lines.append(json.dumps(f"{secs} s of {' + '.join(sources)} from {start} s, take {data['corpus']} "
                            f"in peterhudson/timegrapher-corpus (CC0).", ensure_ascii=False).join(["notes = ", ""]))
    lines += ["", "[[recording]]", f"file = {json.dumps(clip_name(data))}", f"position = {json.dumps(position)}"]
    with open(os.path.join(out, "session.toml"), "w") as f:
        f.write("\n".join(lines) + "\n")
    if os.path.getsize(os.path.join(out, clip_name(data))) <= KEEP_BYTES:
        kept = os.path.join(DATA, name)
        shutil.rmtree(kept, ignore_errors=True)
        shutil.move(out, kept)
        return kept
    return out


def recording(name, data, corpus, clips):
    """The folder (or file) the figure opens, before it is put in the home folder."""
    if "corpus" not in data:
        target = os.path.expanduser(data["path"])
        if not os.path.exists(target):
            raise SystemExit(f"{name}: {target} is not here; {data.get('source', 'see the figure file')}")
        return target
    if "seconds" in data:
        # A clip is kept while the figure's [data] still describes it.
        for here in (os.path.join(DATA, name), cached_clip(data)):
            try:
                with open(os.path.join(here, "source.json")) as f:
                    same = json.load(f) == json.loads(json.dumps(data))
            except OSError:
                same = False
            if not clips and same:
                return here
        return clip(name, data, corpus)
    return fetch_take(corpus(), data["corpus"])


def home_folder(work):
    try:
        if not os.path.exists(GUIDE_HOME):
            os.makedirs(GUIDE_HOME)
            open(os.path.join(GUIDE_HOME, MARK), "w").close()
        if os.path.exists(os.path.join(GUIDE_HOME, MARK)):
            for entry in os.listdir(GUIDE_HOME):
                if entry != MARK:
                    path = os.path.join(GUIDE_HOME, entry)
                    shutil.rmtree(path) if os.path.isdir(path) and not os.path.islink(path) else os.remove(path)
            return GUIDE_HOME
    except OSError:
        pass
    return os.path.join(work, "home")


class Layout:
    """An egui_tiles tree, as the app saves it, from a nested description:

        ["row" | "column", [share, child], ...]   panes side by side or stacked
        ["tabs", "Pane", ...]                     panes as tabs, the first shown
        "Pane"                                    one pane in its own tab bar
    """

    def __init__(self):
        self.tiles = {}
        self.next = 1

    def add(self, tile):
        tid = self.next
        self.next += 1
        self.tiles[str(tid)] = tile
        return tid

    def pane(self, name):
        if name not in PANES:
            raise SystemExit(f"unknown pane {name!r}; the panes are {', '.join(PANES)}")
        return self.add({"Pane": name})

    def tabs(self, names):
        kids = [self.pane(n) for n in names]
        return self.add({"Container": {"Tabs": {"children": kids, "active": kids[0]}}})

    def build(self, node):
        if isinstance(node, str):
            return self.tabs([node])
        kind, rest = node[0], node[1:]
        if kind == "tabs":
            return self.tabs(rest)
        if kind not in ("row", "column"):
            raise SystemExit(f"a layout node starts with row, column or tabs, not {kind!r}")
        kids, shares = [], {}
        for share, child in rest:
            kid = self.build(child)
            kids.append(kid)
            shares[str(kid)] = float(share)
        direction = "Horizontal" if kind == "row" else "Vertical"
        return self.add({"Container": {"Linear": {"children": kids, "dir": direction, "shares": {"shares": shares}}}})


def tree(spec):
    lay = Layout()
    shown = lay.build(spec)
    used = {t["Pane"] for t in lay.tiles.values() if "Pane" in t}
    if len(used) != sum("Pane" in t for t in lay.tiles.values()):
        raise SystemExit("a pane appears twice in the layout")
    hidden = [lay.pane(p) for p in PANES if p not in used]
    invisible = []
    if hidden:
        bar = lay.add({"Container": {"Tabs": {"children": hidden, "active": hidden[0]}}})
        invisible = hidden + [bar]
        root = lay.add({"Container": {"Linear": {"children": [shown, bar], "dir": "Horizontal", "shares": {"shares": {}}}}})
    else:
        root = shown
    return {
        "id": TREE_ID,
        "root": root,
        "tiles": {"next_tile_id": lay.next, "tiles": lay.tiles, "invisible": invisible},
        "height": None,
        "width": None,
    }


def merge(base, over):
    out = dict(base)
    for k, v in over.items():
        out[k] = merge(out[k], v) if isinstance(v, dict) and isinstance(out.get(k), dict) else v
    return out


def app_ron(fig, size):
    settings = merge(BASE_SETTINGS, fig.get("settings", {}))
    settings["panes"] = tree(fig["layout"])
    x, y = (SCREEN[0] - size[0]) // 2, (SCREEN[1] - size[1]) // 2
    window = (f"(inner_position_pixels:Some((x:{x}.0,y:{y}.0)),outer_position_pixels:Some((x:{x}.0,y:{y}.0)),"
              f"fullscreen:false,maximized:false,inner_size_points:Some((x:{size[0]}.0,y:{size[1]}.0)))")
    return "{\n" + ",\n".join(
        f"    {json.dumps(k)}: {json.dumps(v)}"
        for k, v in [("timegrapher-settings", json.dumps(settings)), ("window", window)]
    ) + ",\n}\n"


def openbox_config(home):
    """A light title bar in the style of a current desktop, on Breeze's buttons."""
    themes = os.path.join(home, ".local", "share", "themes")
    theme = os.path.join(themes, "GuideLight", "openbox-3")
    shutil.copytree("/usr/share/themes/Breeze-ob/openbox-3", theme, dirs_exist_ok=True)
    with open(os.path.join(HERE, "openbox-themerc")) as f:
        rc = f.read()
    with open(os.path.join(theme, "themerc"), "w") as f:
        f.write(rc)
    conf = os.path.join(home, ".config", "openbox")
    os.makedirs(conf, exist_ok=True)
    shutil.copy(os.path.join(HERE, "openbox-rc.xml"), os.path.join(conf, "rc.xml"))


def shot(path):
    subprocess.run(["import", "-display", DISPLAY, "-window", "root", path], check=True)


def same(a, b):
    r = subprocess.run(["compare", "-metric", "AE", a, b, "null:"], capture_output=True, text=True)
    try:
        return float(r.stderr.split()[0]) == 0
    except (ValueError, IndexError):
        return False


def finish(raw, out, fig):
    """The window cut from the white screen, on white with a soft shadow."""
    pad = 40
    subprocess.run([
        "convert", raw, "-trim", "+repage",
        "(", "+clone", "-background", "#000000", "-shadow", "28x14+0+8", ")",
        "+swap", "-background", "white", "-layers", "merge", "+repage",
        "-bordercolor", "white", "-border", str(pad // 2),
        "-resize", f"{fig.get('scale', 100)}%",
        out,
    ], check=True)
    subprocess.run(["pngquant", "--force", "--ext", ".png", "--quality", "70-95", out], check=True)


def make(name, fig, corpus, clips, keep):
    work = os.path.join(CACHE, "figures", name)
    shutil.rmtree(work, ignore_errors=True)
    os.makedirs(work)
    home = home_folder(work)
    os.makedirs(os.path.join(home, ".local", "share", "timegrapher"), exist_ok=True)
    size = tuple(fig.get("window", {}).get("size", [1600, 940]))
    with open(os.path.join(home, ".local", "share", "timegrapher", "app.ron"), "w") as f:
        f.write(app_ron(fig, size))
    openbox_config(home)

    # The recording sits in ~/Recordings/<folder>, as a reader's would.
    data = fig.get("data", {})
    if not data:
        return shoot(name, fig, work, home, [], data, keep)
    source = recording(name, data, corpus, clips)
    folder = data.get("folder", data.get("corpus", os.path.basename(source.rstrip("/"))))
    shelf = os.path.join(home, "Recordings")
    os.makedirs(shelf, exist_ok=True)
    target = os.path.join(shelf, folder)
    if os.path.isdir(source) and "session" in data and "seconds" not in data:
        # A whole take with no session file of its own: its audio, and
        # one written from [data.session].
        os.makedirs(target)
        for entry in sorted(os.listdir(source)):
            if entry.lower().endswith((".flac", ".wav")):
                os.symlink(os.path.join(source, entry), os.path.join(target, entry))
        session = data["session"]
        lines = [f"{k} = {json.dumps(session[k], ensure_ascii=False)}" for k in ("watch", "calibre", "bph", "lift") if k in session]
        lines += ["", "[[recording]]", 'file = "."', f"position = {json.dumps(session.get('position', 'DU'))}"]
        with open(os.path.join(target, "session.toml"), "w") as f:
            f.write("\n".join(lines) + "\n")
    elif os.path.isdir(source):
        os.symlink(source, target)
        if "file" in data or "files" in data:
            target = os.path.join(target, clip_name(data))
    else:
        os.symlink(source, os.path.join(shelf, os.path.basename(source)))
        target = os.path.join(shelf, os.path.basename(source))
    args = [target]
    if data.get("mode", "open") == "replay":
        args = ["--replay", target]
    if data.get("mode") == "microphone":
        # The app listens to a virtual stand that plays the recording.
        folder_of = target if os.path.isdir(target) else os.path.dirname(target)
        stand = {k.upper(): os.path.join(folder_of, v) for k, v in data.get("stand", {}).items()}
        if not stand:
            stand = {"DU": target}
        fig = dict(fig, _stand=stand)
        args = []
    return shoot(name, fig, work, home, args, data, keep)


def shoot(name, fig, work, home, args, data, keep):
    env = dict(os.environ, DISPLAY=DISPLAY, HOME=home,
               XDG_DATA_HOME=os.path.join(home, ".local", "share"),
               XDG_CONFIG_HOME=os.path.join(home, ".config"))
    env.pop("WAYLAND_DISPLAY", None)
    procs = []
    pulse = None
    if os.path.exists(f"/tmp/.X{DISPLAY[1:]}-lock"):
        raise SystemExit(f"{name}: display {DISPLAY} is in use, by another run of this script?")
    try:
        if "_stand" in fig:
            pulse = start_stand(work, env)
            env["PULSE_SERVER"] = f"unix:{pulse}/native"
            # On the stand from the start, unless an action puts it there.
            if not any(step[0] == "stand" for step in fig.get("actions", [])):
                first = next(iter(fig["_stand"]))
                env["_STAND_PLAYER"] = str(place(fig["_stand"][fig.get("on_stand", first)], env))
        procs.append(subprocess.Popen(["Xvfb", DISPLAY, "-screen", "0", f"{SCREEN[0]}x{SCREEN[1]}x24", "-wr", "-nolisten", "tcp"],
                                      stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL))
        time.sleep(1)
        procs.append(subprocess.Popen(["openbox"], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL))
        time.sleep(0.5)
        app = os.environ.get("APP_BIN", os.path.join(ROOT, "target", "release", "timegrapher-app"))
        log = open(os.path.join(work, "app.log"), "w")
        procs.append(subprocess.Popen([app, *args], env=env, cwd=home, stdout=log, stderr=log))
        # Keep the pointer off the window, so nothing shows a hover.
        subprocess.run(["xdotool", "mousemove", "0", str(SCREEN[1] - 1)], env=env)

        for step in fig.get("actions", []):
            if step[0] == "stand":
                stop_player(env)
                if step[1] != "off":
                    env["_STAND_PLAYER"] = str(place(fig["_stand"][step[1].upper()], env))
            else:
                act(step, env)

        wait = fig.get("wait", {})
        least, most = wait.get("at_least", 4), wait.get("at_most", 600)
        start, prev = time.time(), None
        raw = os.path.join(work, "screen.png")
        # A live screen never settles: take it after a set time instead.
        if "exact" in wait:
            time.sleep(wait["exact"])
            shot(raw)
        while "exact" not in wait:
            time.sleep(2)
            shot(raw)
            if prev and time.time() - start >= least and same(prev, raw):
                break
            if time.time() - start > most:
                if data.get("mode") == "replay":
                    break
                raise SystemExit(f"{name}: the picture still changes after {most} s")
            prev = raw + ".prev.png"
            shutil.copy(raw, prev)
    finally:
        stop_player(env)
        if pulse:
            subprocess.run(["pulseaudio", "--kill"], env=dict(os.environ, PULSE_RUNTIME_PATH=pulse),
                           capture_output=True)
        for p in reversed(procs):
            p.terminate()
        for p in procs:
            try:
                p.wait(5)
            except subprocess.TimeoutExpired:
                p.kill()

    out = os.path.join(IMAGES, fig["output"])
    finish(raw, out, fig)
    if not keep:
        shutil.rmtree(work, ignore_errors=True)
    print(f"{name}: {os.path.relpath(out, ROOT)}")


def start_stand(work, env):
    """A PulseAudio server of the figure's own, with a null sink "stand"
    whose monitor is the default input, "USB_Stand_Mic" (as the persona
    harness's stand): what plays into the sink, the app hears."""
    pulse = os.path.join(work, "pulse")
    os.makedirs(pulse, exist_ok=True)
    # A run killed part way leaves its server and its player behind; the
    # player would find the new server and play into it as a second watch.
    subprocess.run(["pulseaudio", "--kill"], env=dict(os.environ, PULSE_RUNTIME_PATH=pulse),
                   capture_output=True)
    try:
        with open(os.path.join(pulse, "players")) as f:
            for pid in f.read().split():
                try:
                    os.killpg(int(pid), 15)
                except OSError:
                    pass
        os.remove(os.path.join(pulse, "players"))
    except FileNotFoundError:
        pass
    script = os.path.join(pulse, "stand.pa")
    with open(script, "w") as f:
        f.write("load-module module-native-protocol-unix\n"
                "load-module module-null-sink sink_name=stand rate=48000 channels=1 "
                "sink_properties=device.description=Stand\n"
                "load-module module-remap-source master=stand.monitor source_name=standmic rate=48000 "
                "channels=1 source_properties=device.description=USB_Stand_Mic\n"
                "set-default-source standmic\n")
    subprocess.run(["pulseaudio", "-n", "-F", script, "-D", "--exit-idle-time=-1", "--disallow-exit"],
                   env=dict(os.environ, PULSE_RUNTIME_PATH=pulse, HOME=env["HOME"]), check=True,
                   capture_output=True)
    return pulse


def place(path, env):
    """Put the watch on the stand: play the recording into it, looping."""
    p = subprocess.Popen(["bash", "-c", f"while true; do paplay --device=stand {shlex.quote(path)} || sleep 1; done"],
                         env=env, start_new_session=True,
                         stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    with open(os.path.join(env["PULSE_SERVER"][len("unix:"):-len("/native")], "players"), "a") as f:
        f.write(f"{p.pid}\n")
    return p.pid


def stop_player(env):
    pid = env.pop("_STAND_PLAYER", None)
    if pid:
        try:
            os.killpg(int(pid), 15)
        except OSError:
            pass


def act(step, env):
    """One step before the picture: ["click", x, y] (in the window),
    ["key", "ctrl+o"], ["type", "text"], ["sleep", seconds], or (with a
    microphone figure) ["stand", "DD"] to turn the watch, ["stand", "off"]
    to take it off."""
    kind = step[0]
    if kind == "sleep":
        time.sleep(step[1])
    elif kind == "click":
        win = subprocess.run(["xdotool", "search", "--sync", "--onlyvisible", "--name", "^Timegrapher"],
                             env=env, capture_output=True, text=True).stdout.split()[0]
        # Relative to the app's own window, below its title bar.
        # Hover first, then press and release, as a hand would: egui acts
        # on what is under the pointer as of the frame before.
        subprocess.run(["xdotool", "mousemove", "--window", win, str(step[1]), str(step[2])], env=env)
        time.sleep(0.5)
        subprocess.run(["xdotool", "click", "1"], env=env)
        time.sleep(0.5)
        subprocess.run(["xdotool", "mousemove", "0", str(SCREEN[1] - 1)], env=env)
    elif kind == "key":
        subprocess.run(["xdotool", "key", step[1]], env=env)
    elif kind == "type":
        subprocess.run(["xdotool", "type", step[1]], env=env)
    else:
        raise SystemExit(f"unknown action {kind!r}")


def main(argv):
    args = argv[1:]
    keep = "--keep" in args
    args = [a for a in args if a != "--keep"]
    corpus = os.path.join(CACHE, "corpus")
    if "--corpus" in args:
        i = args.index("--corpus")
        corpus = args[i + 1]
        del args[i:i + 2]
    figdir = os.path.join(HERE, "figures")
    clips = "--clips" in args
    args = [a for a in args if a != "--clips"]
    names = args or sorted(f[:-5] for f in os.listdir(figdir) if f.endswith(".toml"))
    for name in names:
        with open(os.path.join(figdir, name + ".toml"), "rb") as f:
            make(name, tomllib.load(f), lambda: ensure_corpus(corpus), clips, keep)


if __name__ == "__main__":
    # Killed (by timeout, say): still close the screen, the app and the stand.
    signal.signal(signal.SIGTERM, lambda *_: sys.exit(1))
    sys.exit(main(sys.argv))
