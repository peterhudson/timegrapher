# Desktop app

The desktop app (**Timegrapher** in the downloads, `timegrapher-app` when
built from source) is the live timegrapher screen: put the watch on the
microphone and it shows the paper-strip trace with rate, amplitude and beat
error, the way a bench timegrapher does. A recording, or a folder of
segments, opens analysed all at once to look back through; it can also be
replayed as if it were live.

```sh
cargo run --release -p timegrapher-app             # live from a microphone
cargo run --release -p timegrapher-app -- rec.flac  # analyse a recording
cargo run --release -p timegrapher-app -- --replay rec.flac  # replay it as if live
```

On Linux, building it needs the ALSA headers (`sudo apt install libasound2-dev`).

## The screen

Along the very top, the menu bar. **File**: Open Recording… (Ctrl+O, or
Cmd+O on a Mac), Open Folder…, Save… or Save Again (Ctrl+S), Save As…
(Ctrl+Shift+S), Save Analysis Only…, New Session and Quit (Ctrl+Q).
**Edit**: Settings… (Ctrl+, or Cmd+, on a Mac). **View**: a tick for the
readings and for each pane, and Reset Panes. **Help**: Check for
Updates…, Report a Problem… and, on Linux, **Add to Applications Menu**,
which puts the app
and its icon (a black dial with red lines) in the desktop's menu, so the
window shows that icon too (it writes `timegrapher.desktop` and the icon
under `~/.local/share`, pointing at the program running now). Beside the
menus, one icon for each pane that shows or hides it (resting the pointer
on an icon names its pane), and a gear at the end that opens Settings.
Each icon is a small picture with a pun on
its pane: a pole dancer for the Paper Strip, a stethoscope for the Tick
Tock Profile, a die showing six for the Positions Test, a hare for the
Rate chart, a dromedary for the Rate distribution (one hump), an Allen
key for Allan, and so on. A pane showing has its icon in colour on a
tinted chip; a hidden one is faint grey. The charts and distributions
take their reading's colour, and the Steadiness views share one green.

**The Watches pane**, at the left to begin with, lists your watches, then
where the sound comes from and what to do with it, then the watch itself.
It is a pane like the others: drag it by its tab, or hide it and bring it
back from its icon or the View menu. At its top, under **Watches**, one
row per watch in the library (see The library, below): its first photo
(by name; a tile with its initial while it has none, or one that can't be
read), its name, its calibre, how many takes it has and the date of the last, and the space
they use, with the library's total size and folder and **Show in
Finder** at the foot. Its toolbar has a search (in the name, calibre,
notes and fields), **Sort** by Name, Last Take or Size, **Archived**
(once a watch is archived, to list those too), **Quick Listen** and
**New Watch…**. **Quick Listen** (off while listening) listens on the
microphone without picking or adding a watch first, for a watch handed
over the counter: it asks first when there is unsaved sound or an
unsaved change, as New Session does, then clears the take's name, shows
**Unsorted** and starts a new session. **Stop**, like **Save…**, saves
the take into Unsorted, named as a watch's takes are, and after a
Resume, Stop or Save Again adds the sound since to the same take.
Unsorted is listed above the watches while it holds a take, and its
takes have the same menu as a watch's, with **File Under a Watch** (into
the watch picked from it) in place of Move to Another Watch, so a take
waits there until it is filed or deleted. Picking a watch ends Quick
Listen. No watch can be called Unsorted. **New Watch…** opens a form:
only the **Name** is needed, and Calibre (which fills the Lift Angle and
Beat Rate), Lift Angle, Beat Rate and Notes can each be
removed with its ×, then given back, with any of the suggested fields or
one of your own, from **Add Field**; **Archived** hides the watch from the
list without deleting it. The first watch makes the library's folder.
Clicking a watch shows its details, its photos as a strip of thumbnails
(clicking one opens it in the computer's own viewer; right-clicking offers
**Open**, **Show in Finder** and **Delete…**, asking first; one that can't
be read shows its name and "Can't be shown" instead), and its **Takes**,
newest first: each
with its name, positions, length, the rate, amplitude and beat error (its
size) it measured when saved, the space it uses, and **Analysis Only** or
**Incomplete** when it is one. Clicking a take picks it and shows its
**Open** and **Replay** buttons under it; double-clicking opens it as
Open… does. The take on show is marked **On Show** and is not read again
when opened again. Right-clicking offers **Open**, **Replay** (when it has its sound),
**Show in Finder**, **Rename…** (its folder; nothing relies on the name),
**Keep Analysis Only…**, **Move to Another Watch** and **Delete…** (asking
first, with its size; it can't be undone). Keep Analysis Only… asks
first, saying the take's size and its sound's and what is lost: the take
can't be measured again without its sound, and Scope and Replay don't
work on it, while the readings, charts, Steadiness and the Positions Test
still do. It then reads the sound once more in the background, with
progress and **Cancel** under the take, writes the analysis beside the
take, checks it reads back, and only then puts it in the take's place
and deletes the sound; `session.toml` gains `audio = false` and keeps
everything else it says. It works on a take whose `session.toml` names
the whole folder in each recording entry, as the app's own takes do; a
take that lists its sound file by file is refused with a message saying
so. A take Save Again saves into, one being replayed or analysed, or one
whose analysis is being kept is not renamed, moved or deleted until that
is done. The toolbar then has **‹ All Watches**, **Edit…** and **Add
Photos…**, which copies the photos picked (JPEG, PNG, TIFF, or HEIC as
an iPhone takes them, each kept as it is) into the watch's `photos/`,
adding a number to a name already there. Thumbnails are read in the
background, turned the way the camera says, and kept in this computer's
cache folder (never in the library), so the next start shows them at
once.
Picking a watch, refused while listening, sets the take's name, calibre,
lift angle and beat rate from it; changing them under **This Take**
changes that take, not the watch. With a watch picked, **Save…** saves
the take into it with no question, in a folder named by when the take
started (in this computer's time) and its position when it used only one;
Save Again then saves into the same take, and Save As… still asks. A
library another copy of the app has open is read only: Quick Listen, New
Watch…, Edit…, Add Photos… and a photo's Delete… are off, and Save… asks where. Then **Microphone** or **Recording**. For the
microphone: the microphone menu and **Rescan**, **Start** (or **Stop** and
**Resume**), **New Session** (listening or not: listening carries on into the
new session, unless closing the kept sound fails), **Save…**
(then **Save Again** and **Save As…**), and the microphone's Input Level,
peak meter, Show Every Input and **Keep Awake** (see Long takes, below).
For a recording: **Open…**, **Open Folder…**, **Add to a Watch…** (once a
take folder from outside the library is open: into the watch picked
above or another, copied or moved; a copy is made beside its new place
and moved in whole, so one cut short is not a take), the path box, **Replay** (or
**Stop**, **Cancel** while one is being analysed, and **Analyse All**) and
**Replay Speed**. Below, under **Watch** (**This Take** with a watch
picked): name, calibre, beat rate, lift angle and position. Opening a recording, by **Open…**, **Open Folder…**, a
path typed into the box and Enter, or the command line, analyses it all at
once, which is how a watchmaker looks back through a take; **Replay**
plays a file through as if live (a folder's files one after the other, in
the order it is analysed in, with the watch from its session file), at
**Replay Speed** (from half to 16 times its own speed, changeable while it
plays; the readings are the same at any speed), and **Analyse All**, shown
during a replay and after it, goes back to the whole recording analysed at
once without opening it again, keeping the settings chosen under Watch.
Dropping a file or folder on the window opens it too where the desktop
passes drops to the app (Windows, macOS and X11; not Wayland). Clicking a
group's name, such as Watch, folds it away to its caption, and clicking it
again opens it.

When viewing a recording analysed as a whole, known jumps in its beat log also
bound each reading. Saved position and settling boundaries add to those cuts;
they do not replace them. Earlier and later pieces remain available to inspect.

When a replay has matching saved position history, its position bands and
settling intervals are restored in recorded-audio time. Readings do not span a
saved move or settling end. Choosing a position filters that history rather
than adding a new move. Stretches the take masked out come back masked.
A plain recording without matching position history still allows manual moves.

Every other pane's own settings sit in a toolbar across the top of the
pane, with the pane's **?** at its right; a toolbar too narrow for its
settings carries them onto a second line. The charts carry theirs in the
tab bar: **Match Strip**, on, shows the same stretch of time as the paper
strip, and off, the whole session. A choice between two options, such as
Stacked and Beside, switches to the other option wherever it is clicked,
so it can be flipped back and forth without moving the pointer. Every
pane but the Watches pane has **Export…** in its toolbar (a chart's in its
tab bar), which saves the pane as a picture: a PNG of it as it is on the
screen, or, with a name ending `.svg`, an SVG drawing whose lines and
text stay sharp at any size. A name with no ending is saved as a PNG.
In the picture the pane's name (with its own averaging time, if it has
one) and a chart's key take the place of its controls. The save starts
in the folder last chosen for a picture, else in the folder of the
recording last opened; the app forgets both when it closes. Titles and
labels are in Title Case. Across the top, the readings: five cards of one width, Readings (with
**Average Over**), Rate, Amplitude, Beat Error and Beat Rate, and below
them the panes. Along the bottom, the
status line. Every card, pane and reading has a **?** beside its name: resting
the pointer on it shows the plain answer in a line or two, and clicking it
opens the whole explanation in place (click it again, or anywhere else, to
close it). Every setting gives a short hint when the pointer rests on it. Before anything is
open, the main area offers the three ways in, beside the Watches pane: **Listen to a Watch**,
**Open a Recording…** and **Open a Folder…**. While a recording is being
analysed it shows how far the analysis has got, in hours of sound and time
left, with **Cancel**; a problem shows in a red banner floating over the
top of the main area, so nothing below it moves, until it is dismissed or
the next start clears it. A recording that can't be opened leaves the one
shown in place, and the path box goes back to it. Choosing **Recording**
while a stopped microphone session is in hand shows the ways in again
rather than that session's figures, and says the session is kept;
**Microphone** brings it back. The app follows the
desktop's light or dark setting (Appearance, in Settings, overrides it),
and uses the Inter typeface with figures of even width, so readings don't
shift as they change.

**A folder of segments.** A long take saved as segments (the recorder's
hourly files, or an overnight run split into 10-minute FLACs) opens as one
recording: **Open a Folder…** on the opening screen, **Open Folder…** in
the Watches pane or the File menu, a folder typed into the path box, or the folder's path given to the app
when it is started from a terminal (`timegrapher-app FOLDER`; see
[Without a window](#without-a-window) for its name in each download).
A saved take opens from its own folder or from the folder it was saved
into, when that holds just the one take; with several, the message names
them so one can be picked.
Its WAV and FLAC files are joined end to end in the order its
`session.toml` lists them as `[[recording]]` entries, with any it doesn't
list after them; without such a list, in natural name order, where a run
of digits counts as a number (`part2` before `part10`, `take_part01-099`
before `take_part02-001`). The files' dates never change the order, but
when a file was written earlier than the one joined ahead of it by more
than that file's own length, the window says so, so a wrongly named file
can be spotted. **Replay** plays a folder's files one after the other in
the same order. A 16-hour night takes some minutes to analyse; the
window shows the hours done, the time left and the files in order, and
Steadiness then covers the whole night.

**Readings.** The **Beat Rate** card gives the beats per hour the
readings use: found from the beats while Beat Rate under Watch is Auto,
or the one set there, and says which. Rate (seconds per day gained, +, or lost, −), amplitude (degrees) and beat error (ms), each a
fit over the last few seconds of beats (10 s by default; type any time
from 1 s to an hour, or pick one of the presets from 2 to 60 s; the setting is
Average Over, in the Readings card). Early in a session, or after a pause or a gap of
more than 2 s with no beats, there are fewer seconds of beats than asked
for: the readings use what there is and say so ("190 beats in 24 s of
180 s"); they never fit across a pause or gap, since the watch may have
been moved or have drifted meanwhile. The panes that average (the Paper Strip's
Rate Line, guides and overlays, the Tick Tock Profile, the three charts and the three
distributions) follow Readings' Average Over unless set otherwise: each has
its own **Average Over** in its toolbar (the charts' in their tab bar),
"As Readings" by default or one of the presets, and a pane with its own
time names it in its tab ("Rate · 30 s"). A pane's own time sets everything
in it that Average Over would. Changing Readings' Average Over puts every
pane back on it, dropping their own times. The big amplitude is the average of the two
swings, in whole degrees, with a ± figure: the standard error of the
median of its 2 s windows, estimated from successive window differences
and widened for the windows' lag-one correlation.
It is model-based precision, not accuracy: a wrong lift angle moves
every amplitude together (1° of lift angle moves it about 4°).
Under it are the amplitude from the ticks (blue dot) and from the tocks
(orange dot). The sound can't tell which beat is
which pallet, so the first beat heard is called the tick;
which side is the entry pallet can't be told from the sound, so the sign
of the beat error follows which beat came first, as on tg. The big beat
error is timed from the unlock, as tg measures it (checked against tg
in the regression tests); beside it, smaller, is the beat error timed from the beat as a whole,
nearer the drop, which is the gap between the two lines on the strip.
Beside the rate, when it is at least 0.05 s/d, is its ± figure: the
estimated standard error from short-window rates, scaled to the averaging
time. It is not the covariance error of the fitted beat-time slope.
Under the rate: how many beats the readings used and
the jitter: how far single beats land from the steady line the rate and
beat error are fitted to (a robust standard deviation, in microseconds),
which is the spread of the dots across the strip; smaller means less
scatter. Noise, a muffled sound and the stand add to it as well as the
watch. The **?** on
each reading explains it at length.

**What limits a reading.** Each figure carries its ±, worked out as
`analyze` and `session` work out theirs (`timegrapher_core::measure`),
over the averaging time. These are model-based precision estimates, not
bounds on total error or measures of the watch's true variation. Window
readings that follow one another are counted as fewer independent ones,
from their lag-one correlation; memory of another shape can still make
the estimates too small. The
[precision assumptions and probes](measurement-precision.md) explain what
is calculated and what has been checked. The amplitude's and beat error's ± are the
standard errors of their 2 s windows in the averaging time. The rate's ±
comes from the scatter of 10 s windows over the run so far (back to a
pause or a move, up to five minutes), scaled to the averaging time; it
needs about 30 s of unbroken beats. What the ± can't show is named, as a
plain fact, at the right of the reading's caption, with a sentence on
hover: "12 s of beats" when the beats span under 10 s, "540 of 600
expected beats found" when under 95% were found, "3% of beats clipped"
when over 1% clip (amplitude and beat error), "lift angle 52°, the
default (calibre not set)" when no calibre or lift angle is set
(amplitude), a beat rate set that differs from the one heard, and pauses
in the stretch. On the beat error, a reading of an opened file whose Tick
and Tock were named by the beat count across a break says so ("Tick and
Tock named by the beat count across 1 break"). Where the beat error's
sign differs either side of a cut (a pause, a jump, a move or its
settling) among the beats of the position the whole-take line covers
(the decision `analyze` makes over its stretches), that line under the
Beat Error card is the mean and spread of every reading's size, and a
line under it says "whole take given as a size: its sign differs either
side of a cut".
No reading is graded. That the input's clock hasn't been measured is
said in the Rate card's **?** rather than on the card, with how to
measure it. When the beats found don't come at regular intervals (their
timing scatter is 2% of a beat or more, `beats::watch_heard`, as
`analyze` uses), the cards show no figures, only **No regular beat found
(timing scatter 8.6 ms, 5.2% of a beat)**. When nothing in the take has
had a regular beat after its first few seconds, the cards and charts give
way to one card saying the same; they appear once a regular beat is
found.

**Long takes.** Listening carries on while the window is hidden behind
other windows, minimised, or with the screen locked or switched off: the
sound is saved and read as it comes in, on a thread of its own, so the
window shows everything heard as soon as it is back. The exceptions are a
file dialog (Open, Save, choosing a folder) and Save, Save Again or Save
As copying the take: meanwhile the sound waits in memory, and is saved and
read once they finish. With **Keep Awake** on (it is unless switched off),
the app asks the computer not to go to sleep on its own while it listens;
the screen can still lock and switch off as usual. If the system refuses,
a note under the switch says so. When the sound's own timestamps jump by
more than 5 seconds (as when the computer sleeps: a laptop's lid closed,
Sleep chosen from a menu, or Keep Awake off), the app leaves the jump out
of the take as a break, as after Stop and Resume, keeps listening, and a
message gives the length of the jump. A shorter hole (a dropout in the
sound, or a knock that costs part of a beat) shows in the beats. It is a
jump when the first beat heard after it is more than a quarter of a beat
off the count, when it follows a stretch too quiet to follow, or when the
timing of both sides steps by more than 4 ms, and by about the same, as a
whole recording's reading finds one. A step is found ten or more beats
after it, and the chart points read since are dropped; after a silent hole
one can show up to 4 s after the sound returns, even when no beat was
lost, as the silence leaves the 4 s of sound the beats are found in. No
reading or amplitude window reaches across a jump. Tick and Tock are then
told apart by their sound, the first 16 good beats of each side after the
jump against up to 32 before, so they keep their names. Until they are,
the beat error, each side's amplitude and the Tick Tock sound are left
out, and so they are in a stretch between two jumps too close together to
check. When the two sounds are too alike to tell, or fewer than 16 good
beats a side were heard before the jump, the sides follow the count, as in
a whole recording and at the start of every session. A hole that costs
close to an odd number of beats can leave no step to see, and then Tick
and Tock swap unseen: within 4 ms more than the beat error of it, or three
times the beat error if that is more (4.5 ms at a 0.5 ms beat error, 9 ms
at 3 ms), as in a whole recording, or anywhere within a quarter of a beat
of it when the beats just after the hole are too ragged to show the step.
The same check follows a change of position. The app's log notes every ten
minutes that listening is still going.

**Positions Test.** The Positions Test pane, a tab beside the Tick
Tock Profile to begin with (its icon is a die showing six, one face for
each position), leads through the six positions in the app's order: Dial Up,
Dial Down, Crown Down, Crown Left, Crown Up, Crown Right. Its toolbar
has **Start Test** (**Start a New Test** once one has finished), **Load
Full-Wind Results…**, **Settle** and **Measure** (the seconds the watch
settles after each turn and is then measured: 20 s and 60 s unless set
otherwise, kept between runs, 5 to 600 s and 20 to 3600 s), and **Save
as CSV…**, **Copy** and, after a test just after winding, **Save
Results for 24 Hours…**. While the pane is shown with nothing listened
to yet, it sits above the welcome card, so a test can start from it.
Start Test starts listening if need be and brings the pane to the front.
A banner at the top of the pane names the position and shows a progress
bar while the watch settles and is measured. It then says which position
to turn the watch to; press the button naming it (**Start CL
Measurement**) once it is there. **Restart DD Measurement**, naming the
position just measured (or the one under way), settles and measures it
again, after a knock for instance; **Pause** holds the test, leaving out
what the position measured so far, and **Resume** settles and measures it
again from the start; **Stop** ends the test and clears its table. Only
the measured seconds in each position count. From the moment the test
starts, the strip, its position ticker and the charts mark the test's
phases, Dial Up included, each in its own grey: **Before Test** over the
sound before the test began (left plain, with a line where the test
begins), **Settling** over each settle (a light shade), each position's
measured stretch (the darkest shade, named by the position), and **Not
Used for Positions Test** (hatched) over the time after a measurement
while the watch is being turned, over a measurement given up for a
restart or a pause, over the pause itself, and over the part of a
position measured when the test was stopped. Beats outside the measured
stretches are drawn faintly. The marks stay after the test until a new
test or a new session. Each position is a stretch of the one session, as
any change of position is, so the strip and charts mark them and a saved
session keeps them for `timegrapher session`.

Under the banner, the table fills in as each position is measured, read
with core `session::measure` as `session` does (rate on the input's
measured clock when there is one): each position by name ("Crown Up
(CU, 3H)"), its rate, its rate less the mean of the positions measured,
its amplitude and its beat error (a size); the position under way says
how far its settling or measuring has got. Then a Mean row, the largest
difference between the rates (D), vertical less horizontal for rate and
amplitude (DVH), and a plain line for each condition found in a
position's sound: no regular beat (with its timing scatter), clipping,
under 10 s of beats, a beat error given as a size because its sign
differs either side of a cut, or Tick and Tock named by the beat count
across a break rather than by their sound. After the sixth, **Finish**
reads the six together. The figures are `session`'s for the same
stretches. **Save as
CSV…** saves the watch's name, its calibre, beat rate and lift angle,
when the test began and its settling and measuring times, then the
table, the means, the differences and those lines, as a CSV file a
spreadsheet opens; **Copy** puts the same on the clipboard with tabs
between the columns. It gives no verdict and no pass or fail: the
figures are for the watchmaker to judge.

**Full wind against 24 hours.** After a test run just after winding,
**Save Results for 24 Hours…** saves its results, and saving the session
keeps the same results as `positions-test.json` in its folder. 24 hours
after winding, with the watch neither wound nor worn since, **Load
Full-Wind Results…** picks either file: the pane shows those results as
a Full Wind table with when that test began, the watch and the calibre,
and **Remove** sets them aside. Start Test then runs the six again
against them, and the banner names the hours since the full-wind test
began. As it goes, the pane adds a Full Wind Against N Hours table: each
position's rate and amplitude at full wind and now, and the change, then
the largest change in rate with crown right left out (Im, as NIHS 93-10
takes it) and the size of the change in the mean rate (Ie), all from
core `session::evaluate` as `timegrapher session` reads the two
together. Its CSV and Copy add the full-wind test's start, the hours
since and that table, and the session file marks its stretches with
`wind_h`, the hours since full wind.

`positions-test.json` (format `timegrapher.positions-test/1`; later
versions add keys) holds `format`, `began_utc` and `began_unix` (when the
test began), `watch` and `calibre` (as set then, or empty), `bph` and
`lift_angle_deg` (the beat rate and lift angle it was read at, `bph`
null when not known), `settle_s` and `measure_s` (the test's settling and
measuring in each position, seconds), and `positions`: one entry per
position in the order measured, with `position` (DU, DD, CD, CL, CU or
CR, as in `session.toml`), `rate_s_per_day`, `amplitude_deg` and
`beat_error_ms` (a size), each null when not measured. Files saved before
`bph`, `lift_angle_deg`, `settle_s` and `measure_s` were kept still load.

**Isochronism.** A tab beside the Steadiness panes, from the same run of tests: the
rate by amplitude in 2° steps (the median rate as a dot, with bars over the
middle half), the curve through them and where it turns, and under it the
first half hour against the half hour 24 hours later (or the session's last),
the usual 0 h / 24 h comparison, captioned Full Wind Against 24 Hours
Later. It needs the amplitude to fall
through at least three 2° steps, which a run-down over some hours gives.
A note on what it needs is behind the **?** in its toolbar. Until there are 5 minutes of beats in one position, the
Steadiness and Isochronism tabs say so; during a Positions Test they say
that the test never stays that long in one position. Both scroll with a
bar always showing when the pane is too short for every row.

**True Clock.** While it listens, the app times the microphone's clock
against the computer's. After 20 minutes of listening, once the timing is
known to half a ppm, the Rate card shows the error it timed with a
**Keep** button if no error is kept for that input yet, or if the one kept
is more than 2 ppm from it; then the line also gives how far apart they
are and the day the kept one was measured, since an input reached by a
shared name (the system default, say) may be another sound card. A busy
computer delivers sound less evenly, so the offer can come later. The
timing is only right when the computer's clock is kept by internet time
(NTP). When the microphone's own clock error has been kept (by **Keep**,
`timegrapher clock measure --save`, or `clock from-log --save` after a
take with a clock log; see `clock` in [the agent
interface](agent-interface.md#clock)), a line under the rate gives the
rate corrected onto true time, labelled True Clock, with the day the clock
it uses was kept. A sound card's crystal runs a steady 10 to 50 ppm off,
which moves every rate read on it by 1 to 4 s/d. The big figure always
stays on the card's clock, the same clock tg and `timegrapher analyze`
read, so it compares directly with them; the True Clock line is beside it,
never instead of it. It is looked up by the input's name when a microphone
session starts, and is not shown for a replayed recording. **Settings**,
under **Sound Card Clock**, lists each input's kept clock (its error in ppm
and in s/d, the day it was measured, how, and over how long) with
**Forget** beside it. Forget first asks, naming the input, its error and
the day it was measured, since getting it back takes 20 minutes or more of
listening; then it removes it from the same `clocks.json` that
`timegrapher clock list` and `clock forget` read, so the command line and
the MCP server forget it too; rates from that input are then read on its
own clock again, and the True Clock line goes, until it is timed and kept
afresh. Do this after changing the microphone or sound card behind an
input's name.

**Clipping.** Whenever a beat in the readings has a clipped sample in
its sound, counted as `timegrapher analyze` counts them, an amber banner
above the readings says **Clipping on 3% of beats** (or "under 1%").
Clipping flattens the loudest part of each tick, which moves
the drop and so the amplitude and the beat error; the rate is unaffected.
The banner follows the readings: it shows while the beats being averaged
clip and goes when they no longer do, and looking back through a
recording shows it for the stretches that clipped. The peak meter in the
Watches pane turns red on any clipped sample at once; the banner counts the
beats in the readings. Even a watch driven several times past full scale is
still read, with the banner, rather than taken for silence: when most of
the beats found clip, the flattened peaks no longer count against the
signal.

**Panes.** Below the readings are five panes: the paper strip, the tick tock
profile, and charts of rate, amplitude and beat error over the session. Drag a pane by its tab
to put it beside, above or below another, or onto another's tab to stack
them; drag the gaps between panes to resize them. The selected tab takes
the card's colour, joined to the card below it. The × on a tab hides
that pane (the status line says it comes back from its icon at the top or
the View menu), and the icons and the View menu hide or show each pane
and the readings across the top; a hidden pane comes back where it was,
and changing the strip's direction keeps the choice. **Reset Panes**
(in the View menu) shows them all in their starting places. Periodicity views will join
them.

**Tick Tock Profile.** The typical sound of the ticks (blue) and of
the tocks (orange) over the averaging time (Average Over, up to 60 s, so
it describes the same beats as the readings; a longer time gives a
steadier shape, and after lengthening it the beat count fills in as beats
arrive), from the drop's side of the beat back
through the unlock, with a shaded band where the middle 80% of beats fall
and the marks the engine read: unlock (green) and drop (indigo), from which
amplitude and the unlock-based beat error come, the drop's peak (a thinner
indigo line), the three sounds where the engine tells them apart (dashed brown: 1 unlock,
2 impulse, 3 drop) and the noise floor. Each line is named on the plot.
The solid edges and the dashed sounds are two different measurements of
the same averaged sound, not single beats: the edges are where the
readings come from, and the sounds are where each of the three sounds
rises halfway up its own climb, from a separate measurement of the
beat's shape, so they sit near but not exactly on the edges. A sound line is missing when
that sound can't be told apart from its neighbour (a soft unlock merged
into the impulse, say). The Tick tock
profile card switches the edges and the sounds on and off, sets the scale
and lays the tick above the tock (Stacked) or beside it (Beside). Tick and
tock share one vertical scale, so a quieter side shows smaller, and every
grid line is drawn alike.
Each plot has its own time axis. **Time From: Shared** (the default) draws
tick and tock on one clock: 0 ms is where a watch in beat would drop, and
each side sits half the beat error from it, so the two drop lines stand
the beat error from the drop apart and the two unlock lines the beat error
from the unlock. **Own Drop** measures each side from its own drop, so both
drops sit at 0 ms. The two beat errors differ when tick and tock take
different times from unlock to drop, the imbalance that also gives them
different amplitudes; the unlock figure is the one tg reports. Above each plot are the beat count, the marks in ms
from that side's own drop, the unlock-to-drop time and the amplitude they
give. Linear (the
default) shows the envelope as the engine measures it; dB shows the level
below each side's loudest point, so the quiet unlock shows more clearly. Looking back, the pane shows the sound the session
kept nearest before that moment (one every 2 s); for a recording analysed
all at once it is worked out from the file when you stop on a moment.
When the tick's and tock's marks disagree, or a mark sits mid-ramp, the readings
built on them are suspect.

**Scope.** The sound itself, beat by beat, like the scope view on a
Witschi (whose training course, doc. 71.1010D35e p. 12, gives the
same 20, 200 and 400 ms spans): what the engine heard, high-passed at 1 kHz, not an average. A
tab beside the Tick Tock Profile, set in its toolbar.
**Span 20 ms** shows the Tick and the Tock
in rows of their own, lined up on the drop, from 14 ms before it to 6 ms
after, with the profile's Unlock and Drop marks. **5 ms** (3.5 ms before
the drop to 1.5 ms after) and **10 ms** (8 ms before to 2 ms after) show
the drop closer still, and **50 ms** (15 ms before to 35 ms after) the
sound and what rings on after it, all with the same marks. **One Beat**
runs from 15 ms before one beat to 15 ms before the next, at the
watch's beat rate (125 ms at 28,800 bph, 200 ms at 18,000). **200 ms** shows one
row each from 20 ms before the beat to 180 ms after, so the next beat
shows too. **400 ms** shows every beat in one grey row, three or four
beats at once. **Beats** draws the latest 1, 4, 8 or 16 over each other,
the older ones fainter, so a beat that differs from the rest stands out.
While listening it shows the latest beats; looking back, or on a
recording, it shows the beats up to the moment shown, read from the
recording. Only the few seconds around the beats are read, however long
the recording: a WAV seeks there and a FLAC is searched by its frame
headers (`span.rs`). No crosshair, as on the other sound plots.

**Paper Strip.** One dot per beat, tick and tock in two colours. Across
the strip is how early or late each beat came against a perfect clock
beating exactly at the nominal beat rate (every 125 ms at 28,800 bph): a
beat before that clock's beat is early, after it late, the words Witschi's
training course uses (doc. 71.1010D35e p. 6). Along it is time; both axes
are labelled, in ms across and in minutes and seconds along, and every
grid line is drawn alike. **Rate Line** (on by default) draws the Rate
reading as a thin solid red line over the beats it was fitted to, on top
of them, as tg does: if the dots follow it, the reading describes them
well. **Parallel Guides** (on by default) adds faint red lines at the same
slope across the whole strip, one per grid step, since the eye judges
whether lines are parallel far better than it judges a slope; dots that
bend away from the guides show the rate changing. The guides show with or
without the Rate Line. Under **Draw Over the Strip**, amplitude (purple,
on by default), rate (red) and beat error (yellow) readings can be drawn
as lines against the same time, each on its own scale, whose ends are
written in its colour above the strip (beside it when the strip lies
across), as some versions of tg do for amplitude: a wobble in the rate
that comes with a dip in amplitude shows at a glance. The lines use
readings spaced a four-hundredth of the strip's length apart (about 4.5 s
on a 30-minute strip, never closer than half a second), or the
recording's own when those are no more than twice as far apart, and
break only where the readings do.
Running **across** (the default), time goes left to right with the newest
beats on the right, and early is up, with the strip above the charts.
Running **down**, as on tg, the newest beats are at the top, a watch on
rate draws a vertical line, a gaining watch leans right as it rises (/)
and a losing one leans left (\\). A direction already chosen and saved
stays as it was. The gap between the two lines is the beat error. A line
that runs off one edge comes back on the other, so the strip's width is
its zoom and its length sets how much history it shows. Type both (a width
of ±0.1 ms to ±60 s, typed in ms; a length such as `45`, `5 min`, `2h` or
`1:30:00`) or pick a common value from the list beside each: the widths
run from ±1 ms, for a good watch's beat error, through ±62.5 ms, half a
beat at 28,800 bph and the widest view in which tick and tock can't wrap
onto each other, up to ±60 s, so a long run-down take that drifts by many
seconds can be shown without wrapping; the lengths run from 10 s of beats
to a two-hour run, then **Fit**, which sets the length to the whole
session so far, or its last 24 hours when it is longer, and shows it up
to its latest beat.

On the strip, the mouse wheel changes the length, Ctrl and the wheel (or a
pinch) changes the width, dragging along the time axis looks back through
the session (live or not), dragging across slides the trace, and a double
click returns to the newest beats and centres them. **Auto-Centre** keeps
the newest beats on the centre line all the time, so the older ones slide
away from it as the rate wanders; sliding the trace by hand turns it off.
**Centre** centres once.

**Mask** leaves stretches out of the readings, such as a bump of the
stand in a long take: with it on, dragging along the strip over the beats
masks that stretch (one under half a second is left alone, as a slip of
the mouse), dragging a masked stretch's edge moves it, and clicking a
masked stretch, or right-clicking one with Mask on or off, brings it
back; **Unmask All** brings them all back. Stretches that overlap, by
masking or by moving an edge, join into one and stay joined. Masked stretches are shaded and named on the strip,
and the toolbar says how many there are and how long they add up to.
They are cut like a pause, at both ends: no reading reaches across one,
nothing is read inside one, a chart's line breaks there and comes back
once nine tenths of an averaging time of beats has been heard after it
(as at the start of a session and after a pause), the steadiness tests
and their views leave out any 10-second reading (rate, amplitude or beat
error) that takes in either end of one, as they do one with a 2-second
stretch of no beats in it, and the charts, the whole-take figures, the
distributions (their 2-second stretches too), the steadiness tests and a
Positions Test leave their beats out; a Positions Test's positions are
read again when what is masked changes. A reading whose averaging time
takes in masked sound says how many seconds were masked, unless it is
under 0.05 s; the reading Report a Problem sends, and `timegrapher`'s
JSON, keep the caveat and its seconds even then.
A take saved from a session in the app keeps every masked stretch
(`masked` on each recording of its `session.toml`, in seconds of the
saved sound; see [sessions.md](sessions.md)), so `timegrapher session`
leaves them out too, and all of them come back masked when the saved
take is opened or replayed. Masks on a recording opened from disk are
kept by **Save Masks** (beside **Unmask All**, in the File menu while
they're unsaved, or Ctrl+S, Cmd+S on a Mac), and New Session, opening
another recording, starting the microphone (or a Positions Test with it)
or quitting asks first when they've changed; they stay on show until
listening starts. On a take it changes only `masked` on the session file's
entries for what was opened, written whole through a file beside it;
comments and everything else stay as they were. A lone file, or a folder
with no session file, gets a new `session.toml` with what saving a take
recorded here would write for the same settings (its one entry is the
whole folder), which makes its folder a take that the app and
`timegrapher session` open as one; opening the file itself then reads it
too. It is refused, saying why, when the folder holds other recordings, or
a session file that doesn't list what was opened. Report a Problem sends
the masked stretches with its details. While Mask is on, dragging along
the strip masks instead of sliding the trace. Mask turns off, and nothing
is masked, when a new take, New Session or another recording starts.

**Charts.** Rate, amplitude (the average, from the ticks and from the tocks) and beat error (from the
unlock and from the drop) over time. Each reading keeps its colour across
the app (rate red, amplitude purple, beat error yellow), so a second line
of the same reading is told apart by its dashes: amplitude from the ticks
dashes purple over blue and from the tocks purple over orange, and beat
error from the drop is a fainter dashed yellow; the key and the box at the
pointer draw them the same way. A chart
with more than one line has its key at the right of its tab bar, where it
covers nothing. Each chart labels its vertical scale at two marks or more,
in small text, however short the chart; a long time at the right end of
the time axis is left out rather than cut in half. **Time Span**, in the Charts and
Distributions cards, sets what they cover: **Session**, everything since the
session started, or **Strip**, the same stretch of time as the paper strip,
moving with it, so that with the charts under a strip lying across every
time axis lines up: the strip and the charts keep the same width of axis
on the left and of overlay scales on the right, so their times sit one
above the other. In Strip mode the strip sets the time, so only the
vertical scale can be dragged or zoomed. Wherever the pointer is on the
strip or a chart, a thin line marks that moment on all of them, and on the
strip a label gives its time from the start to the hundredth of a second
(such as 1:23.45). With the pointer
anywhere over a chart, a vertical line marks that moment and a box lists
every line's value there. Their time axes move together:
the mouse wheel zooms time around the pointer, Ctrl and the wheel zooms
the vertical scale, dragging pans, and a double click fits the whole
session again and, while listening, follows new beats as they come in.
Once a chart has been panned or zoomed a **Follow Live** button
(**Show Whole Session** for a recording) appears on it and does the same,
and the box at the pointer lists these controls. Clicking a moment shows it on the strip and in the
readings. The scale holds every value in view, so no point is drawn
off the chart. Beat error is charted as a
size, as on its card. Changing the averaging time or the
lift angle works every point out again, back to the start of the session.

**Distributions.** How the values of rate, amplitude and beat error spread
over the session or the strip's length (Time Span), switched on in the
Distributions card (all off at first). An average gives one number; these
show whether a watch keeps to one state or moves between two, such as a
high and a low amplitude. Rate, amplitude and beat error count either
the readings (**Values: Readings**, the default, averaged over Average
Over as on the charts and the strip) or each 2 seconds of beats (**2 s**,
the finest the engine measures them, with more scatter; none reaches
across a break or into a masked stretch).
**Show: Probability** (the default) is a normal probability plot, the
probability paper of flood-frequency analysis: each value against its rank,
on a vertical scale spaced in standard deviations and labelled in percent
(Hazen plotting positions). Values from one steady state fall along the
dashed straight line, drawn through the median with a slope from the
interquartile range; two states draw two straighter stretches joined by a
bend, or an S across the line. **Cumulative** draws the share of values at
or below each value on a plain scale. Neither keeps the time order, so the
strip and the charts show when a change happens. The scale leaves out the
most extreme 0.5% at each end so stray readings can't squeeze the rest
together, and the line above gives the count, the median, the middle 80%,
the outliers left off and the peaks when there is more than one. The
statistics are in `timegrapher_core::histogram`, so the CLI and reports can
share them.

**Steadiness.** How the rate, the amplitude and the beat error (from the
unlock) each change over the session, from the same tests as
`timegrapher series` (see [series.md](series.md)). Each view is a pane of
its own, so two can sit side by side; their icons sit together under
**Steadiness** in the icon bar (and in a Steadiness menu under View), and
they start as tabs beside the Tick Tock Profile, with only Readings Over
Time shown. Each series shows the sentence `series` writes, saying what
was detected (a cycle, two levels, a step, a trend, correlation between
readings, or none of these), with no verdict word or colour, and under it
the statistics: readings, median, standard deviation, the scatter from one
reading to the next, the lag-1 correlation and Ljung–Box p-value, and the
trend and period when found. **Rate**, **Amplitude** and **Beat Error**,
in each pane's toolbar, choose which series that pane shows; at least one
stays on.

Known phase jumps and position boundaries remain cuts in the derived
Steadiness log, including when separate visits to one position are joined.
Desktop rate readings do not bridge those cuts; the overall fit still combines the
pieces using their existing beat weights. Joining also keeps amplitude sides
and signed beat errors aligned with the renumbered beats. This preserves
supplied context; it does not detect new jumps or change the statistical tests.
Amplitude-window eligibility across internal cuts is unchanged. Where the
beat error's sign differs either side of a cut, its readings are drawn as
sizes, positive, as the tests beside them read them.

The views:
**Readings Over Time** (the readings in time order, with the stretches of
constant level the change finder found and, for two states, the two
levels dashed; scaled to the readings with lone wild ones left out),
**Periods** (the period search's score at each period on a logarithmic
axis, with the 1% false-alarm line dashed, the escape, fourth and centre
wheels' periods marked, and the strongest component labelled; **Log
Scale** in its toolbar draws the scores on a log scale, so a small peak
beside a tall one still shows), **Cycle**
(the readings folded at the cycle the tests found: every cycle as dots,
coloured by when it came from purple for the first to yellow-green for the
last, so slow wander shows as a run of colour; a box round the middle half
of each phase's readings, for a phase with at least four; the median shape
as a line; and a note giving the line's swing, highest less lowest, beside
the boxes' median height), **Autocorrelation** (with the ±1.96/√n band
dashed, the repeat marked and the Ljung–Box p-value), **Allan Deviation**
(log-log, against the dashed line independent readings follow, labelled
where it ends, with the averaging time of least scatter labelled),
**CUSUM** (with the ±1.36 lines dashed and the largest excursion
labelled) and **FFT** (the plain amplitude spectrum of each series, like
Witschi's: the size of the sine wave at each period, log-log, in the
series' own units, the rate's as the swing peak to peak it makes in 10 s
rate readings, in s/d, from 20 s up, worked out from the beat timing's
spectrum (a timing sine of *a* ms at period *P* swings the rate by
2·*a*·2π/*P*·86.4 s/d, of which a 10 s reading keeps sin *x*/*x*, *x* =
π·10/*P*); **Timing** in its toolbar draws rate as the beat timing's own
spectrum, in ms, at every period; the wheels' periods are marked and the
tallest peak labelled: the highest point above both its neighbours, so a
spectrum still climbing at either end of its axis, as the rate's slow
wander builds up in the timing at the longest periods, labels its own
peak rather than the end, and none when it has no peak), **STFT Map** and
**Wavelet Map** (each series' size at each period, up on a log axis with
the longest at the top, and time of the session, across, from core's
time-frequency maps: the STFT from the amplitude spectrum of each 20
minutes of readings stepped 5 minutes, the rate's of the beat timing in
ms, and the wavelet map from a synchrosqueezed wavelet transform, with
each unbroken stretch read alone; coloured in viridis from dark purple
for the smallest size to yellow for the size 99.5% of the cells read stay
under and above; a cell nothing was read for, such as a window with a
reading missing or a stretch's ends the wavelet can't reach, is left
empty and shown as "not read" in the key under the map; the wheels' turns are
dashed across it and named at the right; **Log Scale** in the toolbar
colours the sizes on a log scale over a range of 300 to 1; the pointer
over a cell shows its time, period and size). In Periods and FFT each wheel's
period is a bold line and half of it, twice a turn as an off-centre wheel
shows, a fainter dashed line marked ×2; the grid behind is pale. The tests need 5 minutes
of beats. They run over the whole session in the background, again every
minute while it grows; for a recording they cover what has been replayed,
or the whole file once it is opened. Rows stack and fill the pane when it is
tall enough, sit side by side when it is wide and short, and scroll
otherwise. A line at the top says in plain words what the view
shows. The plots don't pan or zoom, and only the two maps show values under the
pointer.

**Remembered settings.** The app keeps its views between runs: the strip's
direction, width and length and what is drawn over it, the profile's
layout, scale and Time From, the panes shown and their arrangement,
whether the Readings show, the distribution settings, the averaging time,
folded groups in the Watches pane, the appearance, Keep Awake, the last
microphone (used again when it is plugged in) and the window's size and
place. Nothing about the watch is kept: the lift angle, beat rate and
position start fresh, since the next watch on the pickup may differ.
**Reset Panes** puts the panes back as they started.

**Log and crash records.** The app writes a log, `logs/timegrapher.log`, in
the timegrapher settings folder (`~/.config/timegrapher` on Linux,
`~/Library/Application Support/timegrapher` on macOS,
`%APPDATA%\timegrapher` on Windows; the folder that holds `clocks.json`),
where `timegrapher report` collects it: the version, the microphone
opened, level changes, beat lock gained and lost, position changes and
every message the app shows. It is kept under 1 MB, with the two files
before it. If the app panics it also writes `crash-<time>.txt` there with
the message, where it happened and the last 200 log lines, and the next
start shows **The timegrapher closed unexpectedly last time. Make a
report?** with **Make a Report…** (copies the record and opens a new GitHub
issue to paste it into), **Show the Record** and **Dismiss**. Nothing is
sent anywhere by the app itself.

**If the window can't open.** The window draws with OpenGL 2.0 or newer.
Where the graphics can't (a virtual machine or remote desktop without 3D
graphics, a missing graphics driver), the app tries again with wgpu, which
uses Direct3D 12 on Windows (including Windows' own software renderer),
Metal on macOS and Vulkan on Linux. The log records what the window drew
with. If neither opens a window, or the window closes before drawing
anything, the app says why in the log and, on Windows and macOS, in a
message box, and exits with an error. An error that ends the window after it
has drawn is reported the same way.

**Report a Problem.** **Report a Problem…** at the right of the status line
makes the same zip as `timegrapher report` (see [reports.md](reports.md)) to
attach to a GitHub issue. Choose what kind of problem it is (Bug, Reading,
Setup or Idea) and say what happened. **Include the Sound** adds up to five
minutes of the sound, cut without changing a sample: the last five minutes of
a live session (the app keeps the session's sound as it goes), or five
minutes of the recording open, chosen with **Ends At**. Drag the handles on
the waveform to keep only the part that shows the problem, and **Play** it.
**Save Report…** is offered once you tick that you listened and the sound
holds nothing private, such as voices; ticking the CC0 box offers it to the
public test corpus. The zip holds the excerpt and `analyze --json` of it
(which `report --replay` runs again), the app's readings and settings in
`results/app.json`, the log and crash records from the last week and the
stored clock corrections, with the home folder, user name and host name
removed. Saving opens the zip's folder and the matching GitHub form; the app
sends nothing itself.

**Appearance.** In Settings, under General: Auto follows the desktop's
light or dark setting; Light and Dark override it.

**No input touched at launch.** Listing the inputs asks each one what it
supports, which briefly opens it and can disturb another program recording
from the same microphone (a long take running beside the app, say). So the
app lists them only when the microphone menu opens, **Rescan** is pressed
or **Start** is clicked; until then the menu names the microphone kept from
last time, or System Default. Replaying or analysing a recording never
touches an input.

**Level meter.** In the Watches pane, while listening to a microphone: a bar to
the loudest sample of the last half second, grey, or red while the input
clips, with plain reference marks at −10 and −6 dBFS, and the peak in
dBFS beside it ("peak −14 dBFS", with ", clipping" while it clips).

**Status bar.** The input; the signal, "signal 12×", how far the typical
beat's peak stands above the typical level between beats (noise alone
reads about 2×, and below 4× no beats are logged). It measures
loudness, not the shape of the tick, so hum, knocks or a holder that
smears the sound aren't caught by it. Then how much sound the session
holds and whether it is saved, and on the right the beat rate and the
position, **Check for Updates…** and **Report a Problem…**.

**Updates.** With **Check for Updates at Start** on (in Settings, under
General; on unless switched off), the app asks GitHub once at start, on a
background thread, whether a newer release is published
(`timegrapher_core::update::check`). If one is, a banner says
"Timegrapher 0.2.0 is available" with **Download**, which opens the
release's page in the browser. Nothing installs itself. At start, a
failed check or an up-to-date version says nothing. **Check for
Updates…** in the status bar or the Help menu makes the same check and always says what it
found: the notice, "up to date", or why it couldn't check ("no release has
been published yet"). The request carries no account or identifier.

## Settings

**The Settings window.** Edit > Settings… (Ctrl+, or Cmd+,) or the gear at
the end of the pane icons opens it over the app, with four sections:
**General** (Appearance and Check for Updates at Start), **Library**
(below), **Calibres**
(Manage Calibres, below) and **Sound Card Clock** (each input's kept
clock with Forget; see True Clock). It opens at the section it was last
left on. **Done**, Escape or a click outside closes it. What belongs to
the watch on show (its calibre, lift angle, beat rate and position) and
to the microphone (input, level, Show Every Input, Keep Awake) stays in
the Watches pane, beside what it sets.

**The library.** The watches and their takes are to be kept in one
folder, `Timegrapher Data` in the home folder unless another is chosen:
`library.toml` at the top, then a folder for each watch holding its
details (`watch.toml`), its `photos/` and its `takes/`, and `Unsorted/`,
whose `takes/` holds the takes Quick Listen made until each is filed;
each take a standard take folder named by when it started (`2026-10-20
14.32 Dial Up`, with the position in words when the take used only one).
`timegrapher watches list` and `show` read it from the command line,
without changing it ([agent-interface.md](agent-interface.md#watches)),
and `timegrapher record --watch NAME` records a test into a watch's
takes while the app doesn't have the library open
([agent-interface.md](agent-interface.md#record)). Nothing in
it records where it is, so it can be moved, synced or copied whole; a
take with sound files missing is listed as incomplete and never changed.
Settings' **Library** section shows where it is and how many watches and
takes it holds and their size, with **Show in Finder** (Explorer on
Windows, File Manager on Linux), **Move Library…** (into a folder picked,
asking first; onto another disk it is copied, each file with its
modification time, checked and only then removed, and a link or a
special file such as a named pipe in it stops the copy, leaving the
library as it was; while it moves, closing the window or File > Quit is
refused and the app stays open; on macOS Cmd+Q and Quit in the app menu
or the Dock do not wait, and on any system neither does logging out,
shutting down or ending the app from outside; a move cut short that way
leaves the library whole where it was, or whole at the new place once
its copy was finished, which Choose Another… then finds; a copy cut
short leaves its files at the new place without a `library.toml`, which
is not a library and can be deleted),
**Choose Another…** (a library elsewhere, or an empty folder
for a new one; nothing is moved) and, once another is chosen, **Use the
Default**. Only one copy of the app at a time can change a library: a
second copy opens it to read only and says so in Settings, and the
library can't be moved while another copy has it open. The Watches pane adds watches to it and saves takes into them.

- **Input.** In the Watches pane: a sound input device, or a WAV or FLAC file chosen with
  **Open…** or typed in. On Linux the menu offers each microphone once:
  **System default (via PipeWire)**, which is whatever input the
  desktop's sound settings choose and works alongside other programs, and
  each card's direct device, marked "(direct)", which goes straight to
  the card but can't open while the desktop sound server holds it. With a
  USB timegrapher microphone set as the desktop's input, choose the system
  default. **Show every input** lists everything the system offers, with
  ids, as `timegrapher devices` does (the switch is under Microphone). If the input can't be opened, stops,
  or sends no sound for 3 s, the app stops and says why above the
  readings, rather than showing an empty strip. The status bar shows the
  sample rate and bit depth the input opened at. Where the desktop passes
  dropped files on (X11, Windows, macOS; not yet Wayland), dropping a file
  or a folder on the window analyses it. The app asks the device for 48 kHz in as few
  channels as it offers, mixed to mono, and takes 16-bit samples when the
  device has them.
- **Input level.** Under Microphone in the Watches pane: the microphone's gain and a peak meter
  with marks at the −10 dBFS target and the −6 dBFS limit. On Linux, for
  the system default the level is the sound server's input volume
  (`wpctl`, or `pactl` on PulseAudio), which the server writes back onto
  the microphone each time it opens it, so setting the card's own control
  there would not last; for a direct device it is the card's capture
  control (`amixer`). The change is made when you let go of the slider.
  On other systems, set the level in the system's sound settings. The
  same code (`timegrapher_core::mixer`) gives `timegrapher doctor` its
  proposals. When the microphone's automatic gain is on, the card says
  **Automatic gain: on** with a **Turn It Off** button, and otherwise
  says nothing about it.
- **Calibre.** Not Set, a calibre from the list (`timegrapher_core::calibres`,
  some 4,500 of them, each with its sources), or one of your own.
  - **The menu.** It offers Not Set, the calibres picked lately
    (**Recent**), the **Short List** by maker and **Your Calibres**, with
    **Manage Calibres…** at its foot and as a button under it.
  - **Manage Calibres.** The Calibres section of Settings, which
    Manage Calibres… opens, with every calibre in a
    list down the left, by maker (the common and modern makers first, your
    own calibres at the top), and the one selected in full on the right.
    **All**, **Short List** and **Yours** choose what the list shows.
    Typing in its search box searches the whole list. Every word must
    match a calibre's number, one of its other names, or its maker under
    any of the maker's names ("dd 3235", "sw200", "seagull st19"), and the
    best matches come first. **Use for This Watch** sets the selected
    calibre under Watch. **New Custom Calibre…** adds one, named as typed
    in the search box.
  - **The short list.** It starts as the list's common and modern
    calibres. In Manage Calibres a tickbox beside every calibre puts it on
    the short list or takes it off, and **Tick All** and **Untick All**
    beside a maker do a whole maker at once: untick the clones, say, or
    keep just one maker's calibres. **Reset the Short List** goes back to
    the one the app comes with. The short list and the recent picks are
    kept with the app's settings. When the list renames a calibre (the
    "vph" names became "bph"), the short list, the recent picks and a take
    saved under the old name all use the new one.
  - **What a pick does.** It fills in the calibre's lift angle, where the
    list gives one, and names the cycles Steadiness finds after its own
    wheels. If the watch beats at another rate than the calibre, a line
    under the menu says so. With none picked and the lift angle left at
    52°, a line says the amplitude is worked out from that default.
  - **The panel.** Under the menu, a line gives the maker, beat rate, lift
    angle and escape wheel, or says which of them the list doesn't give.
    **Wheels and Sources**, folded until opened, lists each wheel's time
    for one turn, as Manage Calibres does for the calibre selected there:
    - The six every lever movement has (balance, escape, fourth, third
      and centre wheels, and the barrel) come first, then any other the
      list names.
    - The hint on each wheel says where its figure comes from.
    - A wheel nobody has given is blank: type its period and it is kept
      for that calibre, marked "yours", with **Clear** to go back.
    - **Another wheel** adds one by name.
    - Below come the list's sources as links, how its figures are known,
      and its notes.
  - **Your additions.** Wheel periods and custom calibres are kept in
    `calibres.json` in the settings folder (core `calibre_store`), which
    the command line and the MCP server read too. Calibres typed in under
    earlier versions move there the first time the app starts; their original
    settings copy is kept until the catalogue has actually been saved.
    If the catalogue cannot be read (including invalid JSON or an unsupported
    layout), Manage Calibres shows a persistent warning, even with no calibre
    selected. Built-in calibres remain available, but catalogue edits in that
    app instance stay in memory and cannot overwrite the unreadable file.
    Repair or move the file, then restart before making changes to keep; the
    app does not silently reload it or merge temporary edits. An ordinary save
    error also leaves edits in memory and preserves pending legacy settings
    until a later save succeeds. These protections do not add numerical
    validation or concurrent-edit/crash-recovery guarantees to the catalogue.
  - **Share with the Project….** It opens a GitHub issue in the browser,
    filled in with what you added to the calibre, to look over and send.
    Nothing is sent from the app.
  - **A custom calibre.** Its panel, and Manage Calibres, also have its
    name, maker, beat rate and lift angle, and **Delete Calibre**.
  - **What isn't kept.** The pick itself, like the lift angle, is about
    the watch on the stand and isn't kept. Not Set names cycles after the
    wheels most calibres at the beat rate share.
- **Beat rate.** Auto (guessed from the first seconds) or any standard rate
  from 12,000 to 72,000 bph. Changing it starts the readings again.
- **Lift angle.** 52° by default. Type the calibre's angle and press Enter,
  or pick a common one. Amplitude is worked out again at once, history
  included.
- **Position.** DU, DD, CU, CD, CL, CR, with the Swiss laboratory names
  beside the crown positions, the hour that points up: Crown Up (CU, 3H),
  Crown Down (CD, 9H), Crown Left (CL, 6H) and Crown Right (CR, 12H).
  The Positions Test's table names them the same way. A new position is a new
  measurement, so changing it while listening (or paused) starts a new
  stretch, noted in the saved recording. While listening, when the beats
  stop for more than 3 seconds and come back with no change of position
  marked, a banner asks whether the watch was moved, with a button for
  each position and **Not Moved**; it goes by itself a minute later. The strip keeps its beats and
  marks each stretch, shading every other one, with a ticker above the
  plot (beside it, when the strip runs down) that names the position over
  and over along its stretch, so it shows even with both ends of the
  stretch scrolled out of sight; the Rate, Amplitude and Beat Error charts mark them the same way
  over the session or the strip's length. The readings don't reach back
  past the change, the tick and tock profile starts again, and the
  distributions and Steadiness take only the stretches in the position the
  watch is in now: dial up for 5 minutes, dial down for 10, then dial up
  again tests the two dial-up stretches together and leaves the dial-down
  one out. The first 10 seconds in a new position are settling (from
  Resume, when the position was changed while stopped): the ticker says
  Settling, the beats are drawn faintly, and they are left out of the
  readings, the charts' points, the distributions and Steadiness, but kept
  in the audio. A saved take's `session.toml` starts each later position
  after its settling, so `timegrapher session` reads the same seconds as
  the app. Guided runs through the positions (as `timegrapher
  session` reports them) will build on it.

## Sessions

Opening the app never touches the microphone; only **Start** opens it.
When another program records from the same microphone (a long take with
the recorder, say), don't press Start during its take, and press Stop
before one begins.

**Start** begins a session from the microphone. **Stop** stops
listening and lets go of the microphone, so another program can use it
(Microphone Off shows beside the level meter), and keeps everything: the
readings, strip, charts and the sound so far.
**Resume** carries on in the same session: the strip and charts leave
the stopped time out, and the readings start from the beats after the
stop. It carries on only with the input and the sample rate the session
was listening at; with another, it says which and stays stopped.
**New Session** clears everything for the next watch or position;
pressed while listening, listening carries on into the new session
(unless closing the kept sound fails: see Recording stopped, below).
Everything heard is kept as it comes in, so **Save…** can be pressed at
any time, running or stopped: it asks for a folder and copies the session
into a folder of its own inside it. **Save Again** (Ctrl+S) later brings
that copy up to date without asking again, and **Save As…** (Ctrl+Shift+S)
copies the session to a new folder of your choosing, which Save Again then
keeps up to date. Sound that hasn't been saved is never thrown away
without asking: New Session, opening or replaying a recording, closing
the window and File > Quit ask whether to save it first. Until it is
saved it is kept in a folder of the app's own (`timegrapher/unsaved`
under `~/.local/share` on Linux, `~/Library/Application Support` on a
Mac, `%LOCALAPPDATA%` on Windows), which the system doesn't clear. When
the app quits without asking (on a Mac, Cmd+Q and Quit in the app menu or
the Dock, which can't be held back; on any system, a logout or a
shutdown) with more than 5 s of unsaved sound, or is stopped by a crash,
the sound stays there, and the next start says so in a
banner, with when it started (in this computer's time), its length and
size, and **Open** (analysed here, and offered again next time),
**Move To…** (the recording's folder moved into one you pick, to open
with Open Folder…), **Delete…** (asking first) and **Later**. A folder
another copy of the app is recording into is never offered. The
microphone's own settings are kept aside while recordings are open and
come back with **Microphone**.
Once stopped, the time slider shows for a microphone session too; stopped
during a move's settling time, the rate card gives the reading from before
the move and says so.

Each saved recording is a folder named after the start time, then the
watch's name and the calibre when they are set by the first save
(`2026-10-08T21-30-00_Yacht-Master_Rolex_3235`, or just the time), so it
can be analysed again later with a newer engine. The positions aren't in
the name: `session.json` and `session.toml` say where the watch was and
when. It holds:

| File | Contents |
| --- | --- |
| `audio-001.wav`, ... | The audio as captured, mono, 16-bit for a 16-bit microphone, in files of at most an hour; listening again after a pause starts a new file |
| `clock.csv` | System time against frames captured, every 10 s, and on both sides of each pause; frames run on across a pause, with no line made up for the gap |
| `session.json` | Watch, position, lift angle, beat rate, device, start and end, position changes, the readings at the end, and (only once the take has been paused) `breaks`: one entry per resume with `frame` (frames in the take before it), `audio_s`, `segment` (its first file), `utc` (host time of its first frame), `gap_s` (host time missing, from the clock log either side) and `device` (the input after it) |
| `session.toml` | The session file `timegrapher session` reads: watch, calibre (with the wheels of a custom one), lift angle, beat rate, and one `[[recording]]` per stretch in one position with its `start_s` and `end_s` (for a take saved during a Positions Test, the test's own measured stretches, after each settling and for its measuring time, so `timegrapher session` reads the seconds its table did, with `wind_h` in the 24 hour half) |
| `positions-test.json` | Only for a finished Positions Test run just after winding: its results, to load for the test 24 hours later (the format is under Full wind against 24 hours) |

Opening a recording or folder that has a `session.toml` beside it (or in
it) that lists it takes back the watch, the calibre (a custom one joins
Your Calibres), the lift angle, the beat rate and the position stretches,
so the strip and charts mark them and Steadiness tests each position on
its own. Without one, a recording opens with the settings back at their
defaults (no watch name or calibre, 52°, the beat rate found, Dial Up), so
nothing carries over from the last recording, except that a position named as a word of its own in the file's or
folder's name (`reverso_DU_01.flac`) is taken, and the message says so
with the lift angle in use.

The folder reads straight into the long-run analysis, calibrated against
the system clock:

```sh
timegrapher long "2026-10-08T21-30-00_Yacht-Master_Rolex_3235/" \
  --clock "2026-10-08T21-30-00_Yacht-Master_Rolex_3235/clock.csv"
```

The WAV header is brought up to date every 10 s, so a recording survives
the program being stopped abruptly.

**Save Analysis Only…** keeps a microphone session's readings without its
sound, for long takes whose sound would fill the disk. Once listening has
stopped, it asks for a folder and reads the whole session's sound again,
in the background with its progress (and a Cancel) in the status bar:
once for the beats and amplitude windows, as opening the take would find
them, then for the Tick Tock profiles of each 2 seconds, the signal
figure, the clock error the USB lines show and each position's beat
shape. It then writes a new folder, named as a saved take with
`_analysis` on the end, holding `session.toml` (with `audio = false`),
`session.json`, `clock.csv`, `positions-test.json` when there is one, and
an `analysis/` folder, and no sound; the message says how much space it
takes. A folder already there is never written over. While it works, a
progress bar in the status bar shows how far it has read, with Cancel,
which leaves nothing written; listening, New Session and opening a
recording are refused with a message saying it is still keeping the
analysis. The session's sound itself stays unsaved: the status bar says
its analysis is kept, and Save still keeps the sound. The layout is
in [sessions.md](sessions.md#a-take-kept-as-its-analysis-alone).

Opened, such a take shows its readings, Paper Strip, charts, Steadiness
tests and position stretches as the take with its sound would, from the
beats it kept, and the Tick Tock Profile from the profile kept for the
2 seconds before the moment shown. It can't be measured again with
other settings, and the Scope and Replay need the sound: the Scope says
there is no waveform, and Replay says the take can't be replayed. Masks
can still be changed and saved into its `session.toml`.

## Without a window

For scripts, tests and an AI agent helping someone set up. The program
is `timegrapher-app` when built from source, as below; in the downloads it
is `Timegrapher.exe` on Windows, `Timegrapher.AppImage` on Linux and
`Timegrapher.app/Contents/MacOS/timegrapher-app` on macOS.

```sh
timegrapher-app --devices          # sound input devices and their formats, as JSON
timegrapher-app --headless rec.flac --every 10 --average 10
```

`--headless` runs a recording through the live engine as fast as it can and
prints a JSON line of readings every `--every` seconds of audio (also
`--bph`, `--lift`). The same calls are in the core library
(`timegrapher_core::capture` for devices, capture and levels,
`timegrapher_core::live` for the live readings,
`timegrapher_core::recorder` for saving), so the command-line tool can
offer them too. On Windows the app opens without a console window, so
these two print only when their output is redirected to a file or piped
to another program (`timegrapher-app --devices > devices.json`).

## How the live readings are made

Every half second the last 4 s of audio go through the same envelope,
template and beat tracker as a recording (`timegrapher_core::live`). Beats
are added to a running beat log once they are at least a quarter of a
second from the end of the audio, so each beat is measured once, with the
same template from the first seconds onward; that keeps the beat's
reference point still, so the strip doesn't jump. Amplitude is measured on
2 s windows of that log, as in a recording. The readings are fits over the
log, so the live screen and an analysis of the saved recording agree: on
the 5-minute 3235 recording, the live jitter (165 µs median) matches the
command line's 166 µs.

Expect the first reading about 4 s after the watch goes on the microphone.
If nothing is heard for 5 s with the beat rate on Auto, the template is
dropped and the next watch is picked up from scratch, even at a different
beat rate.

## Recording stopped: recovery files retained

If keeping the sound or closing a recording fails, capture stops and the
app shows the scratch folder in a warning that stays for as long as that
session does; after New Session it can be dismissed, and the next Start
clears it. Those files may
contain incomplete audio or clock information. Resume, Save and Save Again
are unavailable for that session so an earlier saved take stays unchanged.
This is different from an input disconnect alone, which can still be resumed.

Copy the folder shown in the warning to a location outside the app's
folder for unsaved sound before attempting manual recovery or cleanup; do
not treat it as a complete saved take. The next start also offers it back
as unsaved sound. The app attempts to leave a
`RECORDING-ERROR.txt` note there and reports if even that write fails. New
Session, opening another recording, and quitting leave the faulted folder on
disk. After resolving the storage problem, start a new session.
