# 14. Problems and Reports

This chapter covers what to do when something doesn't work, and how to
tell the project about a problem, a reading that looks wrong, or an idea.

## Common Problems

### macOS or Windows won't open the app

The builds are not signed with an Apple or Microsoft developer
certificate, so the system warns you the first time you open each new
version. On macOS, use **Open Anyway** in System Settings → Privacy &
Security; on Windows, **More info** then **Run anyway**. Your download's
**Start Here** page and [Installing and updating](../install.md) give the
steps.

### The microphone menu is empty, or the input won't open

- Check that the microphone is plugged in, then press **Rescan**.
- **macOS**: System Settings → Privacy & Security → Microphone, and turn
  Timegrapher on.
- **Windows**: Settings → Privacy & security → Microphone, and turn on
  *Let desktop apps access your microphone*.
- **Linux**: choose **System default (via PipeWire)** rather than a
  **(direct)** device; a direct device can't open while the desktop's
  sound server holds the card.
- Close other programs that may be using the microphone.

If the input stops or sends no sound for 3 seconds, the app stops
listening and says why in a red banner above the readings.

### The readings say "No regular beat found"

The beats found don't come at regular intervals. Check that the watch is
running, that it is held firmly against the microphone, that the level is
up (the **Peak** meter moves with each beat), and that the room is quiet.
See [Readings](05-readings.md#no-regular-beat-found).

### The amplitude looks wrong

- Check the **lift angle** under Watch: every amplitude depends on it. See
  [Calibres and Lift Angles](09-calibres.md).
- Look for the amber **Clipping** banner: clipping moves the amplitude.
  Turn the level down. See
  [Setting Up the Microphone](03-microphone.md#setting-the-level).
- Look at the [Tick Tock Profile](07-sound-of-the-beat.md): if the Unlock
  or Drop mark sits somewhere odd, the amplitude built on it is suspect.

### The rate differs from another timegrapher's

Both are timed against their own clocks. A computer's sound card clock is
typically 10 to 50 ppm off, which moves the rate by 1 to 4 s/d. Measure
yours once (see [True Clock](05-readings.md#true-clock)). Also check that
both were measured over the same stretch, in the same position, with the
watch equally wound.

### The window doesn't open

The app draws with OpenGL 2.0 or newer. Where that isn't available (a
virtual machine or remote desktop without 3D graphics, or a missing
graphics driver), it tries again with a second graphics system (Direct3D
12 on Windows, Metal on macOS, Vulkan on Linux). If neither works, the app
says why in its log and, on Windows and macOS, in a message box.

### On Linux, the AppImage says FUSE is missing

Install `libfuse2` (Ubuntu 22.04: `sudo apt install libfuse2`; Ubuntu
24.04: `sudo apt install libfuse2t64`), or run the AppImage with
`--appimage-extract-and-run`.

## If the App Closes Unexpectedly

If the app crashes, it writes a record of what happened. The next time it
starts, a banner offers to help you report it:

![The banner after a crash: The timegrapher closed unexpectedly last time. Make a report? with Make a Report…, Show the Record and Dismiss](images/crash-banner.png)

- **Make a Report…** copies the record and opens a new GitHub issue to
  paste it into.
- **Show the Record** shows the file.
- **Dismiss** closes the banner.

Nothing is sent anywhere by the app itself.

## Report a Problem

**Report a Problem…**, in the Help menu and at the right of the status
line, makes a zip file to attach to a GitHub issue:

1. Choose what kind of problem it is: **Bug**, **Reading** (a figure looks
   wrong), **Setup** or **Idea**, and say what happened.
2. **Include the Sound** adds up to five minutes of the sound, cut without
   changing a sample: the last five minutes of a live session, or five
   minutes of the recording that is open, chosen with **Ends At**. Drag the
   handles on the waveform to keep only the part that shows the problem,
   and **Play** it to check.
3. Tick that you listened to it and it holds nothing private, such as
   voices. Then **Save Report…**.
4. If you made the recording and are happy to share it, tick the box that
   offers it to the public test corpus under CC0 (no rights reserved).
5. Saving opens the zip's folder and the matching GitHub form. Attach the
   zip to the issue.

The zip holds the sound excerpt and the command line's analysis of it,
the app's readings and settings, the log and crash records from the last
week, and the stored clock corrections, with your home folder, user name
and computer name removed. **A wrong reading can only be fixed with the
sound it came from**, so please include the sound when a figure looks
wrong.

From the command line, `timegrapher report` makes the same zip (see
[The Command Line](12-command-line.md#report) and
[Reporting a problem](../reports.md)).

### Issues on GitHub

The project's [issues page](https://github.com/peterhudson/timegrapher/issues/new/choose)
has forms for a wrong reading, a bug, an idea, and contributing a recording
of a watch to the public test corpus.
[Contributing recordings](../contributing-recordings.md) says what to
record and how.

## Where Things Are Kept

The app keeps its settings, logs and your additions in a settings folder:

| System | Settings folder |
| --- | --- |
| macOS | `~/Library/Application Support/timegrapher` |
| Windows | `%APPDATA%\timegrapher` |
| Linux | `~/.config/timegrapher` |

In it:

- `logs/timegrapher.log`: the app's log (the version, the microphone
  opened, level changes, beat lock gained and lost, position changes and
  every message the app shows), kept under 1 MB with the two files
  before it;
- `crash-<time>.txt`: a record of each crash;
- `calibres.json`: the wheel periods and custom calibres you added;
- `clocks.json`: the sound card clock corrections you kept.

Saved takes go wherever you chose when saving. Updating the app keeps all
of these.

## Updates

With **Check for Updates at Start** on (in the Help menu; on unless you
turn it off), the app asks GitHub once at start whether a newer version is
published, and shows a banner with **Download** if so. **Check for
Updates…** asks at any time. To update, download the new version and
install it over the old one, as the first time. See
[Installing and updating](../install.md#updating).

---

Previous: [Claude Desktop](13-claude-desktop.md) ·
Next: [Glossary and Sources](15-glossary.md)
