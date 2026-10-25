# Desktop app

The desktop app (**Timegrapher** in the downloads, `timegrapher-app` when
built from source) is the live timegrapher screen: put the watch on the
microphone and it shows the paper-strip trace with rate, amplitude and beat
error, the way a bench timegrapher does. A recording, or a folder of
segments, opens analysed all at once to look back through.

```sh
cargo run --release -p timegrapher-app             # live from a microphone
cargo run --release -p timegrapher-app -- rec.flac  # analyse a recording
```

On Linux, building it needs the ALSA headers (`sudo apt install libasound2-dev`).

The app's look and behaviour are set by the [Design Guide](design/design-guide.md),
and how its parts fit together by [App Structure](design/app-structure.md).
Where today's app differs from them, the app changes.

## The screen

The window has two views, the **Bench** and **Watches**, picked with the
**Bench | Watches** switch at the left of the row under the menus, or
with Ctrl+1 and Ctrl+2 (Cmd+1 and Cmd+2 on a Mac). The Bench is the take
on the microphone or opened: the readings cards and the panes. Watches
is your watches and their takes (below). The app always opens on the
Bench. Switching views never stops, pauses or saves a take, and the
status bar shows the take's state in both. Open on a take, or starting
Listen or a test from a watch, goes to the Bench.

Along the very top, the menu bar. **File**: Open Recording… (Ctrl+O, or
Cmd+O on a Mac), Open Folder…, Save… or Save Again (Ctrl+S), Save As…
(Ctrl+Shift+S), Save Analysis Only…, New Session and Quit (Ctrl+Q).
**Edit**: Remove All Masks… (asks first), Settings… (Ctrl+, or Cmd+, on a Mac). **View**: **Bench** (Ctrl+1)
and **Watches** (Ctrl+2), then a tick for the readings and for each pane,
and Reset Panes (dimmed in Watches, as the panes are the Bench's).
**Watches**: **New Watch…**, **Find in Watches…**, which shows the
Watches view's list with its search ready to type in. Going back to
Quick Listen is **No Watch (Quick Listen)** on the Calibre card's title
(below). **Help**: Check for
Updates…, Report a Problem… and, on Linux, **Add to Applications Menu**,
which puts the app
and its icon (a black dial with red lines) in the desktop's menu, so the
window shows that icon too (it writes `timegrapher.desktop` and the icon
under `~/.local/share`, pointing at the program running now). Beside the
menus, on the Bench, one icon for each pane that shows or hides it (resting the pointer
on an icon names its pane), and a gear at the end that opens Settings.
In Watches the pane icons hide and leave their room, so the gear stays
exactly where it is on the Bench.
The icons come in groups: the core panes first (the Paper
Strip, the Tick Tock Profile and the Scope), then **Analysis** (Charts,
Distributions, the Steadiness views and Isochronism) and **Tests** (the Positions Test and the Power Reserve Test), each
under its name. The window is never smaller than 800 by 600 points, so
the panes' toolbars and Settings always have room. Where the window is too narrow for every icon, the
largest groups fold into a menu under their name, with the number of
their panes showing, until the rest fit.
Each icon is a small picture with a pun on
its pane: a pole dancer for the Paper Strip, a stethoscope for the Tick
Tock Profile, a die showing six for the Positions Test, a camel for
Charts (it charts a course across the take, as across a desert), a
jellyfish for Distributions (its body is called its bell), an Allen key
for Allan, and so on. A pane showing has its icon in the accent blue on a
tinted chip; a hidden one is grey. No pane icon takes a reading's colour.
The readings have pictures of their
own wherever the app shows or switches one: a hare for the rate, a swing
for the amplitude and a seesaw for the beat error, in their colours.
A pane of several plots, one for each reading (Charts, Distributions and
the Steadiness panes), shows the rate alone until its switches turn the
others on; each pane keeps its switches from then on.

**The readings**, across the top, are six cards, left to right:
**Current Position**, **Calibre** and **Listen**, then **Rate**,
**Amplitude** and **Beat Error**, sharing the rest of the width. Every card is the same three rows,
so the titles, the big figures and the controls each read in one line
across: a title row (the caption, the card's **?** in its top-right
corner, and what limits the reading in a small chip beside the ?), a
figure row (the big figure with its ± and unit on the figure's baseline)
and a controls row. What stands behind a figure is in its hover: the
beats the rate is fitted to and their jitter, True Clock, the Tick and
Tock amplitudes, the beat error from the drop, how the beat rate was
found (on the Calibre card), and, for a stopped take, the whole take's reading (see below).
**Current Position** shows the watch as it lies, in teal, the colour of
position information everywhere, with no stand: the case, its crown at
3, the numbers 12, 3, 6 and 9, and the hands at 10:10. Standing, the
whole watch is turned so the hour that points up is at the top, numbers
and all, as the watch in your hand would be: the crown on top for Crown
Up, at the bottom for Crown Down, on the left for Crown Left and on the
right for Crown Right. Lying, it is drawn flat with its side wall below:
the dial for Dial Up, and for Dial Down only the case back's fluted
ring, crown on the left. The picture sits at the card's left, across the
title and figure rows; beside it are the caption, the name ("Crown
Down") and both codes, the common one then Witschi's ("CD · 9H"; DU · CH
and DD · CB lying flat). Across the controls row are six buttons, **DU DD
CU CD CL CR**, the current one filled teal. Each one's hover gives its
name, codes and key ("Crown Down (CD, 9H) · key 4") and how long each
change of position settles (the Positions Test pane's Settle). Along each
button's foot a teal bar, over a faint track, fills with the seconds
heard in that position, in Quick Listen as in a test or a take on show:
settled and not masked, each position's own time only. It is full at the
Positions Test pane's Each Position (60 s unless set otherwise; at 120 s, 60 s
heard fills half), so a free-form test shows which positions still need
time; the hover gives the seconds. While you listen, or before anything
is on show, a button sets the position; keys 1 to 6 do the same, on the Bench,
alone (never with Ctrl, Cmd, Alt or Shift held) and not while typing in
a field. With a take on show, and during a Positions Test
or a Power Reserve Test, the buttons only show the position, the take's
latest or the one the test set: a take's positions can't be changed
afterwards. **Listen** has a **Listen** icon, which starts Quick Listen
(below), or with a watch picked that watch's Listen take, and becomes a
**Stop** icon while listening; beside it a **Save** icon (a floppy disk),
whose menu offers **Keep in a Watch** (your watches, then **New
Watch…**; shown for a Quick Listen take, and for a recording opened from
outside the Watches folder) and **Save in a Folder…**, which asks where,
or **Save Again** once it has been saved. The Save icon is hidden for a
watch's own take, which saves itself at Stop. Beside them are the take's
length ("12:48"), live while listening and the whole take for one
opened, and its state under it (Listening, Stopped, Opened, or 0:00 and
Not Started). The status bar no longer gives the length. Under them
**Average Over** and its pop-up ("10 s"), the time the readings are
fitted over. **Rate**, **Amplitude** and **Beat Error** have no
controls: each is its figure.

**Calibre** says which watch is on the microphone, and holds what the
readings are worked out for, in one place. Its title is the **Watch
line**: the watch picked, or **No Watch (Quick Listen)**, with ⏷. It is
the one place on the Bench that says which watch a take goes into. Its
menu has **No Watch (Quick Listen)**, then under **Recent** up to five
watches, the one with the newest take first, then **New Watch…** and
**Find in Watches…**. Another watch can't be picked while listening. The
card is still the Calibre card: its hover and its **?** say so. With No
Watch, the **Calibre** pop-up is on its figure row
(its search covers the whole table, with **Manage Calibres…** at its
foot; see Calibre, below), and under it **Lift**, the lift angle, one
field typed in or picked from the common ones with its ▾ (52° by
default), then the beat rate's pop-up (**Auto**, found from the beats and
given beside it, "Auto 28800", or a standard rate, "21600 bph"). A calibre picked in Quick
Listen sets the beat rate, and the lift angle where the table has one,
which then show as text; Not Set puts back Auto and 52°. A calibre
picked that beats at another rate than the one heard says so in a chip
("Peacock SL1258 is 28800 bph"). With a watch whose calibre the table
knows, the card names the calibre (as "Peacock SL1258 is 28800 bph" when
the watch is heard at another rate), with the beat rate and lift angle
it sets as text (Edit… on the watch changes them), the calibre in the
figure's size. A watch with no calibre the table knows reads "No
Calibre" there, its Lift and beat rate set on the card, and what is set
there is saved to the watch, as Edit Watch would ("Saved to Seiko 6R15
Diver." in their hovers). Lines too long for the card end in "…", with
the whole sentence on hover.

**Narrower windows.** Current Position, Listen and Calibre keep their
full width first, since Rate, Amplitude and Beat Error need room only
for their figures, which take a smaller form (the ± over the unit
beside them) where they are narrower. Then Listen, Current Position and
Calibre take compact forms in turn, down to the 800-point minimum, the
reading cards always keeping room for their captions whole. Compact,
Current Position is captioned **Position**, with a smaller picture, the
position's two-letter code beside it ("CR"; the name is in its hover),
and under it one pop-up, **Set**, of the six positions (with their keys)
instead of the buttons (none while a take is on show or a test runs); keys 1 to 6 still work, and the bars aren't shown. Listen keeps
its two icons, gives the take's length beside its caption ("Listen
12:48"; on the narrowest windows in the Listen icon's hover instead),
and shows Average Over's pop-up as **Avg 10 s**. Compact, Calibre keeps
its pop-up with the lift and beat rate under it; on the narrowest
windows the lift alone, since nothing finds the lift while Auto finds the
beat rate, with the beat rates heading the Calibre menu. For a watch
with no calibre they head the title's menu instead, under **Beat Rate
(hidden at this width)**: the rate set, Auto and Other Rates, a change
saved to the watch. On the narrowest windows the title is a step
smaller, a long name ends in "…", and No Watch (Quick Listen) reads
**No Watch**.

**Quick Listen**, from the Listen icon on the Listen card, listens on the
microphone without picking or adding a watch first, for a watch handed
over the counter: it clears the take's name, shows **Unsorted Takes** in the
Watches view and starts a new session. It asks first only about what
isn't Quick Listen's: changed masks on a recording opened, or a watch's
unsaved details. One icon starts and stops it; there
is no pause, and listening again starts a new take. Its take is not kept
until it is saved: once it has stopped, **Keep in a Watch** saves it into
the watch picked from the Save menu (or **New Watch…** there adds one
first, and the take is kept in it when the form's button is pressed),
with the watch's calibre, lift angle and beat rate (a beat rate heard
that differs from the watch's is kept as heard; a take kept in a watch
without a calibre, by an older app, opens with the watch's),
**Save in a Folder…** saves it into a folder chosen, and **Save…** in the
File menu saves it into Unsorted, named as a watch's takes are. Once
kept, Stop or Save Again adds the sound since to the same take. Until it
is kept, its sound goes without a question when you quit, start another
take or open a recording, and is not kept for the next start to offer
back. Starting a take of a watch ends Quick Listen.

**The Watches view** fills the window below the switch, with its
toolbar on the switch's row. From 900 points wide, the list of watches
is a 280-point column at the left and the watch picked sits beside it,
its side growing to about 1100 points and no wider, so each figure stays
near its date; only the watch's side scrolls. Narrower, one shows at a
time: the list, or the watch with **‹ All Watches** (a back chevron and its words) in the toolbar, which
goes back to the list with that watch still marked and the search kept.
The list has **Unsorted Takes**, always first, then one row per watch
(see The Watches folder, below): its first photo (by name; a tile with
its initial while it has none, or one that can't be read), its name, its
calibre (or, with **List Shows** in the toolbar, one of the fields added
to the watches, such as Customer; the choice is kept between runs, and
shows the calibre while no watch has that field), how many takes it has and the date of the last, and the space
they use once it is 1 MB or more. The row picked is marked. While a search is typed, a line under the
list says how many it shows of how many, "1 of 4 watches", and what it
found in each row's name and the line under it is in the accent blue.
Picking a watch while a Quick Listen is listening asks about its unsaved
sound first, as the Watch line does, then stops the listening and picks
the watch; while a watch's take is listening, picking another is
refused until it stops. Its toolbar has a search (a loupe; in the
name, calibre, notes and fields), **Sort** by Name, Last Take or Size,
**List Shows**, and **New Watch…** (a watch and a plus). In a narrow window, Sort and List Shows fold
into the "…" menu first; New Watch… and the search always stay
in the row. The Watches folder's location, and Show in Finder for it, are
in Settings. Clicking **Unsorted Takes** goes back to Quick Listen: no
watch, no calibre, the default lift angle and the beat rate found from
the beats (a watch's take on show is put away first; other readings on
show keep the settings they were measured with, and these apply from
the next Listen), and shows the
takes Quick Listen kept. Those takes have the same menu as a watch's, with **File Under a Watch** (into
the watch picked from it) in place of Move to Another Watch, so a take
waits there until it is filed or deleted. No watch can be called
Unsorted. **New Watch…** opens a form:
only the **Name** is needed. Name, Calibre, Lift Angle, Beat Rate and
Notes are always in the form, set or not. Lift Angle and Beat Rate are
either the calibre's or yours:

- **With a calibre**, each figure the calibre table gives is shown ("50°
  from the calibre") and can't be changed.
- **A figure the table doesn't give**, or **with no calibre**, is set in
  the form: the lift angle as a number (52° until set), the beat rate
  from a menu that starts at Auto.

**Add Field** adds a suggested field or one of your own, and
only those can be removed, with their ×. A watch is kept or deleted:
there is no hiding one (a watch an older version marked Archived is
listed like any other, and saving it clears the mark). The first watch makes the Watches folder.
The watch shown has a header: its first photo or initial, its name, and
under it its calibre, beat rate, lift angle and added fields, with
**Edit…** and **Add Photos…** at the right. Then, in this order:

- **New Take**: **Listen** (the blue one, the default), **Positions
  Test** and **Power Reserve Test**, three equal buttons, each with a
  line under it saying what it does. Each switches to the Bench and
  starts a new take on the microphone there, asked about first when
  there is unsaved sound other than a Quick Listen take's, and saved into
  the watch when it stops. While a take runs on the Bench, the three are
  greyed out, and the New Take row says "Stop the take on the Bench
  first." at its right; that row is always there, so nothing moves.
- **Takes**, with the count, and **Open** and **…** at the right of its
  title, acting on the take picked; then the takes as a table, newest
  first: Started, Position (such as DU, or DU, CU, or All 6; Power Reserve for that
  test), Length, Rate (s/d), Amplitude (°), Beat Error (ms, its size) and
  Size (MB), as measured when saved (the rate corrected from the take's
  clock log, as the Bench reads the take), with **Analysis Only**, **Incomplete**
  or **On Show** after the date when it is one. Nothing is inserted
  between rows. Clicking a row picks it; double-clicking opens it as
  Open does, and the take on show is not read again. Open is greyed out
  while a take runs on the Bench.
- **Details**: the fields added to the watch and its notes.
- **Photos**: a strip of thumbnails (clicking one opens it in the
  computer's own viewer; right-clicking offers **Open**, **Show in
  Finder** and **Delete…**, asking first; one that can't be read shows
  its name and "Can't be shown" instead).

Right-clicking a take, or **…**, offers **Open**,
**Show in Finder**,
**Keep Analysis Only…**, **Move to Another Watch** and **Delete…** (asking
first, with its size; it can't be undone; deleting the take on show
leaves the window empty, as at the start). Keep Analysis Only… asks
first, saying the take's size and its sound's and what is lost: the take
can't be measured again without its sound, and the Scope doesn't
work on it, while the readings, charts, Steadiness and the Positions Test
still do. It then reads the sound once more in the background, with
progress and **Cancel** under the table, writes the analysis beside the
take, checks it reads back, and only then puts it in the take's place
and deletes the sound; `session.toml` gains `audio = false` and keeps
everything else it says. It works on a take whose `session.toml` names
the whole folder in each recording entry, as the app's own takes do; a
take that lists its sound file by file is refused with a message saying
so. A take Save Again saves into, one being analysed, or one
whose analysis is being kept is not renamed, moved or deleted until that
is done. **Add Photos…** copies the photos picked (JPEG, PNG, TIFF, or HEIC as
an iPhone takes them, each kept as it is) into the watch's `photos/`,
adding a number to a name already there. Thumbnails are read in the
background, turned the way the camera says, and kept in this computer's
cache folder (never in the Watches folder), so the next start shows them at
once.
Picking a watch, refused while listening, sets the take's name, calibre,
lift angle and beat rate from it; changing them on the readings' cards
changes that take, not the watch. With a watch picked, **Stop** saves
the take into it, as **Save…** does, with no question, in a folder named by when the take
started (in this computer's time) and its position when it used only one;
Save Again then saves into the same take, and Save As… still asks.
Watches another copy of the app has open are read only: New Watch…, Edit…, Add Photos… and a photo's Delete… are
off, the Listen icon says Quick Listen can't keep its take, and Save…
asks where. **New Session**, in the File menu, clears what is shown,
and lets go of a recording opened, listening or not (listening carries
on into the new session, unless closing the kept sound fails). Picking or
adding a watch while readings are on show leaves them as they were
measured: the watch's details apply to its next take. **Keep in a
Watch**, for a recording opened from outside the Watches folder (a lone
file or a take folder), copies it into one of your watches, or into a
**New Watch…**, kept in the new watch when you press its button, asking
only what neither its files, the watch nor the take on show say (its
position, beat rate and lift angle are the ones on show, written with
it; when it started is offered to check); the original stays where it
is. **Keep** stays greyed out until what is asked is given.

**Open Recording…** or **Open Folder…** in the File menu first asks where
the recording goes (when there are watches and it isn't in them already):
**Add to a Watch** copies it into the watch picked in the question, or a
**New Watch…** named there, as a take of its own, and opens it there; **Just
Open It** opens it where it is, not in your watches (to add it later, open
it again). Adding asks only for what neither its files nor the watch say:
**Beat Rate** and **Lift Angle** (none when the watch, or its calibre, sets
them), **Position** (none when its session file or its file name gives it)
and **Started**, the minute it started in this computer's time (none when
its session.json says; offered from when its file was last written, less its
length). They are written into its session files, keeping the rest of a
session.toml as it was, comments too; a recording with no session file gets
one, as a take recorded here would have, named by when it started.

Opening a recording, by **Open Recording…** or **Open Folder…** in the File menu,
a take's **Open** in the Watches view, or the command line, analyses it whole,
all at once, which is how a watchmaker looks back through a take.
Dropping a file or folder on the window opens it too where the desktop
passes drops to the app (Windows, macOS and X11; not Wayland).

When viewing a recording analysed as a whole, known jumps in its beat log also
bound each reading. Saved position and settling boundaries add to those cuts;
they do not replace them. Earlier and later pieces remain available to inspect.

Every other pane's own settings sit in a toolbar across the top of the
pane, one row, with the pane's **?** at its right. The toolbar is a row
of icons, each naming itself and saying what it does on hover, in the
words this guide uses: a switch is an icon that stays pressed, tinted,
while it is on; a choice between two settings is one icon showing the
setting in use, and a click changes to the other (the Paper Strip's
Direction is the exception: a labelled two-part control, **Down |
Across**); a choice of more is a row of icons with the picked one tinted.
Icons are for what can be clicked: a setting that holds a value (Width,
Length, Settle, Span, Beats) shows its name in words before
its field, with the same hover; a search box keeps the magnifying glass
beside it. **Start Test**, **Start a New Test** and **Copy** keep their words
beside their icons. When the pane is too narrow for every icon, the last
ones fold into a **…** menu at the row's end, in words, as a Mac toolbar
does, and come back out when the pane is wide enough again; the pane's
plot never gives up room for its tools, and with everything in the row
there is no **…**. Every
pane but the Watches and Power Reserve panes has **Export…** (an arrow rising
out of an open box, as Share is drawn on a Mac) in its toolbar, which saves the pane as a picture: a PNG of it as it is on the
screen, or, with a name ending `.svg`, an SVG drawing whose lines and
text stay sharp at any size. A name with no ending is saved as a PNG.
In the picture the pane's name (with its own averaging time, if it has
one) and the charts' key take the place of its controls. The save starts
in the folder last chosen for a picture, else in the folder of the
recording last opened; the app forgets both when it closes. Titles and
labels are in Title Case. Across the top, the readings (above), and below
them the panes. Along the bottom, the
status line. Every card, pane and reading has a **?** beside its name: resting
the pointer on it shows the plain answer in a line or two, and clicking it
opens the whole explanation in place (click it again, or anywhere else, to
close it). Every setting gives a short hint when the pointer rests on it. Before anything is
open, the Paper Strip says where to start: **Listen**, on the Listen card,
listens without picking a watch, and Save keeps what it heard; for a
watch's history, **Watches** at the top lists your watches, to pick one
and then Listen, Positions Test or Power Reserve Test, each take saved
into it, or to add one with New Watch…; and recordings and folders open
from the File menu, analysed all at once. While a recording is being
analysed the Paper Strip shows how far the analysis has got, in hours of sound and time
left, with **Cancel**; a problem shows in the status bar behind a red
failure icon, its
whole message and Dismiss a click away, so nothing in the window moves. A recording that can't be opened leaves the one
shown in place. The app follows the
desktop's light or dark setting (Appearance, in Settings, overrides it),
and uses the Inter typeface with figures of even width, so readings don't
shift as they change.

**A folder of segments.** A long take saved as segments (the recorder's
hourly files, or an overnight run split into 10-minute FLACs) opens as one
recording: **Open Folder…** in the File menu, or the folder's path given to the app
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
can be spotted. A 16-hour night takes some minutes to analyse; the
window shows the hours done, the time left and the files in order, and
Steadiness then covers the whole night.

**Readings.** The **Calibre** card gives the beats per hour the
readings use: found from the beats while its beat rate is Auto ("Auto
28800"), or the one set there or by the calibre. Rate (seconds per day gained, +, or lost, −), amplitude (degrees) and beat error (ms), each a
fit over the last few seconds of beats (10 s by default, or 2, 4, 20, 30
or 60 s; the setting is Average Over, on the Listen card). Early in a session, or after a break in listening or a gap of
more than 2 s with no beats, there are fewer seconds of beats than asked
for: the readings use what there is and say so in the rate's hover
("190 beats in 24 s of 180 s"); they never fit across a break or gap, since the watch may have
been moved or have drifted meanwhile. Average Over is one setting for the
whole app: the panes that average (the Paper Strip's Rate Line, guides
and overlays, the Tick Tock Profile, Charts and Distributions) all
average over it, and have no time of their own. The big amplitude is the average of the two
swings, in whole degrees, with a ± figure: the standard error of the
median of its 2 s windows, estimated from successive window differences
and widened for the windows' lag-one correlation.
It is model-based precision, not accuracy: a wrong lift angle moves
every amplitude together (1° of lift angle moves it about 4°).
Its hover gives the amplitude from the ticks and from the tocks. The sound can't tell which beat is
which pallet, so the first beat heard is called the tick;
which side is the entry pallet can't be told from the sound, so the sign
of the beat error follows which beat came first, as on tg. The big beat
error is timed from the unlock, as tg measures it (checked against tg
in the regression tests); its hover gives the beat error timed from the beat as a whole,
nearer the drop, which is the gap between the two lines on the strip.
Beside the rate, when it is at least 0.05 s/d, is its ± figure: the
estimated standard error from short-window rates, scaled to the averaging
time. It is not the covariance error of the fitted beat-time slope.
The rate's hover gives how many beats the readings used and
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
break or a move, up to five minutes), scaled to the averaging time; it
needs about 30 s of unbroken beats. What the ± can't show is named, as a
plain fact, in a chip beside the reading's caption, with its sentence on
hover (a click keeps it open, as the ? does; with several, the chip
shows the first and how many more, "12 s of Beats +1", and the hover
lists them all): "12 s of beats" when the beats span under 10 s, "540 of 600
beats" when under 95% of the beats expected were found, "3% clipped"
when over 1% of beats clip (amplitude and beat error), a beat rate set that differs from the one heard, and breaks
in listening in the stretch (such as the computer sleeping). Chips are in Title Case ("3% Clipped"). When no calibre or lift angle is set, "Default"
beside Lift on the Amplitude card says the amplitude comes from the
default 52° (on a compact card, the field's hover says it). The cards give their units short:
s/d and ms. On the beat error, a reading of an opened file whose Tick
and Tock were named by the beat count across a break says so ("sides
named by count", with the number of breaks in the hover). A masked
stretch in the averaging time reads "6.8 s masked". Once stopped, each figure's hover also gives the whole take in the
position the watch is in, settling left out, as `timegrapher session`
reads it (core `session::measure_masked`, the time in other positions
masked): the rate fitted over every beat, the amplitude and beat error
the median of the 2 s windows, each with its ±, and the 5th to 95th
percentile of the 10 s rates and of the 2 s amplitudes. Where the beat
error's sign differs either side of a cut (a break, a jump, a move or
its settling) in that stretch, it combines the sizes either side, as
`session` does, and says the "whole take is given as a size: its sign
differs either side of a cut".
No reading is graded. That the input's clock hasn't been measured is
said in the Rate card's **?** rather than on the card, with how to
measure it. When the beats found don't come at regular intervals (their
timing scatter is 2% of a beat or more, `beats::watch_heard`, as
`analyze` uses), the cards show no figures, only **No regular beat found
(timing scatter 8.6 ms, 5.2% of a beat)**. When nothing in the take has
had a regular beat after its first few seconds, the strip shows the
sounds that might be beats as faint dots, under the line **No regular
beat found yet. The faint dots are sounds that might be beats.** Nothing
is measured from them; the readings and charts appear once a regular
beat is found. With nothing heard at all, one card says so in the
strip's place.

**Long takes.** Listening carries on while the window is hidden behind
other windows, minimised, or with the screen locked or switched off: the
sound is saved and read as it comes in, on a thread of its own, so the
window shows everything heard as soon as it is back. The exceptions are a
file dialog (Open, Save, choosing a folder) and Save, Save Again or Save
As copying the take: meanwhile the sound waits in memory, and is saved and
read once they finish. With **Keep Awake** on (in Settings, under
General; it is on unless switched off), the app asks the computer not to go to sleep on its own while it listens;
the screen can still lock and switch off as usual. If the system refuses,
Keep Awake's caption says so in its place. When the sound's own timestamps jump by
more than 5 seconds (as when the computer sleeps: a laptop's lid closed,
Sleep chosen from a menu, or Keep Awake off), the app leaves the jump out
of the take as a break, keeps listening, and a
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
jump against up to 32 before, so they keep their names; a whole
recording's reading makes the same check on the same beats. Until they are,
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

**Positions Test.** The Positions Test pane, a hidden tab beside the Tick
Tock Profile to begin with (its icon is a die showing six, one face for
each position), leads through the six positions in the app's order: Dial Up,
Dial Down, Crown Down, Crown Left, Crown Up, Crown Right. Its toolbar
has **Start Test** (**Start a New Test** once one has finished), **Load
Full-Wind Results…**, **Settle** and **Each Position** (the seconds the watch
settles after each turn and is then measured: 20 s and 60 s unless set
otherwise, kept between runs, 5 to 600 s and 20 to 3600 s; each is a
field to type in with a menu of common times beside it, 10, 20, 30 and
60 s for Settle and 30 s to 5 min for Each Position, which only the
guided test uses), and **Save
as CSV…**, **Copy** and, after a test just after winding, **Save
Results for 24 Hours…**. The pane can be shown with nothing listened
to yet, so a test can start from it.
Start Test starts listening if need be and brings the pane to the front.
A banner at the top of the pane names the position and shows a progress
bar while the watch settles and is measured. It then says which position
to turn the watch to; press the button naming it (**Start CL
Measurement**) once it is there. **Restart DD Measurement**, naming the
position just measured (or the one under way), settles and measures it
again, after a knock for instance; **Pause** holds the test, leaving out
what the position measured so far, and **Resume** settles and measures it
again from the start; **Stop** ends the test and clears its table. Each
button keeps its own place, as wide as the widest it holds, so none moves
as the test moves on: Stop is always where it was. Only
the measured seconds in each position count. From the moment the test
starts, the strip, its track and the charts mark the test's
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

**The table for every take.** With no guided test, the pane shows a
table for the take on show or under way, filling in as the Current
Position card is pressed, so a free take becomes a partial or full
positions test:

- **Columns:** Position, Duration, Rate (magenta), Amplitude (violet) and
  Beat Error (amber).
- **Duration** is the time marked in the position, less the Settle at
  the start of each stretch and less masked time.
- **Readings** show once a position has 10 s. Under that it reads
  "Under 10 s"; a position never marked reads "Not marked".
- **Average** and **Largest Difference** follow once two positions
  have readings: the average rate, amplitude and beat error over the
  positions read (with "n of 6" in Duration), then the highest of each
  less the lowest.
- The figures are core `session::positions_table`'s: each position's
  stretches read together, the time between them left out as a mask is.
  While listening the table is read again every second, in the
  background, the last table staying on show until the next is ready.

Under the banner, the table fills in as each position is measured, read
with core `session::measure` as `session` does (rate on the input's
measured clock when there is one): each position by name ("Crown Up
(CU, 3H)"), its rate, its rate less the mean of the positions measured,
its amplitude and its beat error (a size); the position under way says
it is settling, then "Measuring…" until it has 10 s, then shows its
figures so far, in the accent colour, read again every second as This
Take is. Then an Average row, a Largest Difference row (rate, D;
amplitude; beat error), vertical less horizontal for rate and
amplitude (DVH), all from core `session::indices`, the values
`session` gives (a position read twice is averaged first), and a plain line for each condition found in a
position's sound: no regular beat (with its timing scatter), clipping,
under 10 s of beats, a beat error given as a size because its sign
differs either side of a cut, or Tick and Tock named by the beat count
across a break rather than by their sound. After the sixth, **Finish**
reads the six together. The figures are `session`'s for the same
stretches. **Save as
CSV…** saves the watch's name, its calibre, beat rate and lift angle,
when the test began and its settling and measuring times, then the
table, the averages, the largest differences and those lines, as a CSV file a
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

**Power Reserve.** The Power Reserve pane, a hidden tab beside the
Positions Test to begin with (its icon a fuel gauge near empty), runs the
watch down from full wind to stop in one position, the one on the
Current Position card. Its toolbar says which ("Runs in Dial Up (DU), set
on the Current Position card"; it has no position setting of its own),
then holds **Keep Sound** or **Analysis Only**, and **Start Test**. Before a test the pane says how much space is free where
the take will be kept against the most an 80 h run writes (the sample
rate times the bytes a sample times 3,600 a hour: 0.35 GB at 48 kHz,
16-bit). Start Test starts a new session in the card's position as it
stands; while it runs the computer is kept awake whatever Keep Awake is
set to, the position can't change (the card's other positions are greyed,
their hover saying why), and the **Stop** icon on the Listen card, or **Stop Test**
in the pane, ends the test. The run ends at the last beat heard before 10 minutes with none
(a beat heard is one matching the beat's sound with a quality over 0.4,
in a run of at least eight such beats each within one and a half beats of
the last, so a lone click is not the watch); beats that come back sooner
join the run, and each silence of 30 s or more inside it is counted as a
stop on the way. A run still going at 80 h ends there. The pane shows the
run time from the first beat heard (to now while it runs), and at the
end the run time in true time (through the input's clock as measured
while listening, else its stored correction) and how it ended, then
the rate and amplitude against hours since the first beat, and a table
of each hour (the last as long as is left): its rate, median amplitude
and beat error from the unlock, each measured as `timegrapher session`
measures a stretch (core `session::measure_masked`), and how much of it
its whole 10 s readings cover (core `clean`). **Save as CSV…** and
**Copy** give the watch, the run and the table. The take
is kept in the picked watch's history, named by its local start time and
"Power Reserve", with
`power-reserve.json` beside the standard files; with Analysis Only it is
then kept as its analysis alone, as Keep Analysis Only does, and its sound
deleted. The Watches view lists it as "Power Reserve 46 h 12 min", adding
"(still running at the end)" when it was stopped by hand or at the limit.
Every pane reads the take as any other.

`power-reserve.json` (`schema` `power-reserve/1`) holds `started_utc`,
`position`, `sound_kept`, `ending` (`stopped`, `by_hand` or `limit`),
`first_beat_s` and `last_beat_s` (audio seconds of the take), `run_s`
(true seconds from the first to the last), `pauses[]` (`from_s`, `to_s`:
the silences of 30 s or more inside the run) and `hours[]`: `hour` (0,
1, 2, ...), `span_s` (true seconds, 3,600 but for the last),
`rate_s_per_day`, `amplitude_deg`, `beat_error_ms` (signed, from the
unlock) and `read_s` (seconds of whole 10 s readings), each value null
when not read.

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
known to half a ppm, Settings, under Microphone's **Quartz
Correction**, shows the error it timed (with how much has been heard
until then) with a **Keep** button if no error is kept for that input yet, or if the one kept
is more than 2 ppm from it; then the line also gives how far apart they
are and the day the kept one was measured, since an input reached by a
shared name (the system default, say) may be another sound card. A busy
computer delivers sound less evenly, so the offer can come later. The
timing is only right when the computer's clock is kept by internet time
(NTP). When the microphone's own clock error has been kept (by **Keep**,
`timegrapher clock measure --save`, or `clock from-log --save` after a
take with a clock log; see `clock` in [the agent
interface](agent-interface.md#clock)), Quartz Correction lists it, and
the Rate figure's hover gives the rate corrected onto true time,
labelled True Clock, with the day the clock it uses was kept. A sound card's crystal runs a steady 10 to 50 ppm off,
which moves every rate read on it by 1 to 4 s/d. While listening, the big
figure stays on the card's clock, the same clock tg and `timegrapher analyze`
read, so it compares directly with them; True Clock is in its hover,
never instead of it. It is looked up by the input's name when a microphone
session starts. A take opened from disk whose session file names a clock
log (every take the app keeps has `clock.csv`) is read on that log
instead, fitted as `timegrapher session` fits it: the Positions Test
tables, the readings worked out over the whole take, the Rate figure and
the Charts are all corrected for that take's own sound card, so one take
has one rate and it is what the command line reads. The Rate's hover
says the figure is on the take's clock log, and by how many ppm. A log
that can't be read is named in the line shown on opening, and the take
is read as heard. A recording opened without a log shows no True Clock. **Settings**,
under Microphone's **Quartz Correction**, lists each input's kept clock, its own (its error in ppm
and in s/d, the day it was measured, how, and over how long), with
**Forget** beside it; the input listening is marked "(listening)", or
named with "None kept" when it has none. Forget first asks, naming the input, its error and
the day it was measured, since getting it back takes 20 minutes or more of
listening; then it removes it from the same `clocks.json` that
`timegrapher clock list` and `clock forget` read, so the command line and
the MCP server forget it too; rates from that input are then read on its
own clock again, and True Clock goes from the hover, until it is timed and kept
afresh. Do this after changing the microphone or sound card behind an
input's name.

**Clipping.** Whenever a beat in the readings has a clipped sample in
its sound, counted as `timegrapher analyze` counts them, the Amplitude
and Beat Error cards say so in a chip, **3% Clipped**, its hover saying
why it matters. Clipping flattens the loudest part of each tick, which
moves the drop and so the amplitude and the beat error; the rate is
unaffected. The chip follows the readings: it shows while the beats
being averaged clip and goes when they no longer do. The peak meter, in
Settings under Microphone, says "clipping" on any clipped sample at once; the
chip counts the beats in the readings. Even a watch driven several times
past full scale is still read, with the chip, rather than taken for
silence: when most of the beats found clip, the flattened peaks no
longer count against the signal.

**Panes.** Below the readings, to begin with, are the paper strip (with
Rate Line and Parallel Guides on) and under it the tick tock profile,
with the Scope as a tab behind it. Every other pane, the Watches view
included, starts hidden, and its icon at the top or the View menu shows
it: the charts of rate, amplitude and beat error over the session, the
distributions, the Steadiness views, Isochronism, the Positions Test and
Power Reserve. Charts and Distributions open as tabs behind the profile,
so showing them never takes room from the strip. No pane on show is laid
out shorter than 140 pt (240 pt wide side by side), and Charts and
Distributions are kept tall enough for their toolbar and one plot at a
readable height (about 60 pt of plot for a distribution): the room comes
from the panes with some to spare, and a window too small for them all
shares its room in proportion. A
layout saved by an earlier version of the app is set aside once for this
starting one. Drag a pane by its tab
to put it beside, above or below another, or onto another's tab to stack
them; drag the gaps between panes to resize them. The selected tab takes
the card's colour, joined to the card below it. The × on a tab hides
that pane (the status line says it comes back from its icon at the top or
the View menu), and the icons and the View menu hide or show each pane
and the readings across the top; a hidden pane comes back where it was,
and changing the strip's direction keeps the choice. **Reset Panes**
(in the View menu) puts the panes back as they start, with the readings
shown. Periodicity views will join
them.

**Tick Tock Profile.** The typical sound of the ticks (blue) and of
the tocks (deep orange) over the averaging time (Average Over, up to 60 s, so
it describes the same beats as the readings; a longer time gives a
steadier shape, and after lengthening it the beat count fills in as beats
arrive), from the drop's side of the beat back
through the unlock, with a shaded band where the middle 80% of beats fall
and the marks the engine read, named outside the plot: a line from the
top is an edge, a dot on the curve is a sound. The edges (Unlock, Drop and
Peak, where amplitude and the unlock-based beat error come from) are solid
grey lines coming down from their names in bold above the plot, each
stopping just short of the trace. The three sounds (Unlock Sound, Impulse
Sound and Drop Sound, where the engine tells them apart) are white dots on
the trace where each sound has risen halfway up its own climb, each with a
dotted leader straight down past the plot to its name in grey italics
below it. Names never sit inside the plot: where two would come closer
than 12 points (more than a word space, so two names never read as one
phrase), the later one moves one row further out and its line runs on to it, and
where that row is taken too it slides along with a short angled leader.
A name changes row only once an overlap starts or ends by 2 points, so
names don't hop as the sound drifts. Hovering a line, a dot or a name
gives its name and time. The edges and the sounds are two different
measurements of the same averaged sound, not single beats, so they sit
near but not exactly on each other. A sound is missing when it can't be
told apart from its neighbour (a soft unlock merged into the impulse,
say). Each plot carries its side's facts in its top-left corner, over
the quiet lead-in before the unlock: Beats, Unlock, Drop, Unlock to Drop
and Amplitude, a row each, the values with their units. The box is the
same on both plots, so Tick and Tock compare row against row, and it
sits under the band and the line, so a spike that runs into it still
shows. On a plot under 100 points tall it keeps Unlock to Drop and
Amplitude (fewer if even those don't fit), and hovering it gives all
five. A plot that would be under 100 points with its names outside
takes them in: the edges' names sit along its top on the plot's
background, one row, under the trace (Unlock starts right of its line
where the facts box has the corner, and a name that would crowd the one
before, usually Peak beside Drop, is left to its line's hover); the
sounds keep their dots, and hovering one names it. Stacked, the one
time axis is under Tock. The moment shown ("At
10:00") is at the right end of the toolbar. The noise floor is a flat
dashed line. The Tick tock
profile card switches the edges and the sounds on and off, switches
**dB Scale** (lit: decibels below the loudest point; off: linear)
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
below each side's loudest point, so the quiet unlock shows more clearly. Stopped, the pane shows the sound the session
kept nearest the end (one every 2 s); for a recording analysed
all at once it is worked out from the file.
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
(a drum and its sticks)
runs from 15 ms before one beat to 15 ms before the next, at the
watch's beat rate (125 ms at 28,800 bph, 200 ms at 18,000). **200 ms** shows one
row each from 20 ms before the beat to 180 ms after, so the next beat
shows too. **400 ms** shows every beat in one grey row, three or four
beats at once. **Beats** draws the latest 1, 4, 8 or 16 over each other,
the older ones fainter, so a beat that differs from the rest stands out.
Each row is scaled to its own loudest beat, so the Tick and Tock rows
compare in shape, not in size. While listening it shows the latest beats; stopped, or on a
recording, it shows the last beats, read from the recording. Only the few seconds around the beats are read, however long
the recording: a WAV seeks there and a FLAC is searched by its frame
headers (`span.rs`). No crosshair, as on the other sound plots.

**Paper Strip.** Its toolbar runs, left to right: Rate, Amplitude and Beat
Error (Draw Over the Strip, below); Width and Length, each one field with
its presets under its ▾, then Fit; Centre and Auto-Centre; Rate Line and
Parallel Guides; the Mask tool (scissors) and the Position Brush;
Direction; and Export. One dot per beat, tick and tock in two colours. Across
the strip is how early or late each beat came against a perfect clock
beating exactly at the nominal beat rate (every 125 ms at 28,800 bph): a
beat before that clock's beat is early, after it late, the words Witschi's
training course uses (doc. 71.1010D35e p. 6). Along it is time; both axes
are labelled, in ms across and in minutes and seconds along, and every
grid line is drawn alike. **Rate Line** (on by default) draws the Rate
reading as a thin solid magenta line over the beats it was fitted to, on top
of them, as tg does: if the dots follow it, the reading describes them
well. **Parallel Guides** (on by default) adds faint magenta lines at the same
slope across the whole strip, one per grid step, since the eye judges
whether lines are parallel far better than it judges a slope; dots that
bend away from the guides show the rate changing. The guides show with or
without the Rate Line. Under **Draw Over the Strip**, rate (magenta), amplitude
(violet, on by default) and beat error (amber) readings, in that order
as everywhere in the app, can be drawn
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
`1:30:00`) or pick a common value from the ▾ inside each field: the widths
run from ±1 ms, for a good watch's beat error, through ±62.5 ms, half a
beat at 28,800 bph and the widest view in which tick and tock can't wrap
onto each other, up to ±60 s, so a long run-down take that drifts by many
seconds can be shown without wrapping; the lengths run from 10 s of beats
to a two-hour run.

**Fit**, the switch beside Length (arrows out to two edges, lit while
on), shows the whole session so far along the strip, up to its latest
beat (its last 24 hours when it is longer):

- While it is on, the length keeps growing with the take as beats arrive.
- Zooming the strip's time (the wheel, or a length typed or picked) or
  looking back through it (dragging, or a click on a chart) turns it off.
  Changing the width leaves it on.
- Opening a take turns it on and sets the width to the narrowest of the
  listed widths that holds all its beats without wrapping (leaving out
  one stray dot in a thousand at each end), so the whole take shows at
  once. Listen then brings back the width, length and Fit the strip had
  before, unless they were changed by hand meanwhile.

On the strip, the mouse wheel changes the length about the time under the
pointer, which stays where it is, Shift or Ctrl and the wheel (or a
pinch) changes the width, dragging along the time axis looks back through
the session (live or not), dragging across slides the trace, and a double
click returns to the newest beats and centres them. **Auto-Centre** (a
pushpin on a dashed line, lit while on) keeps
the newest beats on the centre line all the time, so the older ones slide
away from it as the rate wanders; sliding the trace by hand turns it off.
**Centre** (two arrows meeting on a dashed line) centres once.

**Mask** (scissors: nothing is deleted, the beats stay, hatched) leaves stretches out of the readings, such as a bump of the
stand in a long take: with it on, dragging along the strip over the beats
masks that stretch (one under half a second is left alone, as a slip of
the mouse), dragging a masked stretch's edge moves it (the pointer is a
cross over the strip and a two-way arrow within 8 points of an edge),
dragging with the right button still looks back through the session, and clicking a
masked stretch, or right-clicking one with Mask on or off, brings it
back; **Edit ▸ Remove All Masks…** brings them all back, after asking. Stretches that overlap, by
masking or by moving an edge, join into one and stay joined. Masked stretches are shaded on the strip and named on its track,
and the Mask tool's hover says how many there are and how long they add up to.
They are cut like a break in listening, at both ends: no reading reaches across one,
nothing is read inside one, a chart's line breaks there and comes back
once a whole averaging time has been heard after it, a beat in each 2
seconds of it (as at the start of a session and after a break; each
point on a line is the reading over the averaging time that ends there,
which is why the line is blank just after one), the steadiness tests
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
take is opened. Masks on a recording opened from disk are
kept when it is saved: a mask change marks it unsaved, and **Save** in
the File menu (Ctrl+S, Cmd+S on a Mac) writes them, and New Session, opening
another recording, a take of a watch, the Listen icon on the Listen card,
the Positions Test pane's Start Test, or quitting asks first when they've
changed; they stay on show until
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

**Position Brush** (the highlighter, the brush's position code in teal,
then ▾) marks a stretch of the strip as one position, on a live take or
one opened:

- Pick a position from its menu, then drag along the strip. While
  dragging, a teal preview says the position and the times, such as
  "CD 4:30 to 5:30".
- A drag stops at the newest beat. A stretch painted up to the newest
  beat (within a second) leaves the watch in that position from then on;
  otherwise the position after the stretch carries on there.
- Painting is a correction after the fact, so it never starts a
  settling: an edge it makes starts at the fade. A change of position
  recorded (on the Current Position card, or by a Positions Test) that
  painting covers loses its settling; painting from inside a settling
  cuts it short there. Painting the position already recorded changes
  nothing.
- The positions recorded stay under the painting. A right click on a
  painted stretch in the track's lane takes that painting off and brings
  back what was recorded there, its changes and settling with it (the
  app has no Undo, so this is the way back). Hovering a painted stretch
  there says so.
- **Stop Painting** at the end of its menu, or Esc, puts the brush down.
  It and Mask turn each other off. It isn't offered during a Positions
  Test, which sets each position itself.
- Dragging a change of position along the track moves it, brush on or
  off, with its settling; it stops half a second short of the changes
  either side.
- What the brush paints is the take's position record, the one the
  Current Position card writes: the track, the charts' marks, the
  Positions Test table and saved takes all read it. Times are
  kept to a tenth of a second. A take saved with painting showing also
  lists the positions recorded under it, as `[[recorded]]` entries in
  its `session.toml`, so Remove Painting works after it is opened again;
  readings, here and on the command line, take the positions shown.
- On a recording opened from disk it is saved as masks are: a change
  marks it unsaved, and **Save** writes one entry per stretch in place of
  the entries for what was opened, each keeping everything else the first
  of them said (each later position starting after its settling, with
  `settle_s = 0`, as a saved take's do), and the positions recorded
  under any painting. It is refused where those entries list more than
  one file.

**The track** runs along the top of the strip (down its right side when
the strip runs down):

- Each stretch in one position is teal, named at its start ("Crown Up
  (CU, 3H)"), or by its code where the name doesn't fit, or not at all.
- It is solid through the settling after each change recorded, then
  fades over 40 s and stays faint, so each change shows; a thin dark
  line marks each change. The solid band says **Settling** where it is
  64 pt wide or more, **STL** from 28 pt, **S** from 12 pt, and nothing
  narrower, in dark text on the teal; the position's name starts after
  it. Hovering the band always says "Settling" and how long. Changing
  **Settle** on the Positions Test pane redraws every band and the
  table. A change painted with the Position Brush starts at the fade. The plot behind the beats takes the same at a sixth of
  the strength.
- Masked stretches are grey hatching over it, named "Masked".
- Each name sits on a small plate of the card's colour, in the text
  colour, so it reads over solid teal, faded teal or hatching.
- During and after a Positions Test, the time before the test and the
  time it didn't use are left plain, with their names.
- **Tick** and **Tock** are in the corner left of it; each line drawn
  over the strip is named in its colour in the corner right of it, above
  its scale. When the strip runs down they stay above it.

**Charts.** Rate, amplitude (the average, from the ticks and from the tocks) and beat error (from the
unlock and from the drop) over the whole take, one chart above the other
in the one pane, each with its reading's name at its top left. The hare,
the swing and the seesaw in the pane's toolbar show or hide the Rate,
Amplitude and Beat Error charts; the charts shown share the pane's
height, and at least one stays on. A pane too short for every chart draws
the first ones at a readable height and names the rest at its foot ("Beat
Error not shown: no room"); the Distributions pane does the same. Each
reading keeps its colour across
the app (rate magenta, amplitude violet, beat error amber), so a second line
of the same reading is told apart by its dashes: amplitude from the ticks
dashes violet over blue and from the tocks violet over deep orange, and beat
error from the drop is a fainter dashed amber; the key and the box at the
pointer draw them the same way. The Amplitude and Beat Error charts
each carry the key to their own lines at their top right. Each chart labels its vertical scale at two marks or more,
in small text, however short the chart; a long time at the right end of
the time axis is left out rather than cut in half. The charts always
cover the whole take: to look at a stretch of it, zoom or pan them, or
use the strip with its overlays and masks. Wherever the pointer is on the
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
over the whole session, leaving out any masked stretch (mask a stretch
to narrow what they cover), one plot above the other in the one pane,
hidden at first. The hare, the swing and the seesaw in its
toolbar show or hide each reading's plot; at least one stays on. An average gives one number; these
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
**Analysis** in the icon bar (and in an Analysis menu under View), and
they start as hidden tabs beside the Tick Tock Profile. Each series shows the sentence `series` writes, saying what
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
axis, with the 1% false-alarm line dashed, the calibre's wheels' periods
marked, and the strongest component labelled; **Log
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
empty and shown as "not read" in the key under the map, which is titled
with what the colour measures and its unit (**Timing Swing (ms)** for the
rate's map, the timing's swing at each period, not a rate in s/d;
**Amplitude Swing (°)**; **Beat Error Swing (ms)**); the wheels' turns are
dotted across it and named at the right; **Log Scale** in the toolbar
colours the sizes on a log scale over a range of 300 to 1; the pointer
over a cell shows its size with its unit, its period and its time, as
"Timing Swing 1.8 ms · period 12 s · at 3:20"; the two maps show **one map at a time**, Rate first, picked with the Rate, Amplitude and Beat Error icons joined into one three-part control, so the map has the pane, and they share the choice, so switching between them keeps the same map). In Periods and FFT each wheel's
period is a bold dotted line and half of it, twice a turn as an off-centre wheel
shows, a fainter dotted line marked ×2; the grid behind is pale. The
wheels' lines are grey and dotted, and named, so they never
read as the grid or a reading. With no calibre picked, or none of its
wheels known, one minute and one hour are marked in grey instead, periods
alone named after no wheel. **Wheel Periods**, in the toolbars of
Periods, FFT, STFT Map and Wavelet Map, shows or hides these marks; it
starts on. The tests need 5 minutes
of beats. They run over the whole session in the background, again every
minute while it grows; for a recording they cover the whole file once
it is opened. Rows stack and fill the pane when it is
tall enough, sit side by side when it is wide and short, and scroll
otherwise. A line at the top says in plain words what the view
shows. The plots don't pan or zoom, and only the two maps show values under the
pointer.

**Remembered settings.** The app keeps its views between runs: the strip's
direction, width and length and what is drawn over it, the profile's
layout, scale and Time From, the panes shown and their arrangement,
whether the Readings show, the distribution settings, the averaging time,
folded groups in the Watches view, the appearance, Keep Awake, Listen
When the App Opens and whether the microphone has been set up, the last
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
start shows **Closed Unexpectedly…** in the status bar, which opens
**The timegrapher closed unexpectedly last time. Make a report?** with **Make a Report…** (copies the record and opens a new GitHub
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

**Report a Problem.** **Report a Problem…** in the Help menu
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

**Listing the inputs.** Listing the inputs asks each one what it
supports, which briefly opens it and can disturb another program recording
from the same microphone (a long take running beside the app, say). So the
app lists them only when the microphone menu opens, **Rescan** is pressed
or listening starts; until then the menu names the microphone kept from
last time, or System Default. Analysing a recording never touches an
input. Once the microphone is set up, the app starts listening as it
opens (see Sessions, below), unless Listen When the App Opens is off.

**Level meter.** In Settings under Microphone, and on the Set Up the
Microphone sheet, while listening to a microphone: a bar to
the loudest sample of the last half second, grey, over a grey band between
plain reference marks at −10 and −6 dBFS, and the peak in
dBFS beside it ("peak −14 dBFS", with ", clipping" while it clips).

**Status bar.** First what the strip shows: the take's name (Quick
Listen, New Take, or the name of the recording or saved take), its watch
(No watch for Quick Listen, No watch picked for a recording or saved
take with none) and its state
(Not Started, Listening, Stopped, Opening or Opened), such as "Quick
Listen · No watch · Listening", "New Take · Pepsi · Not Started" or
"w.wav · No watch picked · Opened". Then, while listening, the input;
the signal, "signal 12×", how far the typical
beat's peak stands above the typical level between beats (noise alone
reads about 2×, and below 4× no beats are logged). It measures
loudness, not the shape of the tick, so hum, knocks or a holder that
smears the sound aren't caught by it. Then how much sound the session
holds and whether it is saved, and the latest message. It repeats
nothing the cards show. On the right are its notices, one short word
each for what needs you: an error, behind a red failure icon; **Closed Unexpectedly…**
after a crash; **Unsaved Sound Found…** for sound kept from an earlier
session; **Timegrapher 0.2.0 Available…**. A click opens the whole
message over the window with its buttons. Nothing ever appears above the
readings, so the window never moves under the pointer.

**Updates.** With **Check for Updates at Start** on (in Settings, under
General; on unless switched off), the app asks GitHub once at start, on a
background thread, whether a newer release is published
(`timegrapher_core::update::check`). If one is, the status bar says
**Timegrapher 0.2.0 Available…**, which opens **Download**, which opens the
release's page in the browser. Nothing installs itself. At start, a
failed check or an up-to-date version says nothing. **Check for
Updates…** in the Help menu makes the same check and always says what it
found: the notice, "up to date", or why it couldn't check ("no release has
been published yet"). The request carries no account or identifier.

## Settings

**The Settings window.** Edit > Settings… (Ctrl+, or Cmd+,) or the gear at
the end of the pane icons opens it over the app. As the Mac's System
Settings does, a sidebar down its left lists the categories, each with its
icon and name, the one picked filled in the accent colour, and the picked
category's name heads its settings on the right. Each page is in
groups: a group's title in small grey type above a rounded panel no wider
than 560 points, its rows apart by hairlines, each row's name at the left
with a one-line caption under it and its control at the right of the
same row. A message about a setting (Keep Awake refused, an input's level
that can't be set, the microphone reporting its automatic gain otherwise)
takes that row's caption, in the same space, so no row grows.
**General**: Appearance (Auto, Light or Dark); **Listening**, with
**Listen When the App Opens**, on unless switched off, and **Keep
Awake**; **Updates**, with Check for Updates at Start. **Microphone**:
the **Microphone** group (the microphone menu and **Rescan**, **Show
Every Input**), **Level** (**Input Level**, **Automatic Gain** with its
warning in the caption, and **Peak** with its shaded band) and **Quartz
Correction**, each input's kept clock as a row with Forget (see True
Clock). **Watches Folder** (below) and **Calibres** (Manage Calibres,
below). It opens at the
category it was last left on, kept with the app's settings across
restarts. **Done**, at the top right, Escape or a click outside closes it. What belongs to
the take on show (its calibre, lift angle, beat rate, position and
Average Over) stays on the readings' cards, beside what it sets.

**The Watches folder.** The watches and their takes are kept in one
folder, `Timegrapher Data` in the home folder unless another is chosen:
`library.toml` at the top, then a folder for each watch holding its
details (`watch.toml`), its `photos/` and its `takes/`, and `Unsorted/`,
whose `takes/` holds the Quick Listen takes kept with Save… in the File
menu until each is filed;
each take a standard take folder named by when it started (`2026-10-20
14.32 Dial Up`, with the position in words when the take used only one).
`timegrapher watches list` and `show` read it from the command line,
without changing it ([agent-interface.md](agent-interface.md#watches)),
and `timegrapher record --watch NAME` records a test into a watch's
takes while the app doesn't have the Watches folder open
([agent-interface.md](agent-interface.md#record)). Nothing in
it records where it is, so it can be moved, synced or copied whole; a
take with sound files missing is listed as incomplete and never changed.
Settings' **Watches Folder** category shows where it is and how many watches and
takes it holds and their size, with **Show in Finder** (Explorer on
Windows, File Manager on Linux), **Move Watches…** (into a folder picked,
asking first; onto another disk it is copied, each file with its
modification time, checked and only then removed, and a link or a
special file such as a named pipe in it stops the copy, leaving the
folder as it was; while it moves, closing the window or File > Quit is
refused and the app stays open; on macOS Cmd+Q and Quit in the app menu
or the Dock do not wait, and on any system neither does logging out,
shutting down or ending the app from outside; a move cut short that way
leaves the Watches folder whole where it was, or whole at the new place once
its copy was finished, which Choose Another… then finds; a copy cut
short leaves its files at the new place without a `library.toml`, which
is not a Watches folder and can be deleted),
**Choose Another…** (a Watches folder elsewhere, or an empty folder
for a new one; nothing is moved) and, once another is chosen, **Use the
Default**. Only one copy of the app at a time can change your watches: a
second copy opens them to read only and says so in Settings, and the
folder can't be moved while another copy has it open. The Watches view adds watches to it and saves takes into them.

- **Input.** The microphone, chosen in Settings under Microphone or on
  the Set Up the Microphone sheet, or a WAV or FLAC file
  opened from the File menu. On Linux the menu offers each microphone once:
  **System default (via PipeWire)**, which is whatever input the
  desktop's sound settings choose and works alongside other programs, and
  each card's direct device, marked "(direct)", which goes straight to
  the card but can't open while the desktop sound server holds it. With a
  USB timegrapher microphone set as the desktop's input, choose the system
  default. **Show every input** lists everything the system offers, with
  ids, as `timegrapher devices` does (the switch is in Settings under
  Microphone). If the input can't be opened, stops,
  or sends no sound for 3 s, the app stops and says why above the
  readings, rather than showing an empty strip. The status bar shows the
  sample rate and bit depth the input opened at. Where the desktop passes
  dropped files on (X11, Windows, macOS; not yet Wayland), dropping a file
  or a folder on the window analyses it. The app asks the device for 48 kHz in as few
  channels as it offers, mixed to mono, and takes 16-bit samples when the
  device has them.
- **Input level.** In Settings under Microphone: the microphone's gain and a peak meter
  with marks at the −10 dBFS target and the −6 dBFS limit. On Linux, for
  the system default the level is the sound server's input volume
  (`wpctl`, or `pactl` on PulseAudio), which the server writes back onto
  the microphone each time it opens it, so setting the card's own control
  there would not last; for a direct device it is the card's capture
  control (`amixer`). The change is made when you let go of the slider.
  On a Mac or Windows the system sets the level: **Open Sound Settings…**
  opens the system's sound input settings (Sound > Input on a Mac, Sound
  on Windows), with the Peak meter here to watch as you change it. The
  same code (`timegrapher_core::mixer`) gives `timegrapher doctor` its
  proposals.
- **Automatic Gain.** In Settings under Microphone, for an input whose
  own automatic gain the app can reach: the sound card's mixer switch on
  Linux, on a Mac a USB microphone's switch, set over USB
  (`timegrapher_core::mixer::usb_agc`) since macOS shows no such setting,
  and on Windows the input's own switch through the system
  (`timegrapher_core::mixer::endpoint_agc`, the AGC tickbox the Sound
  Control Panel shows on some inputs).
  Off by default and kept for each input. Each time the app opens the
  input it sets the switch to match, since a stand turns it back on each
  time it is plugged in, and it reads it back: where the microphone
  reports otherwise, the row's caption says so in place of the warning to
  keep it off. On a Mac the
  request reaches every USB microphone's switch at once, so it is sent
  only when the input's own switch needs changing. The switch is hidden
  for an input without one, such as a built-in microphone.
- **Calibre.** Not Set, a calibre from the list (`timegrapher_core::calibres`,
  some 4,500 of them, each with its sources), or one of your own.
  - **The menu.** On the Calibre card with no watch picked, and in the
    New Watch form. A **Search every calibre** box at its top, ready to
    type into, lists every calibre matching a name, another name or the
    maker (the first 200, with how many more). Empty, the menu offers Not
    Set, then **Recent**, the **Short List** by maker and **Custom** (your
    own calibres). The list scrolls between the search field and
    **Manage Calibres…**, which stays at the menu's foot and opens
    Settings at Calibres, and as a button under it. Recent is the 6
    calibres the app was last set to, by a pick in the menu, a watch or
    take opened, or a take saved: the latest first, none twice, never
    expiring, and kept across restarts.
  - **Manage Calibres.** The Calibres category of Settings, which
    Manage Calibres… opens. Under the heading, one search field over the
    table ("Search Makers and Calibres": what you type in the maker or the
    calibre together, or one of the calibre's other names), the count of
    calibres shown beside it ("4,537 calibres"), and at the right **New
    Custom Calibre…**, which adds a calibre the list doesn't have, shown
    and selected whatever the filters hide. Then every
    calibre in a table on the left and, level with it on the right,
    the one selected in full (see **The details** below). Opened
    from the Calibre menu while a beat rate is known (set on the Calibre
    card, found from the beats, or the watch's in its form), the table
    starts with the Beat Rate funnel at that rate and says so above it
    ("Showing 28800 bph, the rate found from the beats"), with **Show
    All** beside it; the Beat Rate funnel's Clear Filter clears it too. Opened from the gear,
    or with no rate known, nothing is filtered.
    - **Columns.** **Maker**, **Calibre**, **Beat Rate (bph)**, **Lift
      Angle (°)** and the short list, a narrow column headed by a star with a star on
      each row, filled for a calibre on the short list and an outline for
      one off it. Clicking a heading sorts on it, a second click reverses
      it; it starts by maker. Rows are 28 points. The units are in the
      headings, and the numbers are right-aligned under the end of theirs.
      A value the list doesn't give is a dim "—" (its filter lists it as
      "Not Given") and sorts after every value.
    - **Custom calibres.** Your own calibres show under the maker
      **Custom**, in the accent colour, always at the top of the table
      whatever the sort, and the Maker funnel filters them as any maker.
    - **Filters.** Each heading has a ▾ just after its name that opens
      its filter, as a spreadsheet's column filter does; while a column's
      filter is on, its ▾ is a funnel in the accent colour. Its menu sorts on the column (A to
      Z and Z to A; smallest or largest first for numbers; starred first or
      last for the short list), clears that column's filter, and lists the
      column's values with a tickbox each, **(Select All)** at the top and
      a search over them. The list keeps its height while you search. In
      Maker and Calibre the search also finds a calibre by one of its
      other names, or its maker under any of the maker's names ("dd 3235",
      "sw200", "seagull st19"). With a search typed, Select All reads
      **(Select All Search Results)** and picks just the values the search
      lists, as a spreadsheet's does. The star column's funnel with only
      Starred ticked shows just the short list. The
      ticks apply on **OK**; **Cancel** leaves the filter as it was, and OK
      needs at least one value ticked. It lists the values the other
      filters let through; the funnels combine, so a row shows only when
      every column's value is ticked, and the search over the table
      narrows them too.
    - **The details.** The calibre selected, in a card level with the
      table: its name alone (every fact in the row stays in the row; other
      names it goes by under it), then for a custom calibre its name,
      maker, beat rate and lift angle, then the **Wheels** group, a row
      for each wheel's time for one turn, a caption under the group saying
      so:
      - The six every lever movement has (balance, escape, fourth, third
        and centre wheels, and the barrel) come first, then any other the
        list names.
      - The hint on each wheel says where its figure comes from.
      - A wheel nobody has given reads a dim "Not Known": type its period
        and it is kept for that calibre, its caption "Custom", with
        **Clear** to go back.
      - **Add a Wheel…**, the group's last row, opens a row to type
        another wheel's name and period, with **Add** (or Return) to add
        it once both are filled in, and **Cancel** to forget the row.
      - Then the **Sources** group, its title saying how the figures are
        known ("Sources · Published List"): a row for each source, its
        name the link (the address only on hover) and under it what the
        source says, then the list's notes.
      - At the foot, **Share with the Project…** and, for a custom
        calibre, **Delete Calibre**.
    - **A narrow window.** When the table and the details don't both fit,
      the table takes the whole width, and choosing a calibre shows its
      details in its place with **‹ All Calibres** to go back.
  - **The short list.** It starts as the list's common and modern
    calibres. In Manage Calibres clicking the star in a calibre's row puts
    it on the short list or takes it off; filter the table with the funnels
    first to work through one maker, or the clones. **Reset the Short
    List**, in the star column's funnel menu, goes back to
    the one the app comes with. The short list and the recent picks are
    kept with the app's settings. When the list renames a calibre (the
    "vph" names became "bph"), the short list, the recent picks and a take
    saved under the old name all use the new one.
  - **What a pick does.** It fills in the calibre's lift angle, where the
    list gives one, and names the cycles Steadiness finds after its own
    wheels. If the watch beats at another rate than the calibre, a line
    under the menu says so. With none picked and the lift angle left at
    52°, a line says the amplitude is worked out from that default.
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
  - **A custom calibre.** It starts with no wheels: a custom calibre is
    often obscure or vintage, so nothing is assumed. Typing its beat rate
    fills in the balance's period, two beats (7200 ÷ bph seconds: 0.25 s
    at 28,800 bph), and a new beat rate changes it, unless a period typed
    by hand is there.
  - **What isn't kept.** The pick itself, like the lift angle, is about
    the watch on the stand and isn't kept. Not Set names cycles after the
    wheels most calibres at the beat rate share.
- **Beat rate.** On the Calibre card: Auto (guessed from the first seconds) or any standard rate
  from 12,000 to 72,000 bph. Changing it starts the readings again; with
  a take open, the take is read again at the new rate, keeping the
  settings and masks as they are.
- **Lift angle.** **Lift**, on the Calibre card: 52° by default. Type the calibre's angle and press Enter,
  or pick a common one. Amplitude is worked out again at once, history
  included.
- **Position.** On the Current Position card, or keys 1 to 6: DU, DD, CU, CD, CL, CR, with the Swiss laboratory names
  beside the crown positions, the hour that points up: Crown Up (CU, 3H),
  Crown Down (CD, 9H), Crown Left (CL, 6H) and Crown Right (CR, 12H).
  The Positions Test's table names them the same way. A new position is a new
  measurement, so changing it while listening starts a new
  stretch, noted in the saved recording. The strip keeps its beats and
  marks each stretch on its track (see **The track** under Paper
  Strip); the Rate, Amplitude and Beat Error charts mark them too,
  shading every other one, over the session or the strip's length. The readings don't reach back
  past the change, the tick and tock profile starts again, and the
  distributions and Steadiness take only the stretches in the position the
  watch is in now: dial up for 5 minutes, dial down for 10, then dial up
  again tests the two dial-up stretches together and leaves the dial-down
  one out. The first seconds in a new position are settling, as many as
  the Positions Test pane's **Settle** (20 s unless set otherwise), the
  one settling for every change of position, in a free take or the guided
  test alike, from the next change on: the track is solid teal through
  it, the beats are drawn faintly, and they are left out of the
  readings, the charts' points, the distributions and Steadiness, but kept
  in the audio. A saved take's `session.toml` starts each later position
  after its settling, and each entry says `settle_s = 0`, so
  `timegrapher session` skips nothing more and reads the same seconds as
  the app (a take saved by an earlier build skips its settling a second
  time there). Both correct the rates from the take's clock log (see True
  Clock). Guided runs through the positions (as `timegrapher
  session` reports them) will build on it.

## Sessions

At the first launch the app shows **Set Up the Microphone**, and listens
while it is open. It is three numbered steps: **1, Choose the microphone
the watch sits on** (the microphone menu with Rescan, and Show Every
Input); **2, Put a ticking watch on it**; **3, Set the Input Level so the
peaks sit in the shaded band**: Input Level ("Not adjustable here" where
the system sets it), **Automatic Gain** with its standing warning to keep
it off (only for an input whose automatic gain the app can reach), and
the **Peak** meter with its dBFS, over a shaded band from −10 to −6
dBFS. Under them a status line two lines tall says one thing: the green
dot and "Listening to …", a failure (the red icon, then its words, cut at
two lines with the whole of it on hover), a warning such as "Clipping:
lower the Input Level.", or "Not listening.". It changes nothing around
it. At the bottom are two notes and **Not Now** and **Done**; Return
presses Done and Escape presses Not Now. Choosing another microphone
listens to that one instead. Done keeps Quick Listen running and counts
the microphone as set up. Not Now stops listening and closes the sheet,
which then doesn't come back by itself (Settings > Microphone sets it
up later); it doesn't count as set up, so the app still doesn't listen
by itself at launch. Once the microphone is set up (by Done, or by any
listening that opened it with the sheet closed), every launch starts
Quick Listen at once, unless **Listen When the App Opens** (Settings,
under General) is off or a recording was given on the command line. With it off, the microphone is opened only when
listening starts. When another program records from the same microphone
(a long take with the recorder, say), don't start listening during its
take, and press Stop before one begins.

**Listen** on the Listen card (Quick Listen), or **Listen**, **Positions
Test** or **Power Reserve Test** for a watch picked in the Watches view,
begins a session from the microphone. **Stop** stops
listening and lets go of the microphone, so another program can use it,
and keeps everything on show: the
readings, strip, charts and the sound so far. There is no pause:
listening again starts a new take.
**New Session** clears everything for the next watch or position;
pressed while listening, listening carries on into the new session
(unless closing the kept sound fails: see Recording stopped, below).
Everything heard is kept as it comes in, so **Save…** can be pressed at
any time, running or stopped: with a watch picked it saves the take into
that watch, after Quick Listen into Unsorted, and otherwise it asks for a
folder and copies the session into a folder of its own inside it. A Positions
Test or Power Reserve Test that ends with no watch picked asks where it
goes, at Stop (for a Power Reserve Test, before its take is saved):
**Keep in Unsorted**, the default, keeps it in Unsorted to file under a
watch later; **Keep in a Watch…** keeps it in a watch picked from the
menu, or in a new one (**New Watch…**), with the settings it ran with
(the watch's calibre, lift angle and beat rate apply to its next take);
**Throw Away** deletes it, sound and readings. The question sits at the
top or the foot of the window, whichever is clear of the test's pane, so
the test's results stay in view. Quick Listen's sound is never asked
about; a test's always is. **Save Again** (Ctrl+S) later brings
that copy up to date without asking again, and **Save As…** (Ctrl+Shift+S)
copies the session to a new folder of your choosing, which Save Again then
keeps up to date. Sound that hasn't been saved is never thrown away
without asking, except a Quick Listen take's: New Session, opening a
recording, starting a take of a watch, closing the window and File > Quit
ask whether to save it first, while unsaved Quick Listen sound goes
without a question. Until it is
saved it is kept in a folder of the app's own (`timegrapher/unsaved`
under `~/.local/share` on Linux, `~/Library/Application Support` on a
Mac, `%LOCALAPPDATA%` on Windows), which the system doesn't clear. When
the app quits without asking (on a Mac, Cmd+Q and Quit in the app menu or
the Dock, which can't be held back; on any system, a logout or a
shutdown) with more than 5 s of unsaved sound other than a Quick Listen
take's, or is stopped by a crash,
the sound stays there, and the next start says so in the status bar,
**Unsaved Sound Found…**, which opens the message with when it started (in this computer's time), its length and
size, and **Open** (analysed here, and offered again next time),
**Move To…** (the recording's folder moved into one you pick, to open
with Open Folder…), **Delete…** (asking first) and **Later**. A folder
another copy of the app is recording into is never offered. The
microphone's own settings are kept aside while recordings are open and
come back with the next take on the microphone.
Stopped during a move's settling time, the rate card gives the reading
from before the move and says so.

**The cards keep their height.** Every card is 134 points tall at every
window width, whatever it shows: its three rows have their places whether
anything is in them or not. A missing figure is a "—", with what the
reading waits on in the unit's place, in short words ("Settling, 20 s";
"Waiting for Beats"; "Before the Move"), the full sentence in the hover.
With no regular beat, the reading cards say "No Regular Beat" there, with
the full sentence and the timing scatter in the hover. A figure too wide
for its card is drawn smaller. So the cards, and every pane under them,
never change height with the readings. No pane is squeezed below 120 points, so the Paper
Strip keeps room to read on a short window.

Each saved recording is a folder named after the start time, then the
watch's name and the calibre when they are set by the first save
(`2026-10-08T21-30-00_Yacht-Master_Rolex_3235`, or just the time), so it
can be analysed again later with a newer engine. The positions aren't in
the name: `session.json` and `session.toml` say where the watch was and
when. It holds:

| File | Contents |
| --- | --- |
| `audio-001.wav`, ... | The audio as captured, mono, 16-bit for a 16-bit microphone, in files of at most an hour; listening again after a break (the computer sleeping) starts a new file |
| `clock.csv` | System time against frames captured, every 10 s, and on both sides of each break; frames run on across a break, with no line made up for the gap |
| `session.json` | Watch, position, lift angle, beat rate, device, start and end, position changes, the readings at the end, and (only once the take has a break, such as the computer sleeping while it listened) `breaks`: one entry per break with `frame` (frames in the take before it), `audio_s`, `segment` (its first file), `utc` (host time of its first frame), `gap_s` (host time missing, from the clock log either side) and `device` (the input after it) |
| `session.toml` | The session file `timegrapher session` reads: watch, calibre (with the wheels of a custom one), lift angle, beat rate, and one `[[recording]]` per stretch in one position with its `start_s` and `end_s` (for a take saved during a Positions Test, the test's own measured stretches, after each settling and for its measuring time, so `timegrapher session` reads the seconds its table did, with `wind_h` in the 24 hour half) |
| `positions-test.json` | Only for a finished Positions Test run just after winding: its results, to load for the test 24 hours later (the format is under Full wind against 24 hours) |

Opening a recording or folder that has a `session.toml` beside it (or in
it) that lists it takes back the watch, the calibre (a custom one joins
the custom calibres), the lift angle, the beat rate and the position stretches,
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
last 2 seconds. It can't be measured again with
other settings, and the Scope needs the sound: it says there is no
waveform. Masks
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
contain incomplete audio or clock information. Save and Save Again
are unavailable for that session so an earlier saved take stays unchanged.
This is different from an input disconnect alone, after which the take can
still be saved.

Copy the folder shown in the warning to a location outside the app's
folder for unsaved sound before attempting manual recovery or cleanup; do
not treat it as a complete saved take. The next start also offers it back
as unsaved sound. The app attempts to leave a
`RECORDING-ERROR.txt` note there and reports if even that write fails. New
Session, opening another recording, and quitting leave the faulted folder on
disk. After resolving the storage problem, start a new session.
