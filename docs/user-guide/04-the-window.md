# 4. The Window

The window is arranged around three tasks: setting up the watch, watching
its current behaviour, and reviewing the recording. You can rearrange it
for the job at hand. A wide strip and small profile suit a quick timing
check; an overnight recording benefits from larger charts and analysis
panes.

![The main window with the Watch pane, reading cards, Paper Strip, profile and charts](images/main-window.png)

At the top are the menus, pane icons and reading cards. The panes occupy
the central area; the status line runs along the bottom. After capture
stops, or when a recording is opened for analysis, a **Time** slider below
the readings selects an earlier moment.

## The Menu Bar

| Menu | Main actions |
| --- | --- |
| **File** | **Open Recording…**, **Open Folder…**, **Save… / Save Again**, **Save As…**, **New Session** (also while listening), **Quit** |
| **View** | Show or hide readings and panes, choose Steadiness panes, **Reset Panes**, and select **Appearance** |
| **Help** | **Check for Updates…**, **Check for Updates at Start**, **Report a Problem…** and, on Linux, **Add to Applications Menu** |

The main shortcuts are **Ctrl+O** for Open Recording, **Ctrl+S** for Save
or Save Again, **Ctrl+Shift+S** for Save As, and **Ctrl+Q** for Quit. Use
Cmd on macOS.

**Appearance: Auto** follows the system's light or dark preference;
**Light** and **Dark** override it. The update check asks GitHub about
published releases and offers a link when one is available. Downloading
and installing remain separate actions. See
[Problems and Reports](16-problems-and-reports.md#settings-logs-and-updates).

## Finding a Pane

The icons beside the menus show or hide panes. Hover for a name; an active
icon indicates that the pane is shown. They are grouped into the main
views, Charts, Distributions and Steadiness. The same choices are available
in View, so you do not need to memorise the icons.

| Pane | Purpose | Guide |
| --- | --- | --- |
| **Watch** | Input, watch identity, calibre, lift angle, beat rate and position | [Microphone](03-microphone.md), [Calibres](11-calibres.md) |
| **Paper Strip** | Beat timing and fitted-rate overlays | [Paper Strip](06-paper-strip.md) |
| **Tick Tock Profile** | Combined acoustic envelopes and measured edges | [Sound of the Beat](07-sound-of-the-beat.md) |
| **Scope** | Individual filtered waveforms | [Sound of the Beat](07-sound-of-the-beat.md#the-scope) |
| **Rate**, **Amplitude**, **Beat Error** | Ordinary charts against time | [Over Time](08-over-time.md#the-charts) |
| **Rate Distribution**, **Amplitude Distribution**, **Beat Error Distribution** | Values without their time order | [Distributions](08-over-time.md#distributions) |
| **Readings Over Time**, **Periods**, **Cycle** | Steadiness data and periodic structure | [Steadiness](09-steadiness.md) |
| **Autocorrelation**, **Allan Deviation**, **CUSUM**, **FFT** | Correlation, averaging behaviour, level changes and spectral size | [Steadiness](09-steadiness.md) |
| **Isochronism** | Rate versus amplitude and a run-down comparison | [Isochronism](10-isochronism.md) |
| **Positions Test** | Guided measurements in six positions | [Positions](12-positions.md) |

**Rate** and **Readings Over Time** are different panes. Rate follows the
chosen chart averaging interval and time navigation. Readings Over Time
belongs to Steadiness, with fixed analysis intervals and its own data
selection. The same distinction matters when comparing their amplitude
and beat-error displays.

## Arranging the Panes

Drag a pane's tab to place it beside, above or below another pane. Drop it
on another tab area to make a stack of tabs. Drag the boundaries between
panes to resize them. Use the **×**, an icon or View to hide a pane; showing
it again restores its place. **View → Reset Panes** restores the default
layout if an arrangement becomes awkward.

For a useful review layout, set the Paper Strip to **Across**, put the
ordinary charts beneath it, and enable **Match Strip** in their tab bars.
Keep Tick Tock Profile nearby. A chart event, a bend in the strip and its
sound can then be inspected together.

Pane settings sit in their toolbars or tab bars. Two-way controls such
as **Stacked / Beside** switch between their alternatives. A narrow pane
may truncate text; hover over a summary to read the full version.

## Help at the Point of Use

Hover over **?** beside a card or pane name for a short explanation.
Click it for longer help, and click again or elsewhere to close it.
Individual controls also have hover hints. This guide develops the
measurement concepts and practical checks behind those brief explanations.

## The Status Line and Banners

The status line identifies the source, opened audio format, signal ratio,
amount of captured audio, save state, current messages, beat rate and
position. It also provides update and report actions. Check it when a
screen looks plausible but may be showing a different input or recording
from the one you intended.

Banners draw attention to capture failures, clipping, available updates
or a possible unmarked position change. Read the message itself: an amber
clipping banner describes the current measurement interval, while a red
capture error can remain until dismissed or capture restarts.

## What Is Remembered

The app keeps your workspace preferences: window size and position,
pane arrangement and visibility, appearance, averaging settings,
strip and profile options, folded control groups, Keep Awake and the last
microphone selection. Calibre short lists and custom entries are also retained.

A fresh launch does not assume that the previous watch is still on the
microphone. Watch-specific settings such as calibre, lift angle, beat
rate and position start fresh. Opening a saved session can restore its
recorded context; see [Sessions and Recordings](13-sessions-and-recordings.md).

---

Previous: [Setting Up the Microphone](03-microphone.md) ·
Next: [Readings](05-readings.md)
