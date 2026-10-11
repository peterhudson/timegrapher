# Persona rounds

Seven watch people use a build of the timegrapher the way customers would, and
write notes the way a watchmaker gives feedback to a developer. CI checks the
engine against tg and the corpus, the unit tests and the packages; persona
rounds check what it is like to *use* the desktop app, the command line with
the watchmaker skill, and the MCP server.

| Persona | Who | Uses |
|---|---|---|
| [Sam](people/1-first-timer.md) | First-timer with a new automatic | MCP server in a chat app, live on the stand |
| [Robin](people/2-pre-sale-collector.md) | Collector checking a watch before selling it | Command line and skill, saved recordings |
| [Dale](people/3-tinkerer.md) | Tinkerer who regulates their own watches | Desktop app, live, six positions |
| [Lucía](people/4-spanish-speaker.md) | Spanish speaker doing a six-position test | MCP server, recording each position |
| [Pat](people/5-sceptic.md) | Sceptic who gives wrong details on purpose | Command line and skill |
| [Brunner](people/6-retired-watchmaker.md) | Retired Swiss lab watchmaker | Desktop app, live and a two-hour recording; also reviews the conversations |
| [Mika](people/7-designer-watchmaker.md) | Former Apple software designer who services Seikos on weekends | Desktop app, a deep pass on every feature: six positions live and two saved recordings, judged as a product designer and a watchmaker |

Until rounds get close to a public release, only the watchmaking and design
experts, Brunner and Mika, take part. The other five carry `parked = true` and
sit out unless a round names them with `--only`. While they are parked, the
command line and the MCP server get no persona round.

## How a round works

- **The build** is a release (drafts too) or a commit built from source.
- **The watches** are recordings from the public
  [corpus](https://github.com/peterhudson/timegrapher-corpus). Each persona
  has a dossier saying which recording stands in for their watch, and a
  [truth sheet](truth/) from the corpus's tg references that only the
  reviewer sees.
- **The microphone stand is virtual.** Each persona gets its own PulseAudio
  server with a null sink called `stand` whose monitor is the default input,
  `USB_Stand_Mic`. [`stand.sh place DU`](harness/stand.sh) plays the
  persona's dial-up recording into it in a loop, so the app, `doctor`,
  `record` and the MCP server hear that watch live. No real microphone is
  ever used.
- **The app** runs on the persona's own Xvfb screen; the persona drives it
  with [`app.sh`](harness/app.sh) (xdotool and screenshots) and looks at
  every screenshot.
- **Command line and MCP server** are tested in pairs. The collector is a
  `claude` agent with no tools that knows only its dossier and does the
  physical things (`[PLACE DU]`). The helper is a `claude` agent with only
  the MCP server, or with a shell, the `timegrapher` program and the
  watchmaker skill; it never sees the dossier. Afterwards the collector writes
  their notes and Brunner checks the conversation against the truth sheet.
- **The output** goes to one folder: a report per persona (`ID.md`), the
  screenshots (`ID/`), the conversations (`ID-conversation.md`) and
  `triage.md`, the findings grouped by the part of the project that owns
  them.

## Running one

From a cloud session with this repository checked out:

```sh
python3 personas/harness/round.py setup --build v0.1.0 --work ~/persona-round
python3 personas/harness/round.py run --out /mnt/project-files/testing/YYYY-MM-DD --work ~/persona-round
```

`setup` installs the system packages it needs, downloads the build's Linux
archive (`--build COMMIT` builds from source instead), the watchmaker skill
at that commit and the corpus index. `run` downloads only the recordings the
personas use (about 1 GB), runs the personas that are not parked in parallel, then writes
`triage.md`. `--only ID ...` runs some of them; `--model` picks the model for
every agent. `--notes FILE` gives every persona the build's release notes
(what changed since the last round), and the triage then lists who tried each
item. `--no-mcp-shim` talks to the MCP server directly; use it for builds
that carry the `resultType` fix (main from 2026-10-11).

## Adding or changing a persona

A persona is a file in `people/` (TOML between `+++` lines, then the dossier)
and a truth sheet in `truth/` named by its `id`. Keys: `id`, `name`,
`surface` (`app`, `mcp-pair` or `cli-pair`), `language`, `stand` (position to
corpus `take/file`), `files` (path in the persona's home to `take/file`, or
`take/*` for a whole take as a folder) and `on_stand` (a position the watch
starts in). An app persona can also set `budget` (most actions, default 80),
`min_actions` (sent back until it has done this many, default 40) and `notes`
(most numbered notes, default 10); Mika's deep pass uses 300, 150 and 25. A
dossier states no calibre fact beyond the corpus metadata and
the program's own calibre table.
