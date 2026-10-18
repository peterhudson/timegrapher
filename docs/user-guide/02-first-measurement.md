# 2. Your First Measurement

This chapter takes you from the download to a watch's readings on the
screen. It takes a few minutes, most of them spent letting the watch
settle on the microphone.

## What You Need

- **A computer**: a Mac with macOS 11 (Big Sur) or later, a Windows 10 or
  11 PC (64-bit), or a 64-bit Linux PC with a distribution from 2022 or
  later.
- **A microphone that hears the watch.** The usual choice is a
  timegrapher microphone: a stand with a clamp that holds the watch
  against a contact pick-up, plugged in by USB. Many inexpensive ones are
  sold for use with timegrapher software. A contact (piezo) pick-up with a
  preamplifier also works. A laptop's own microphone can sometimes hear a
  loud watch in a silent room, but it rarely hears the quiet unlock that
  the amplitude is worked out from.
- **A mechanical watch with a Swiss lever escapement**, the escapement
  most mechanical watches use; Timegrapher is built for it. Wind the watch
  fully before you start.
- **A quiet room.** Fridges, fans, keyboards and footsteps all reach a
  contact microphone through the table.

## Install

Download the file for your computer from the
[Releases page](https://github.com/peterhudson/timegrapher/releases/latest):
a disk image (`.dmg`) for macOS, or a zip for Windows or Linux. Each holds
the app, a **Command Line** folder, a **Claude Desktop** folder (macOS and
Windows), this guide and the other documentation in a **Documentation**
folder, and a **Start Here** page that says how to open the app the first
time. On macOS, drag the app onto Applications. On Windows and Linux,
extract the whole folder somewhere you keep and run the app from there:
there is nothing to install. The builds are not signed with an Apple or
Microsoft developer certificate, so macOS and Windows warn you the first
time you open each new version; the Start Here page and
[Installing and updating](../install.md) give the steps past that warning.

When the app first listens, your computer asks whether Timegrapher may use
the microphone. Allow it.

## Open the App

When the app opens, the main area offers three ways in, and the
**Watch** pane on the left holds the microphone's settings and the
watch's details.

![The app before anything is open: the Watch pane on the left, the Positions Test pane at the top, and three buttons in the middle: Listen to a Watch, Open a Recording… and Open a Folder…](images/welcome.png)

- **Listen to a Watch** starts listening to the microphone chosen at
  the top of the Watch pane.
- **Open a Recording…** opens a WAV or FLAC recording and analyses all of
  it at once.
- **Open a Folder…** opens a recording the app saved, or a long take saved
  as a folder of files.

Opening the app never touches the microphone. Only **Listen to a Watch**,
**Start** or **Rescan** does.

## Step by Step

1. **Put the watch on the microphone**, dial up, in the clamp or against
   the pick-up.
2. **Choose the microphone** in the menu at the top of the Watch pane. If
   your timegrapher microphone is your computer's chosen input, the
   default works. [Setting Up the Microphone](03-microphone.md) explains
   the choices.
3. **Tell the app about the watch**, under **Watch**:
   - **Name**: anything you like. It names the folder when you save.
   - **Calibre**: pick the movement from the list, if you know it. This
     fills in the lift angle, which the amplitude depends on. See
     [Calibres and Lift Angles](09-calibres.md). If you don't know it,
     leave it at **Not Set**: the app uses a lift angle of 52° and says so
     beside the amplitude.
   - **Beat Rate**: leave it at **Auto**. The app finds it from the beats.
   - **Position**: leave it at **Dial Up (DU)** for now.
4. **Click Listen to a Watch** (or **Start** in the Watch pane). The first
   readings appear about 4 seconds after the watch is heard.
5. **Check the level.** The **Peak** meter in the Watch pane shows how
   loud the sound is. If it turns red, or an amber **Clipping** banner
   appears above the readings, the sound is too loud and the amplitude
   and beat error will read wrong. Turn the level down, as
   [Setting Up the Microphone](03-microphone.md) describes.
6. **Let it settle.** Leave the watch for a minute without touching the
   microphone or the table. The readings are fitted to the last 10 seconds
   of beats, so they move a little from one second to the next; that is
   the watch and the measurement, and the ± figures say how much.

![The app listening to a watch: readings across the top, the Paper Strip in the middle, and the Tick Tock Profile and charts on the right](images/main-window.png)

## What You Are Looking At

Across the top are the **readings**: rate in seconds per day, amplitude in
degrees, beat error in milliseconds, and the beat rate. Below them:

- The **Paper Strip**, one dot per beat, in the style of a printing
  timegrapher. See [The Paper Strip](06-paper-strip.md).
- The **Tick Tock Profile**, the typical sound of each kind of beat. See
  [The Sound of the Beat](07-sound-of-the-beat.md).
- **Charts** of rate, amplitude and beat error since you started. See
  [Over Time](08-over-time.md).

Every card and pane has a **?** beside its name. Rest the pointer on it
for a line or two of explanation, or click it for the whole explanation.

## When You Are Done

- **Stop** stops listening and lets go of the microphone, keeping
  everything heard so far.
- **Save…** copies the session, sound included, into a folder you choose,
  so you can open it again later. Nothing is lost if you forget: the app
  asks before throwing away sound that hasn't been saved.
- **New Session** clears everything for the next watch. Pressed while
  listening, listening carries on into the new session.

[Sessions and Recordings](11-sessions-and-recordings.md) covers these in
full.

## No Microphone Yet?

You can try everything in this guide on a recording. The project's public
[test corpus](https://github.com/peterhudson/timegrapher-corpus) holds
real recordings of several watches, free to download and use. Click
**Open a Recording…** and choose one, or use **Replay** in the Watch pane
to play it through the app as if the watch were on the microphone.

---

Previous: [How a Timegrapher Hears a Watch](01-how-it-works.md) ·
Next: [Setting Up the Microphone](03-microphone.md)
