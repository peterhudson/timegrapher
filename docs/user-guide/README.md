# Timegrapher User Guide

This guide is for anyone who wants to measure a mechanical watch with
Timegrapher: a watchmaker at the bench, a collector checking a watch before
or after buying it, or someone curious about what their watch is doing. It
covers the three ways to use the software, all from the download:

- **The desktop app**, Timegrapher, which listens to a watch on a
  microphone and draws its beats live, the way a bench timegrapher does.
- **The command line tool**, `timegrapher`, which does the same measuring
  from a terminal, on recordings of any length, and writes reports.
- **The Claude Desktop extension**, which lets Claude (Anthropic's AI
  assistant) run the measurements for you and explain the figures in a
  chat.

All three use the same measuring engine, so the same recording gives the
same figures in each.

![The watch on a microphone feeds the desktop app, the command line and the Claude Desktop extension, which share one measuring engine](images/how-it-fits.svg)

## What Timegrapher Is, and Isn't

Timegrapher is a **measuring instrument**. It tells you what it measured
(rate, amplitude, beat error and much more) and the conditions it noticed
while measuring, such as clipping, a short stretch of beats, or a lift
angle it had to assume. It does not grade a watch, say whether a figure is
good or bad, name a fault, or tell you what to do about it. Those
judgements depend on the watch, its age, its maker's tolerances and what
you want from it, and they are yours to make.

If you would like help with them, the
[watchmaker skill](../../skills/watchmaker/SKILL.md) is a guide written
for AI assistants. It reads the figures against published sources and
cites each one. The Claude Desktop extension carries it, and
[chapter 13](13-claude-desktop.md) shows how to use it.

## Contents

**Getting going**

1. [How a Timegrapher Hears a Watch](01-how-it-works.md): the beat, its
   three sounds, and what rate, amplitude and beat error mean.
2. [Your First Measurement](02-first-measurement.md): from the download
   to readings on the screen in a few minutes.
3. [Setting Up the Microphone](03-microphone.md): choosing the input,
   setting the level, clipping and automatic gain.

**The desktop app**

4. [The Window](04-the-window.md): menus, panes, the **?** buttons and
   arranging the screen.
5. [Readings](05-readings.md): the five cards across the top, the ±
   figures and what limits a reading.
6. [The Paper Strip](06-paper-strip.md): the classic trace, how to read
   its lines, and its controls.
7. [The Sound of the Beat](07-sound-of-the-beat.md): the Tick Tock
   Profile and the Scope.
8. [Over Time](08-over-time.md): the charts, Distributions, Steadiness
   and Isochronism.
9. [Calibres and Lift Angles](09-calibres.md): the calibre list, its
   sources, and adding your own.
10. [Positions and the Positions Test](10-positions.md): the six
    positions and the guided test, at full wind and 24 hours later.
11. [Sessions and Recordings](11-sessions-and-recordings.md): starting,
    stopping, saving, opening and replaying, and long takes.

**Beyond the app**

12. [The Command Line](12-command-line.md): every command, with real
    examples.
13. [Claude Desktop](13-claude-desktop.md): installing the extension and
    testing a watch in a chat.
14. [Problems and Reports](14-problems-and-reports.md): when something
    goes wrong, and how to tell us.
15. [Glossary and Sources](15-glossary.md): the words used in this guide,
    and the documents it cites.

## Conventions in This Guide

- Controls are named in **bold**, exactly as the app shows them, in Title
  Case: **Start**, **Average Over**, **Manage Calibres…**.
- Menus are written with an arrow: **View → Reset Panes**.
- On a Mac, read **Cmd** wherever the guide says **Ctrl**.
- **Tick** and **Tock** are the two kinds of beat a watch makes. The first
  beat the software hears is called the Tick. Ticks are drawn in blue and
  Tocks in orange everywhere.
- The six positions are **DU** (dial up), **DD** (dial down), **CU**
  (crown up), **CD** (crown down), **CL** (crown left) and **CR** (crown
  right). The Swiss names, the hour that points up, are shown beside the
  upright ones: CU is 3H, CD is 9H, CL is 6H and CR is 12H.
- Figures in the examples come from real recordings in the project's
  public [test corpus](https://github.com/peterhudson/timegrapher-corpus),
  played to the app through a virtual microphone. They illustrate the
  screens; they say nothing about how good those watches are.

## Other Documents

This guide is the place to start. The other documents in the
**Documentation** folder go deeper, and are written partly for developers:

- [Installing and updating](../install.md), including the steps past the
  warnings for unsigned software on macOS and Windows.
- [The desktop app in detail](../app.md): every control, setting and file.
- [Test sessions](../sessions.md) and [long runs](../long-runs.md) on the
  command line.
- [The command line for AI agents](../agent-interface.md) and
  [the MCP server](../mcp.md).
- [Validation against tg](../validation.md) and
  [measurement precision](../measurement-precision.md): how the figures
  were checked.
