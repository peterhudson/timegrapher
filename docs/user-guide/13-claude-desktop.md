# 13. Claude Desktop

The Timegrapher extension gives Claude access to the measuring engine on
your computer. You can ask it to analyse a saved recording, compare
positions, or guide a test at the bench. It runs a measurement tool and
receives the result, then explains that result in the conversation.

This can be useful when several observations need to be considered
together. For example, a periodic change in amplitude may deserve a look
at the beat profiles, the rate spectrum and a second recording before
suggesting a mechanical cause. The assistant can help organise that
investigation. Its explanation is still an interpretation: ask it to
separate measured facts, assumptions and possible causes.

## Installing the Extension

Use Claude Desktop on macOS or Windows and the `.mcpb` extension for your
system from the Timegrapher download: **Claude Desktop/Timegrapher.mcpb**.
On Windows, extract the whole zip first. The bundle includes both the
local server and the command-line program. You do not need to build them.
The Linux download supplies the programs in **Command Line**, but has no
Claude Desktop extension bundle.

In Claude Desktop, open **Settings → Extensions → Advanced settings →
Install Extension…**, select the `.mcpb`, and follow the installation
prompts. The extension is unsigned; review its origin and requested
access. If the tools do not appear, quit and reopen Claude Desktop.
Organisation policies may restrict local extensions. Anthropic maintains
the [current installation instructions](https://support.claude.com/en/articles/10949351-getting-started-with-local-mcp-servers-on-claude-desktop).

The Timegrapher tools should then be available in the chat's tools menu.
The **Test My Watch** prompt starts a guided workflow using the project's
[watchmaker skill](../../skills/watchmaker/SKILL.md). The skill asks the
assistant to establish the watch and measurement conditions, use published
references, and cite the sources behind its interpretation.

Tool approval behaviour depends on Claude Desktop's settings and the
permissions you grant. Check those settings before a session; do not
assume that every individual operation will always produce a new prompt.

## A Useful First Conversation

Start with a saved recording whose conditions you know. Give its **full
local path**, the position, calibre, lift angle and winding state. For
example:

> Analyse /Users/you/Recordings/watch_DU.flac. It is a Swiss lever watch
> at 28,800 bph, lift angle 55°, dial up, recorded ten minutes after full
> winding. Tell me the measurement settings and clock basis. Then explain
> what is measured reliably and what needs closer inspection.

For a longer recording:

> Check steadiness over this recording. Compare the readings over time,
> autocorrelation and period search. If you find a candidate cycle, tell
> me its period, size, how many cycles were observed, and whether it
> persists through the take. Treat wheel matches as hypotheses.

For a live positions test, specify the recording and settling durations.
The guided workflow should explain each orientation and wait for you to
position the watch before recording it. State unusual crown arrangements
explicitly; **crown left** is not enough to establish a Swiss hour position
for every case design.

## The Available Tools

| Tool | Use |
| --- | --- |
| `analyze_recording` | Measure a saved recording's rate, amplitude and beat error |
| `tick_tock_profile` | Inspect the averaged Tick and Tock envelopes and measured markers |
| `beat_shape` | Examine the timing and shape of the acoustic groups |
| `check_steadiness` | Analyse variation, correlation and periodic behaviour in a long recording |
| `long_run` | Follow a long recording and search for periodic components; write an HTML report |
| `check_recording` | Check a saved recording's level, clipping and other acquisition conditions |
| `check_microphone` | Capture a short microphone test and report the input conditions |
| `record_position` | Record one position into a test folder |
| `run_session` | Analyse a folder of position takes and write a report |
| `list_sound_inputs` | Inspect available sound inputs |
| `list_calibres` | Look up movement settings, train information and sources |
| `make_report_bundle` | Save a diagnostic bundle locally; specify a destination or it uses the home folder |
| `read_guide` | Read the watchmaker skill and its reference pages |

Several tools also display charts in the conversation. These can make a
pattern easier to discuss, but the chart and the assistant's description
should agree with the returned measurements.

The tools call the same command-line engine used elsewhere in Timegrapher.
Agreement with the desktop app requires matching the **recording interval,
beat rate, lift angle, averaging and clock correction**. In particular,
`check_steadiness` and `long_run` use a selected calibre for wheel naming;
that does not automatically apply its lift angle. Supply the correct
numeric settings, and have the assistant report what it actually used.
Custom calibres saved in the app are available to these local tools too.

## What Happens to the Recording

The measuring programs run locally. Their ordinary analysis tools read
the audio on your computer and return measurements and chart data to the
chat; they do not upload the audio as part of analysis. Those returned
results become part of your conversation with the chat service.

`check_microphone` and `record_position` capture audio. File-analysis tools
work on existing recordings; input discovery may query or open devices
to inspect their capabilities. The microphone check reports sound
settings rather than changing them.

A diagnostic bundle can contain audio. Making one saves a local file;
submitting it to GitHub, attaching a recording to a chat, or sharing a
report is a separate action. Review and listen to anything you choose to
share. See [Problems and Reports](14-problems-and-reports.md).

## Other Assistants and Updates

A client that supports local MCP servers over standard input/output can
run `timegrapher-mcp` from the download's **Command Line** folder. Keep
the matching `timegrapher` executable beside it
and use the server's full path as the command. The [MCP documentation](../mcp.md)
gives configuration details. Assistants that can run shell commands can
use the [command line](12-command-line.md) and watchmaker skill directly.

The extension contains its own binaries. Updating the desktop Timegrapher
app does not by itself update an already installed extension. Install the
new extension bundle when updating that route, and compare reported
versions if two interfaces disagree.

---

Previous: [The Command Line](12-command-line.md) ·
Next: [Problems and Reports](14-problems-and-reports.md)
