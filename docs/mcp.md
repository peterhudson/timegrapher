# The timegrapher in a chat app (MCP)

`timegrapher-mcp` lets an AI chat app that cannot run a command line, such
as Claude Desktop, use the timegrapher. The chat app can then read your
recordings, explain the numbers and guide you through testing a watch. It
speaks the [Model Context Protocol](https://modelcontextprotocol.io) over
stdio. Agents that do have a command line (Claude Code, Codex and the like)
should use the `timegrapher` program and the [watchmaker
skill](../skills/watchmaker/SKILL.md) directly instead; see
[agent-interface.md](agent-interface.md).

Every tool runs the `timegrapher` program with `--json` and returns its
document unchanged, so the chat app sees exactly what the command line
prints; some tools put a few plain lines before it (see
[below](#charts)). The two programs must be installed side by side.

## Install

### Claude Desktop (Mac and Windows)

Each [release](https://github.com/peterhudson/timegrapher/releases) has a
Claude Desktop extension for each system:
`timegrapher-VERSION-macos.mcpb` (Apple silicon and Intel) and
`timegrapher-VERSION-windows-x64.mcpb`. It holds both programs, so nothing
else needs installing.

1. Download the `.mcpb` for your computer.
2. Double-click it or drag it into Claude Desktop's window. Or open Claude
   Desktop's settings, choose **Extensions**, then **Advanced settings**,
   then **Install Extension...**, and pick the file.
3. Choose **Install**. The timegrapher's tools appear in the chat's tools
   menu.
4. Start a chat with the **Test My Watch** prompt from the same menu, or
   just ask, e.g. "Here is a recording of my watch, dial up:
   /Users/you/Recordings/watch_DU.flac. How is it?"

Claude Desktop asks before running a tool. The extension is not signed, so
Claude Desktop says so when installing it; that is expected.

### From source

To build both programs yourself (Rust 1.88 or later):

```sh
git clone https://github.com/peterhudson/timegrapher
cd timegrapher
cargo build --release -p timegrapher-cli -p timegrapher-mcp
```

Both programs are then in `target/release/`. Copy `timegrapher` and
`timegrapher-mcp` (on Windows `timegrapher.exe` and `timegrapher-mcp.exe`)
together into a folder of your choice. The server looks for `timegrapher`
beside itself first, then on your PATH. To point it elsewhere, set
`TIMEGRAPHER_CLI` to the program's full path.

To make a Claude Desktop extension from your own build, run
`crates/timegrapher-mcp/packaging/mcpb.sh target/release timegrapher.mcpb`.
Or add the server to Claude Desktop by hand:

1. Open Claude Desktop's settings, choose **Developer**, then **Edit Config**.
   This opens `claude_desktop_config.json`, which is
   `~/Library/Application Support/Claude/claude_desktop_config.json` on a Mac
   and `%APPDATA%\Claude\claude_desktop_config.json` on Windows.
2. Add the server under `mcpServers`, with the full path to where you put it:

   ```json
   {
     "mcpServers": {
       "timegrapher": {
         "command": "/Users/you/timegrapher/timegrapher-mcp"
       }
     }
   }
   ```

   On Windows, write the path with doubled backslashes, e.g.
   `"C:\\Users\\you\\timegrapher\\timegrapher-mcp.exe"`.
3. Quit Claude Desktop completely and open it again.

### Other chat apps and editors

Any app that runs local MCP servers over stdio works the same way: give it
the full path of `timegrapher-mcp` as the command, with no arguments. VS
Code, Goose and LM Studio are examples.

ChatGPT reaches only servers on the internet, or a local server through
OpenAI's Secure MCP Tunnel
([Connect from ChatGPT](https://developers.openai.com/apps-sdk/deploy/connect-chatgpt)).
That route has not been tried with this server yet.

## What the chat app can do

| Tool | Runs | What for |
|---|---|---|
| `analyze_recording` | `analyze` | Rate, amplitude and beat error of one recording in one position, and whether a watch was heard at all |
| `run_session` | `session` | A multi-position test (DU, DD, CU, CD, CL, CR), Witschi style, with findings; writes `report.html` |
| `check_steadiness` | `series` | Steadiness verdicts over a long recording |
| `long_run` | `long` | Rate and amplitude over hours and the wheel behind a periodic change; writes `report.html` |
| `tick_tock_profile` | `profile` | The Tick and Tock sounds with the marks the engine measured |
| `beat_shape` | `shape` | Unlock, impulse and drop of each side's beat |
| `record_position` | `record` | Records the watch in one position into a test folder and checks the take; **uses the microphone** |
| `check_microphone` | `doctor` | Listens a few seconds and says what to change in the sound settings; **uses the microphone**, changes nothing |
| `check_recording` | `doctor --file` | Whether a recording is fit to measure with |
| `list_calibres` | `calibres` | A calibre's beat rate, lift angle and train |
| `list_sound_inputs` | `devices` | The computer's sound inputs |
| `make_report_bundle` | `report` | A zip for a GitHub issue, saved in your home folder unless told otherwise |
| `read_guide` | (none) | The watchmaker skill and its pages, for the chat app to read |

For a multi-position test, the chat app records each position into one
folder with `record_position` and then reads the folder with `run_session`.
Before each take it tells you how to place the watch and waits for you; your
chat app also asks you to allow each tool call. A take in which no watch is
heard is not added to the test.

### Repeated and overlapping calls

Each tool tells the chat app whether it changes anything (`readOnlyHint`)
and whether calling it again with the same arguments is harmless
(`idempotentHint`). Two tools are not harmless to repeat, because each call
makes something new:

- `record_position` adds a take to the folder and to its `session.toml`.
- `make_report_bundle` writes a new, time-stamped zip.

All the others are. `run_session` and `long_run` write `report.html`, but
the same call writes the same file again.

A chat app that retries a recording, for example after a lost answer, can
pass the same `take_id` with the same folder and position. The take
already made then comes back, marked as such, instead of a second
recording. The server remembers takes only while it runs.

Only one tool uses the microphone at a time. While `record_position` or
`check_microphone` runs, another call to either one gets an error saying
so, and records nothing.

Recordings made elsewhere work too. They are given by their full path on
your computer (`~` for your home folder works). For a multi-position test,
put the position in each file name (`watch_DU.flac`, `watch_DD.flac`, ...)
or keep them in a folder with a `session.toml` ([sessions.md](sessions.md)).

## Charts

In chat apps that support [MCP Apps](https://apps.extensions.modelcontextprotocol.io/api/)
(Claude on the web and desktop, ChatGPT, VS Code, Goose and others), five
tools show their results as charts right in the conversation, drawn the way
the timegrapher app draws them:

- `analyze_recording`: the rate, amplitude and beat error, those three over
  time, and the Tick and Tock profiles from ten seconds in the middle of the
  recording.
- `run_session`: each position's readings against the tolerance, the
  characteristic values, rate and amplitude by position with each
  position's tolerance behind its bar, and the findings.
- `tick_tock_profile`: the Tick and Tock profiles with the unlock and drop.
- `record_position`: a **Live** view while it records, like the app's
  live screen. It shows the rate, amplitude, beat error and beats heard,
  the paper strip of the last 30 seconds (Tick blue, Tock orange), and the
  three readings over time, about once a second. Once the take has been
  checked, the same view shows what the check found. The view gets the
  readings with `recording_status`, a tool for the view alone that the
  model never sees. It reads a file the recording keeps and never opens
  the microphone. Chat apps without views, and any that ask for progress,
  get the latest reading in the progress messages instead, for example
  "Recording CD: 72 of 120 s · +4.1 s/day · 268° · 0.3 ms".
- `check_steadiness`: the app's Steadiness pane (each series' verdict
  over its Readings, Periods, Cycle, Autocorrelation, Allan Deviation or
  CUSUM view, picked with the buttons above it), the readings on
  probability paper as the app's Distribution panes draw them (one steady
  state falls on the dashed straight line, two states bend it), and the
  findings. The chart data rides beside the result, so the model reads the
  verdicts and not the thousands of numbers behind the plots.

The charts use the app's colours for each reading (rate red, amplitude
purple, beat error yellow, Tick blue, Tock orange) and its scales: like the
app, a chart over time is scaled to all but the highest and lowest 2% of
its readings, so one glitch doesn't flatten it, and says how many are off
the scale. Beat error is shown as a size, without a sign, because which
beat counts as the Tick can change between runs of the same recording.
Nothing is drawn outside a chart's plot. Hover over a chart over time to read its
values. The charts follow the chat app's light or dark look. They are one page served by this program, so they load
nothing from the internet. Chat apps without MCP Apps show the same results
as text.

The server also gives the chat app the watchmaker skill and the reference
pages behind it, and instructions to read the skill before guiding anyone.
The model reads them with the `read_guide` tool, because many chat apps
leave resources for you to attach yourself. The same pages are also
resources (`timegrapher://skill/...` and `timegrapher://docs/...`).

Some results begin with a few plain lines for the model, before the
document, about what the numbers alone do not say:

- `analyze_recording`: the rate, amplitude and beat error, each with its
  standard error (±) where the result gives one, then one plain fact for
  each of the result's `caveats` (for example the lift angle assumed, how
  many seconds of beats were heard, the share of beats clipped, or that
  the sound card's clock was not checked). These lines give facts, not
  grades or verdicts.
- `run_session`: each position's readings with their standard errors, the
  characteristic values each named by what it compares (so the largest
  amplitude difference between any two positions, `d_amplitude`, is not
  mistaken for vertical minus horizontal, `dvh_amplitude`), and what limits
  the readings. A sentence names the positions it is about, unless it is
  about every one.
- `record_position`: what was recorded, what the take's check measured,
  the lift angle and whether it was given, the calibre's or the default,
  how many of the expected beats were found and clipped, and whether the
  take was added to the test.
- `check_microphone`: its rate and beat error come from a few seconds of
  sound, so they are not the watch's readings.

The charts list the same facts under **What Limits These Readings**.

The **Test My Watch** prompt is the skill's own
(`prompt-test-my-watch.md`): the step-by-step test an experienced
watchmaker would run, from the calibre and the microphone through the six
positions to a plain answer, with the watch you name added at the end.

## What it never does

- It never uploads anything. A report bundle is a file on your computer
  that you attach to an issue yourself, and only you can say that you
  listened to the excerpt or offer it to the public test corpus.
- It never changes your computer's sound settings: `check_microphone` says
  what to change, and you change it.
- It uses the microphone only for `record_position` and `check_microphone`,
  and only for the seconds asked.

## When it does not work

- **"could not run timegrapher"**: `timegrapher` is not beside
  `timegrapher-mcp` or on your PATH. Put the two together, or set
  `TIMEGRAPHER_CLI`.
- **"is not a full path"**: the chat app passed a bare file name. Give it
  the whole path.
- **No tools in the chat app**: check the path in the config file, and that
  you quit and reopened the app. Running `timegrapher-mcp --version` in a
  terminal shows whether the program starts at all.
