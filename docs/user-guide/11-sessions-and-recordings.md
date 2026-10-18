# 11. Sessions and Recordings

Everything the app hears is kept as it comes in. You can save it, open it
again later, and look back through it, live or not. This chapter covers
sessions from the microphone, saved takes, and opening recordings made by
the app or by anything else.

## A Session from the Microphone

A **session** is everything heard since you started listening: the sound,
the beats, the readings and the positions.

- **Start** (or **Listen to a Watch**) begins a session from the microphone.
  Opening the app never touches the microphone; only Start does.
- **Stop** stops listening and lets go of the microphone, so another
  program can use it, and keeps everything: the readings, strip, charts
  and the sound so far.
- **Resume** carries on in the same session. The strip and charts leave the
  stopped time out, and the readings start from the beats after the stop.
- **New Session** clears everything for the next watch or position.

![The Watch pane after Stop: Resume, New Session and Save… under the microphone menu](images/stopped-buttons.png)

### Looking Back

Once you stop, a **Time** slider appears under the readings. Drag it, or
drag along the Paper Strip, to look back to any moment: the readings, the
Tick Tock Profile and the Scope all show that moment. Stopped during a
move's settling time, the Rate card gives the reading from before the move,
and says so.

![The Time slider under the readings, at the end of a 1 minute 41 second session](images/time-slider.png)

## Saving

Everything heard is kept as it comes in, so you can save at any time,
listening or stopped:

- **Save…** asks for a folder and copies the session into a folder of its
  own inside it.
- **Save Again** (Ctrl+S) brings that copy up to date later without asking
  again.
- **Save As…** (Ctrl+Shift+S) copies the session to a new folder of your
  choosing, which Save Again then keeps up to date.

Sound that hasn't been saved is never thrown away without asking: New
Session, opening or replaying a recording, and closing the window all ask
whether to save it first.

### What a Saved Take Holds

Each saved take is a folder named after its start time, then the watch's
name and calibre when they are set, such as
`2026-10-08T21-30-00_Test-watch_Rolex_3235`. It holds:

| File | Contents |
| --- | --- |
| `audio-001.wav`, … | The sound as captured, in files of at most an hour each; listening again after a pause starts a new file |
| `clock.csv` | The computer's clock against the sound captured, every 10 seconds, used to correct the sound card's clock in a long run |
| `session.json` | The watch, position, lift angle, beat rate, input, start and end, position changes, pauses, and the readings at the end |
| `session.toml` | The session file the command line reads: watch, calibre, lift angle, beat rate, and one entry for each stretch in one position |
| `positions-test.json` | Only after a Positions Test run just after winding: its results, for the test 24 hours later |

The positions are not in the folder's name; `session.json` and
`session.toml` say where the watch was and when. Keeping the sound means a
take can be analysed again later, with a newer version of the software. The
sound files are brought up to date every 10 seconds, so a take survives the
program being stopped abruptly.

## Opening a Recording

Timegrapher reads **WAV** and **FLAC** recordings, from the app or from any
other program. Choose **Recording** at the top of the Watch pane, then:

- **Open…** (or **Open a Recording…** on the welcome screen, or File →
  **Open Recording…**, Ctrl+O) opens one file;
- **Open Folder…** (or **Open a Folder…**, or File → **Open Folder…**)
  opens a saved take, or a long take saved as a folder of files;
- or type a path into the box and press Enter.

Dropping a file or folder on the window opens it too, on Windows, macOS
and Linux desktops that pass dropped files to the app (X11, but not yet
Wayland).

A recording opens **analysed all at once**, which is how to look back
through a take. The main area shows how far the analysis has got, in hours
of sound and time left, with **Cancel**:

![Analysing test-watch-2h (12 files): a progress bar at 12%, 14:06 of 2:00:00 of sound analysed, about 0:26 to go](images/analysing.png)

### The Watch's Details

When a recording or folder has a `session.toml` beside it (or in it) that
lists it, opening it takes back the watch's name, the calibre, the lift
angle, the beat rate and the position stretches, so the strip and charts
mark them and Steadiness tests each position on its own.

Without one, a recording opens with the settings at their defaults (no
name or calibre, 52°, the beat rate found, Dial Up), so nothing carries
over from the last recording, and you set them under Watch. A position
named as a word of its own in the file or folder name, such as
`reverso_DU_01.flac`, is taken from the name, and the status line says so.

### A Folder of Segments

A long take saved as a folder of files (the app's hourly files, or an
overnight run split into 10-minute FLAC files) opens as one recording. Its
files are joined end to end in the order its `session.toml` lists them,
or, without a list, in natural name order, where a run of digits counts as
a number (`part2` before `part10`). The files' dates never change the
order, but when a file was written earlier than the one before it, the app
says so, so a wrongly named file can be spotted.

A sixteen-hour night takes some minutes to analyse; the window shows the
hours done, the time left and the files in order.

## Replay

**Replay** plays a recording through the app as if the watch were on the
microphone: the strip draws, the readings move, and everything works as it
does live. A folder plays its files one after another.

- **Replay Speed** runs from half to 16 times the recording's own speed,
  and can change while it plays. The readings are the same at any speed.
- **Stop** stops the replay.
- **Analyse All**, shown during a replay and after it, goes back to the
  whole recording analysed at once, without opening it again and keeping
  the settings under Watch.

Replay is a good way to learn the app without a watch on the microphone,
using the recordings in the project's public
[test corpus](https://github.com/peterhudson/timegrapher-corpus). The
screenshots in this guide were made that way.

Replaying or analysing a recording never touches the microphone. Choosing
**Recording** while a stopped microphone session is in hand keeps that
session; **Microphone** brings it back.

## Long Takes

The app is happy to listen for hours. For the longest runs:

- Keep the computer from sleeping, and leave other programs off the
  microphone.
- Save early with **Save…**; **Save Again** keeps the copy up to date.
- Measure your sound card's clock once (see
  [True Clock](05-readings.md#true-clock)), so long-run rates can be
  corrected onto true time.
- Open the saved folder later with the command line's `timegrapher long`
  for a report over hours or days (see
  [The Command Line](12-command-line.md#long)).

## If Saving Fails

If keeping the sound or closing a recording fails, for example because
the disk is full, capture stops and the app shows the scratch folder it
was using, with a warning that stays. Copy that folder somewhere outside
the system's temporary folder before trying to recover anything from it:
it may hold incomplete sound. Resume and Save are unavailable for that
session, so an earlier saved take stays as it was. Once the problem is
fixed, start a new session.

---

Previous: [Positions and the Positions Test](10-positions.md) ·
Next: [The Command Line](12-command-line.md)
