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
prints. The two programs must be installed side by side.

## Install

Until release bundles exist, build both programs from source (Rust 1.88 or
later):

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

### Claude Desktop (Mac and Windows)

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
3. Quit Claude Desktop completely and open it again. The timegrapher's tools
   appear in the chat's tools menu.
4. Start a chat with the **Test My Watch** prompt from the same menu, or
   just ask, e.g. "Here is a recording of my watch, dial up:
   /Users/you/Recordings/watch_DU.flac. How is it?"

Claude Desktop asks before running a tool. A one-click install bundle
(`.mcpb`) will come with the release builds.

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
| `check_recording` | `doctor --file` | Whether a recording is fit to measure with |
| `list_sound_inputs` | `devices` | The computer's sound inputs |
| `make_report_bundle` | `report` | A zip for a GitHub issue, saved in your home folder unless told otherwise |

Recordings are given by their full path on your computer (`~` for your home
folder works). Record them with the timegrapher app, or any recorder, first.
For a multi-position test, put the position in each file name
(`watch_DU.flac`, `watch_DD.flac`, ...) or keep them in a folder with a
`session.toml` ([sessions.md](sessions.md)).

## Charts

In chat apps that support [MCP Apps](https://apps.extensions.modelcontextprotocol.io/api/)
(Claude on the web and desktop, ChatGPT, VS Code, Goose and others), three
tools show their results as charts right in the conversation, drawn the way
the timegrapher app draws them:

- `analyze_recording`: the rate, amplitude and beat error, those three over
  time, and the Tick and Tock profiles from ten seconds in the middle of the
  recording.
- `run_session`: each position's readings against the tolerance, the
  characteristic values, rate and amplitude by position with each
  position's tolerance behind its bar, and the findings.
- `tick_tock_profile`: the Tick and Tock profiles with the unlock and drop.

The charts use the app's colours for each reading (rate red, amplitude
purple, beat error yellow, Tick blue, Tock orange) and its scales: like the
app, a chart over time is scaled to all but the highest and lowest 2% of
its readings, so one glitch doesn't flatten it, and says how many are off
the scale. Nothing is drawn outside a chart's plot. Hover over a chart over time to read its
values. The charts follow the chat app's light or dark look. They are one page served by this program, so they load
nothing from the internet. Chat apps without MCP Apps show the same results
as text.

The server also gives the chat app the watchmaker skill and the reference
pages behind it as resources (`timegrapher://skill/...` and
`timegrapher://docs/...`), and instructions to read the skill before
guiding anyone.

## What it never does

- It never uploads anything. A report bundle is a file on your computer
  that you attach to an issue yourself, and only you can say that you
  listened to the excerpt or offer it to the public test corpus.
- It never changes your computer's sound settings.
- It does not record yet. Recording through the chat app, and a live check
  of the microphone, will come in a later version, and the chat app will
  ask you before each take.

## When it does not work

- **"could not run timegrapher"**: `timegrapher` is not beside
  `timegrapher-mcp` or on your PATH. Put the two together, or set
  `TIMEGRAPHER_CLI`.
- **"is not a full path"**: the chat app passed a bare file name. Give it
  the whole path.
- **No tools in the chat app**: check the path in the config file, and that
  you quit and reopened the app. Running `timegrapher-mcp --version` in a
  terminal shows whether the program starts at all.
