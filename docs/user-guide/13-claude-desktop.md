# 13. Claude Desktop

The Claude Desktop extension lets Claude, Anthropic's AI assistant, use
Timegrapher for you in a chat. You can ask it, in your own words, to check
your microphone, record your watch in each position, read a recording, or
explain a figure, and it runs the measurements and talks you through them.

It is also where **interpretation** lives. The app and the command line
only measure. The extension carries the
[watchmaker skill](../../skills/watchmaker/SKILL.md), a guide for AI
assistants written for this project, which reads the figures against
published sources (such as Witschi's training documents) and cites each
source it uses. So if you want to know what the figures mean for your
watch, this is the place to ask, and you can check every source it gives.

## What You Need

- **Claude Desktop** on a Mac or a Windows PC, signed in to your Claude
  account.
- The extension, `Timegrapher.mcpb`, in the **Claude Desktop** folder of
  the macOS or Windows download (on Windows, extract the whole zip first).
  It holds both the server and the command line tool, so nothing else needs
  installing. The Linux download has no extension; see
  [Other Chat Apps](#other-chat-apps).

## Install

1. Double-click the `.mcpb` file, or drag it into Claude Desktop's window.
   Or open Claude Desktop's settings, choose **Extensions**, then
   **Advanced settings**, then **Install Extension…**, and pick the file.
2. Choose **Install**. The extension is not signed, so Claude Desktop says
   so while installing it; that is expected.
3. The Timegrapher tools appear in the chat's tools menu.

## Test a Watch in a Chat

The easiest start is the **Test My Watch** prompt, from the same tools
menu. It runs the skill's step-by-step test: it asks about the watch and
its calibre, checks the microphone, has you wind the watch, and then leads
you through the six positions, telling you how to place the watch each
time and waiting for you to say it's ready.

Or just ask, for example:

> Here is a recording of my watch, dial up:
> /Users/you/Recordings/watch_DU.flac. What do the figures say?

> Check my microphone before I start.

> Record my watch in all six positions and give me the results.

Claude Desktop asks before running each tool, so nothing runs without you
seeing it first. Give full paths to recordings and folders.

## What It Can Do

| Tool | What for |
| --- | --- |
| `check_microphone` | Listens a few seconds and states the level, clipping, automatic gain and noise; **uses the microphone**, changes nothing |
| `list_sound_inputs` | The computer's sound inputs |
| `record_position` | Records the watch in one position into a test folder and checks the take; **uses the microphone** |
| `run_session` | A multi-position test from a folder of takes, with the characteristic values; writes `report.html` |
| `analyze_recording` | Rate, amplitude and beat error of one recording in one position |
| `tick_tock_profile` | The Tick and Tock sounds with the marks the engine measured |
| `beat_shape` | Unlock, impulse and drop of each side's beat |
| `check_steadiness` | How the rate, amplitude and beat error move over a long recording, with the statistics |
| `long_run` | Rate and amplitude over hours, and the wheel behind a periodic change; writes `report.html` |
| `check_recording` | A recording's level, clipping, automatic gain and noise |
| `list_calibres` | A calibre's beat rate, lift angle and train |
| `make_report_bundle` | A zip for a GitHub issue, saved in your home folder unless you name another |
| `read_guide` | The watchmaker skill and its pages, for Claude to read |

Each tool runs the `timegrapher` command line and passes back exactly what
it prints, so the figures are the same as the app's and the command
line's for the same recording. In Claude Desktop, several tools also show
their results as charts in the conversation, drawn the way the app draws
them: the live readings and paper strip while `record_position` records,
the readings and Tick and Tock profiles of a recording, a session's
positions, and the Steadiness views.

A calibre you added in the app can be used by name here too.

## What It Never Does

- **It never uploads your recordings.** The tools run on your computer and
  pass back their measurements as text. Those measurements become part of
  your chat with Claude, like anything else you write there.
- **It never changes your computer's sound settings.** `check_microphone`
  states what it measured; any change is yours to make.
- **It uses the microphone only** for `record_position` and
  `check_microphone`, and only for the seconds asked.
- A report bundle is a file on your computer that you attach to an issue
  yourself.

## Other Chat Apps

Any chat app or editor that runs local MCP servers can use the server,
`timegrapher-mcp`, which is in the download's **Command Line** folder beside
the command line tool. Give the app its full path as the command, with no
arguments, and keep `timegrapher` beside it. [The MCP server](../mcp.md) has the details,
including adding it to Claude Desktop by hand and
[what to check when it does not work](../mcp.md#when-it-does-not-work).

Assistants that can run commands themselves, such as Claude Code, can use
the `timegrapher` command line directly with the watchmaker skill; see
[the agent interface](../agent-interface.md).

---

Previous: [The Command Line](12-command-line.md) ·
Next: [Problems and Reports](14-problems-and-reports.md)
