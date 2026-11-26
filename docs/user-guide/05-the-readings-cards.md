# 5. The Readings Cards and the Calibre Card

The six cards across the top of the Bench hold everything the app knows
about the take on show: where the watch lies, what it is, whether it is
being heard, and the three readings.

![A Quick Listen take a minute in, marks 1 to 15 on the six cards](images/readings-cards.png)

Every card has the same three rows, so the titles, the big figures and
the controls each read in one line across:

- **The title row:** the card's name, its **?**, and a chip when
  something limits the reading.
- **The figure row:** the big figure, with its ± and unit.
- **The controls row:** the card's controls, or a small chart.

## Current Position

| Mark | Part | What it does | Why you'd use it |
| --- | --- | --- | --- |
| 1 | Picture and name | The watch as it lies, in teal: the dial for Dial Up, the case back for Dial Down, the crown at the top for Crown Up, and so on. Beside it, the name and both codes: the common one, then Witschi's ("CD · 9H"). | To check at a glance that the app has the position you put the watch in. |
| 2 | **DU DD CU CD CL CR** | Sets the position. The current one is filled. Its key does the same: **U** and **D** lying, and standing the last digit of the hour at the top, **3**, **6**, **9** and **2** (chapter 4). | Each time you turn the watch, so the readings are filed under the right position. |
| 15 | **6 \| 10** | **10** adds a second row of buttons, the four 45° stops **1H30 4H30 7H30 10H30** (1:30 Up and so on: the hour at the top), keys **1**, **4**, **7** and **0**. A 45° stop picked by its key, the Position Brush, or a take on show turns the card to 10. Kept between runs. | For a stand that stops every 45°, to file readings taken between the crown positions. **6** keeps the card to the six usual positions. |

- **The bar under each button** fills with the seconds heard in that
  position, settled and not masked. It is full at the Positions Test's
  **Each Position** time, 60 s unless you change it.
- **Why the bars help:** in a free-form test, where you turn the watch
  yourself, they show which positions still need time.
- **When the buttons only show:** with a take on show, or during a
  Positions Test or Power Reserve Test, the buttons show the position but
  can't change it. A saved take's positions are fixed.
- **Settling:** after each change of position, the first seconds aren't
  read while the watch settles. The button's hover gives the time.

**Good practice:** after you turn the watch, wait until its readings
steady before you judge them, and give a slow position longer. Witschi
gives each position "20 s stabilisation time and ≥40 s measurement time
for stable results" ([Witschi 2025](https://www.witschi.com/wp-content/uploads/firmwares/8Z1UtLGf/20251118_witschi_testing-methods-for-mechanical-watches_en.pdf), p. 33); some watches need far more. On the
public corpus take `datejust41_3235_multipos_105min`, counting from
each turn, Crown Left kept moving for about 3 to 4 minutes and Dial
Down for about 2, well past a settle of 20 or 30 s.

## The Calibre Card

The Calibre card says which watch is on the microphone, and holds what
the readings are worked out for: the calibre, its lift angle and its beat
rate.

| Mark | Part | What it does | Why you'd use it |
| --- | --- | --- | --- |
| 3 | **The Watch line** | The card's title: the watch picked, or **No Watch (Quick Listen)**. Its menu: No Watch (Quick Listen), up to five **Recent** watches, **New Watch…** and **Find in Watches…**. | To choose which watch a take goes into. It is the one place on the Bench that says so. |
| 4 | **Calibre** | Picks the calibre from about 4,500, or one of your own. | The calibre sets the lift angle, which every amplitude depends on, and the beat rate. |
| 5 | **Lift** and the beat rate | The lift angle, 52° unless set; then **Auto** with the beat rate heard ("Auto 28800"), or a rate you pick. | When the calibre isn't listed, or you know better. |

### Picking a Calibre

![The Calibre pop-up open with "Seiko 6R" typed, marks 1 to 3](images/calibre-picker.png)

| Mark | Part | What it does |
| --- | --- | --- |
| 1 | **Search every calibre** | Matches a calibre's name, another name for it, or its maker. ⊗ or Esc clears it. |
| 2 | The matches | ↑ and ↓ move through them; Enter picks the highlighted one. |
| 3 | **Manage Calibres…** | Opens Settings at Calibres, to add your own or change the Short List. See chapter 24. |

With the search empty, the menu offers **Not Set** (back to Auto and
52°), then **Recent** (the last six calibres used), the **Short List** by
maker, and **Custom**, your own calibres.

- **What a calibre sets:** its beat rate, and its lift angle where the
  table has one. They then show as text, not as controls.
- **A calibre that beats at another rate** than the one heard gets a chip,
  such as "Peacock SL1258 is 28800 bph". Check which is right: the
  calibre, or the watch.
- **With a watch picked,** the card names the watch's calibre. Change it
  with **Edit…** on the watch, in the Watches view. A watch whose calibre
  isn't in the table reads **No Calibre**; Lift and the beat rate set on
  the card are then saved to the watch.

### Why the Lift Angle Matters

The amplitude is worked out from the time the balance takes to pass
through the lift angle. A lift angle 3° too small reads every amplitude
about 5% low.

- With no calibre or lift angle set, the app uses **52°**, and Lift's
  hover says it is the default.
- Treat an amplitude on the default as rough until you set the calibre.
- [M4, Amplitude and Lift Angle](m4-amplitude-and-lift-angle.md) has
  the formula.

**Good practice:** set the calibre, so the amplitude uses the maker's
lift angle: "To obtain an accurate amplitude reading, the lift angle of
the movement must be known" ([Witschi 2025](https://www.witschi.com/wp-content/uploads/firmwares/8Z1UtLGf/20251118_witschi_testing-methods-for-mechanical-watches_en.pdf), p. 9). A typical figure is only a
start: "In most modern movements, the lift angle is approximately 51°"
(p. 12), and calibres differ: the table gives the Dandong 3235 59°.

### Why Auto Is Usually Right

**Auto** finds the beat rate from the beats themselves. Set it by hand
only when Auto picks a wrong standard rate, for example on a very quiet
or noisy take. A wrong beat rate gives readings far out, or none.

## Listen

| Mark | Part | What it does | Why you'd use it |
| --- | --- | --- | --- |
| 6 | **Listen** / **Stop** | Starts a Quick Listen take, or with a watch picked, a Listen take of that watch. It becomes **Stop** while listening. | To start and stop measuring. There is no pause: Listen again starts a new take. |
| 7 | **Save** | The Save menu: Unsorted Takes, Keep in a Watch, Save in a Folder…. Hidden while a watch's take records into it, and for a test, set up or running. | See [Stop and Save](02-your-first-take.md#stop-and-save). |
| 8 | Length and state | The take's length, and its state: Not Started, Ready (a test set up, waiting for Start Test), Listening, Stopped, Opening or Opened. | To see how much has been heard. |
| 9 | **Average Over** | How many seconds of beats each reading covers: 2, 4, 10, 20, 30 or 60 s. 10 s unless changed. | See [Choosing Average Over](#choosing-average-over). |
| | **Wind** | Beside Average Over: when the watch was last fully wound, as "Full 2.0 h ago", counting up. Its menu sets it: **Full Wind Now**, **Full Wind [ ] h ago**, or **Not Given**. | So each take is saved with its hours since full wind. See [Wind](#wind). |

### Choosing Average Over

| Shorter (2 or 4 s) | Longer (30 or 60 s) |
| --- | --- |
| Follows a change quickly: turning the regulator, a knock, a change of position. | Steadier figures and a smaller ±. |
| More scatter in the last digit. | Slower to show a change. |

- **One setting for the whole app.** The Paper Strip's rate line, the
  Tick Tock Profile, Charts and Distributions all average over it too.
- **Early in a take,** or after a break, there are fewer seconds of beats
  than asked for. The readings are fitted to the beats there are, and the
  Rate card's hover says how many seconds of beats that is.
- **No reading reaches across a break**, a change of position, or more
  than 2 s with no beats: the watch may have moved or changed meanwhile.

### Wind

The rate and amplitude move as the mainspring lets down (chapters 19 and
21), so a reading means most beside its state of wind. Set **Wind**
before a take: each take is then saved with its hours from full wind to
its start. It is kept with the watch picked in Watches, or for a Quick
Listen until a watch is picked. **Start Test** on a test that begins at
full wind, the Positions Test's full-wind half or a Power Reserve Test,
sets Full Wind Now by itself. Started on a take already listening, that
take is left with no wind: it began before the watch was wound. If the
test's take is thrown away, the wind goes back to what it was before
Start, unless you set it by hand meanwhile. A Power Reserve Test that
found the watch stopped leaves it Not Given, kept or not.

Once a take has begun, or for a take opened, the box shows its hours
from full wind to the take's start, "Began 54.1 h after full". It can
still be set, running or stopped, counting from the take's start: **Full
Wind at Start**, **Full Wind [ ] h before Start**, or **Not Given**. A
take already saved has the new wind at once. A full wind after the take
began can't be given: the beats before it were at some other wind. In a
test that sets its own wind, the Positions Test or a Power
Reserve Test, the box is fixed. A take in Watches can also be given its
wind with **Set Wind** on its row (chapter 22).

The box's hover always gives the whole of it: "Full wind 2.0 h ago"
before a take, "Full wind 54.1 h before it began" once one has begun or
for a take opened, or "Wind not given" until it is set.

## Rate, Amplitude and Beat Error

| Mark | Part | What it shows |
| --- | --- | --- |
| 10 | **Rate** | Seconds a day gained (+) or lost (−), with its ±. |
| 11 | Sparkline | The reading over the take so far, in the reading's colour. |
| 12 | **?** | The card's help: hover for a line, click for the whole explanation. |
| 13 | **Amplitude** | The balance's swing, in whole degrees: the average of the Tick and Tock swings. |
| 14 | **Beat Error** | The size of the beat error, in ms. |

These cards have no controls: each is its figure. [Chapter 1](01-what-a-timegrapher-measures.md) says what each measures;
[M2](m2-rate.md), [M4](m4-amplitude-and-lift-angle.md) and
[M5](m5-beat-error.md) say how.

**Good practice:** read the amplitude with the watch's history in mind.
It falls as the oils age: "As lubricants age, the amplitude gradually
decreases" ([Witschi 2025](https://www.witschi.com/wp-content/uploads/firmwares/8Z1UtLGf/20251118_witschi_testing-methods-for-mechanical-watches_en.pdf), p. 12). Very high readings are a warning too: for
knocking, or overbanking, Witschi gives "Amplitude is too high
(> 330°)" (p. 36).

### What the Hover Shows

Rest the pointer on a figure to see what stands behind it.

![The Rate figure's hover on an opened take](images/rate-hover.png)

| Card | The hover adds |
| --- | --- |
| **Rate** | The sound card's clock (M3). The big figure is on the clock the take is read on: its USB lines, a clock log that measured the card well enough, or a correction kept for the input or saved with the take. The hover says first by how much and from what, as in the picture. **Listening,** a correction kept for the input applies from the first second, and the recording's USB lines once 30 s of sound are in; until then the hover says first which clock the figure is on for now. Then the beats the readings are fitted to ("79 beats in 10 s"), and the **jitter**. |
| **Amplitude** | The amplitude of the Ticks and of the Tocks, apart. |
| **Beat Error** | First its **BE Angle**, the beat error as an angle of the balance's swing: "That's a BE Angle of 1.7 ± 0.2° of the balance's swing, at 302° amplitude". The ms grow as the amplitude falls; the degrees stay the same (M5). Then the beat error timed from the drop, beside the big figure's, timed from the unlock; a co-axial has none from the drop. |

- **Jitter** is how far single beats land from the steady line the rate
  is fitted to, in microseconds: the spread of the dots across the strip.
  Noise, a muffled sound and the stand add to it as well as the watch.
- **A stopped or opened take** adds the **whole take** in the current
  position, with the settling left out: the rate over every beat, the
  amplitude and beat error over every window, and the range the 10 s
  rates covered (5th to 95th percentile). The command line's
  `timegrapher session` reads the same.
- **With the strip moved back** off the newest beats, the cards read the
  stretch ending at the strip's newest edge, and the hover says so. The
  status bar stays live.

### The ±

The **±** beside each figure is how far the reading would likely move on
a repeat under the same conditions: its standard error.

- It is worked out from how the short readings in the take scatter, with
  neighbouring readings that resemble each other counted as fewer.
- **It isn't accuracy.** A wrong lift angle or an uncorrected sound card
  clock gives a small ± on a wrong figure.
- It takes its scatter from at least 30 short readings. A reading with
  fewer borrows the scatter of the 30 nearest it in the take, so the
  amplitude's and beat error's ± appear after about a minute and the
  rate's after about 5 min of beats. The Rate figure's hover counts them
  until then: "± after 5 min of beats; 12 of 30 so far".
- [M7, The ± and Correlated Readings](m7-the-plus-minus.md) explains it
  in full.

### When Amplitude Differs from Your Bench Timegrapher

First check that both instruments use the movement's beat rate and lift
angle. Compare the watch in the same position and state of wind, let it
settle on each holder, and use the same averaging time where possible.
Then look at the **Tick Tock Profile**. On each side, does the Unlock
mark fall on the first sound and the Drop mark on the later, louder one?
A holder that muffles the unlock can give a repeatable amplitude with a
small ± even when the two instruments disagree.

For scale, on 90 readings of the same recordings at the same lift angle,
the median absolute difference between Timegrapher and the software
timegrapher **tg** was 2.8°. On the ETA 2824 recordings, Timegrapher
read 7° to 21° higher. These figures describe agreement between two
acoustic methods. The project also checked balance travel directly on
video for individual movements. See the [comparison with tg](../developer/regress.md#holdout-takes)
and the [optical comparisons and their limits](../research/validation.md#optical-amplitude-comparison).

### Chips

A chip beside a card's ? names, as a plain fact, something the ± can't
show. Its hover gives the whole sentence; with several, the chip shows
the first and how many more ("12 s of Beats +1").

| Chip | Means | What to do |
| --- | --- | --- |
| **6 s of Beats** | The beats span under 10 s. | Wait for more beats. |
| **540 of 600 Beats** | Under 95% of the beats expected were found. | Check the contact and the level. |
| **3% Clipped** | Over 1% of the beats in the reading clipped (Amplitude and Beat Error). | Lower the input level. See chapter 3. |
| **28800 bph Heard** | The beat rate set differs from the one heard. | Check the calibre or the beat rate. |
| **1 Break** | The reading's stretch holds a break in listening, such as the computer sleeping. | Nothing; the reading is fitted either side. |
| **6.8 s Masked** | Masked time in the stretch. | Nothing; it was left out by hand. |
| **Sides Named by Count** (Beat Error) | On an opened take, Tick and Tock were named across a break by counting beats, not by their sound. | Treat the beat error's sign across that break with care. |
| **Beat Error From the Drop** | The unlock wasn't found, so the beat error is timed from the drop. | Raise the level, or improve the contact, so the quiet unlock is heard. |
| **Co-axial Escapement Estimate (beta)** (Amplitude) | The calibre is a co-axial, read in beta: the amplitude uses the Swiss lever's formula and hasn't been checked against a reference. At narrower widths, **Co-axial Estimate (beta)**. | Treat it as an estimate (chapter 24). |
| **Co-axial (beta)** (Beat Error) | The beat error is the unlock's alone; a co-axial has none from the drop. When none is given, the chip says why: **Co-axial: Unlock Not Found**, or **Co-axial: the Unlock's Signs Disagree Beyond Their Spread**. | Nothing while a figure shows. With Unlock Not Found, help the quiet unlock be heard, as for Beat Error From the Drop. |
| **Amplitude Read From 4 of 15 Windows** | Under 60% of the short readings in the stretch could be used. | Treat the figure as resting on less than its span. |
| **First 12 s Left Out** | Noise before the watch was heard is left out of a whole take's reading. | Nothing. |

### Listening

In the first 10 s after Listen starts, and after a break, the timing
can't yet tell a watch from the stand's noise. Masked seconds don't
count toward the 10 s. The
figures show there only when the beats' sound already shows a watch.
When it can't tell yet, the cards show no figures, only **Listening**.
Where a settle is running, the cards say **Settling** instead.

### No Regular Beat Found

When the beats found don't come at regular intervals, the cards show no
figures, only **No regular beat found**, with the timing scatter: for
example "(timing scatter 8.6 ms, 5.2% of a beat)".

- **The rule:** over 10 s or more, a scatter of 2% of a beat or more is
  not a watch.
- **What to check:** that the watch is running and on the microphone,
  the input, and the level.
- No reading is graded. The app never says whether a figure is good.

When you put a watch on the stand after the app has been listening to
the room, its first seconds are judged together with the noise before
them. So while a regular beat is first heard, the cards say **Finding
the Beat**, not No regular beat found, for up to 20 s, until the time
they judge holds only the watch.

## Compact Readings

The pin at the right end of the cards folds them into a strip, with the
position, the watch, the take's length and the three figures. See
[chapter 4](04-the-window.md#compact-readings).

On a narrow window the cards also take smaller forms by themselves:

- **Current Position** becomes **Position**, with a **Set** pop-up of the
  six positions in place of the buttons, all ten when a 45° stop is
  current. The keys still work.
- **Listen** gives the length beside its caption, and Average Over as
  **Avg 10 s**. Wind steps down with the room: "Full 2.0 h ago" beside
  its label, then "Wind: full 2.0 h ago" ("Wind: 54.1 h at start" once a
  take has begun), then "Wind 2.0 h", then "Wind" alone. Its hover keeps
  the whole of it.
- **Calibre** keeps its pop-up and Lift. On the narrowest windows, the
  beat rate moves into the Calibre pop-up's menu, or for a watch whose
  calibre isn't in the table, into the Watch line's menu.

---

Previous: [The Window](04-the-window.md) ·
Next: [The Paper Strip](06-paper-strip.md)
