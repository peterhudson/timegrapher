# Contributing recordings

The engine gets better by being tested on watches, microphones and rooms
it has not heard before. Every recording we are sent becomes a test case.
Each new engine build has to read it without crashing. Once
[tg](https://github.com/vacaboja/tg) has been run on the same file at the same lift
angle, every new build is also checked against tg's reading of it, as
`timegrapher regress` does today ([regress](regress.md)).

There are two ways to send a recording:

- **With a problem**: open a [*A reading looks wrong*](https://github.com/peterhudson/timegrapher/issues/new?template=bad_reading.yml)
  issue and attach the recording that shows it.
- **Without a problem**: open a [*Contribute a recording*](https://github.com/peterhudson/timegrapher/issues/new?template=recording.yml)
  issue. Healthy watches are as useful as troubled ones.

Nothing is ever uploaded by the app or the command line. A recording
leaves your computer only when you attach it to an issue yourself.

## What helps most

In order:

1. **A reading from another timegrapher taken at the same moment** on the
   same watch, with a photo of its screen. This gives a second opinion
   that does not come from tg.
2. **Beat rates other than 28,800**: 18,000, 19,800, 21,600 and 36,000
   vph, and older 18,000 vph watches with a soft tick.
3. **Different microphones**: piezo clips, contact microphones, phone
   headsets, cheap USB pickups, and the level that worked.
4. **Hard conditions**: a noisy room, a watch that clips the input, a
   watch running down at low amplitude, a magnetised watch.
5. **Anything the timegrapher reads oddly.**

A few minutes in one position, with the watch still, is plenty.
Say which position it was in and how it was held (a stand, a clamp, a
cloth). If the watch was tilted, say so: tilt alone can move the rate
by 10 s/d.

## Record losslessly

Keep recordings as **FLAC or WAV**. Do not use MP3, AAC or Opus. We tested
this on stored takes: no lossy format moves the rate, but even
320 kbps MP3 moves the amplitude by up to half a degree and the beat
error by up to 0.024 ms, nearly all of the 0.03 ms the regression check
allows. These formats remove sound above about 16–20 kHz, which you
cannot hear but the engine reads. MP3 at 320 kbps also saves only a third
of the space.

The app saves WAV (**Save…**, see [the app](app.md#sessions)), and `timegrapher
doctor --save take.wav` keeps the 10 s it checks. Both work as they are.
The app's WAV can be converted to FLAC (below), which is half the size and
loses nothing.

## Cutting a recording to size

GitHub takes attachments of up to 25 MB. At 48 kHz, 16-bit mono, that is
about **5 minutes of FLAC** (about 18 MB) or **4 minutes of WAV** (about
23 MB). If GitHub refuses the file type, zip it first.

With [ffmpeg](https://ffmpeg.org), this keeps 5 minutes starting 60 s in, as FLAC:

```sh
ffmpeg -ss 60 -t 300 -i audio-001.wav -c:a flac excerpt.flac
```

With [SoX](https://sourceforge.net/projects/sox/):

```sh
sox audio-001.wav excerpt.flac trim 60 300
```

In [Audacity](https://www.audacityteam.org), select the stretch, then **File ▸ Export
Audio…**, choose FLAC and export the selected audio only. Leave
any effects, noise reduction and normalising off: the file must be the
sound as recorded.

Pick a stretch where the problem shows, and say where it starts. Check
that the excerpt still reads the same with `timegrapher analyze
excerpt.flac --lift N`.

## Privacy

A sensitive microphone picks up the room. **Listen to the whole file
before you attach it**, and cut out anything with voices or other sounds
you would not want public. An issue and its attachments are public as soon
as they are posted. Write only what the form asks for: there is no need
for your name, address or the watch's serial number. Strip location data
from any photos you add.

## Licence

Recordings and the details sent with them are added to the test corpus
under [CC0](https://creativecommons.org/publicdomain/zero/1.0/) (no rights
reserved). Anyone, including other timegrapher projects, can use them
without having to give credit. On *A reading looks wrong*, that is
a separate box you can leave unticked: the recording then stays with the
issue only. On *Contribute a recording* it is the point of the form.

CC0 cannot be withdrawn once given. Even so, we will take a recording out
of the corpus if you ask. The corpus is separate from this repository's
code, which stays under the GPL-3.0.

## What happens next

A maintainer listens to the recording, measures it, and adds it to the
public test corpus with your details. You can ask to be credited under a
handle; otherwise no name is recorded. Once tg has been run on it, it becomes
one of the takes every engine change is checked against. If it showed
a problem, the issue tracks the fix.

A one-command report bundle is coming. It will cut the excerpt and gather
the version, setup and readings into one zip, from the app, the command
line or an AI agent helping you.
