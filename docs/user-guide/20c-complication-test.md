# 20c. Complication Test

**What it shows:** how much a complication, most often a chronograph,
moves the watch when it runs: the amplitude, rate and beat error with it
off, then on, then off again, all in one position, and the change, **On
less Off**, with its ±.

**Why you'd use it:** a running chronograph takes power from the train,
so the balance gets less and its amplitude drops. Watchmakers read the
amplitude with it off, then on, and compare. Reading the
watch off again after On means a steady run-down of the mainspring
doesn't pass for the complication's effect.

The Complication pane is a hidden tab beside the Positions Test, its
icon a light switch (off, on, then off again), under **Tests** in the
icon bar and the View menu.

## The Toolbar

| Control | What it does | Why you'd use it |
| --- | --- | --- |
| **New Test** | Sets the test up on a clear Bench, in the position on the Current Position card, and starts nothing; **Start Test** on the Listen card starts it. **Cancel Test** takes its place until then. | To begin. |
| **Settle** and **Each Stretch** | The seconds the watch settles after each switch, and the seconds each stretch is measured. Every guided test shares these two: changing them here changes them in the Positions Test and Dynamic Poise too. | Longer gives a smaller ± (M7). The rate's ± needs about five minutes of take in all. |
| **Copy** | The watch, the position, the table and the findings, with tabs between the columns. | To paste into a spreadsheet or a message. |
| **Export…** | Saves the pane as a picture, PNG or SVG. | For a service record or a report. |
| **Info** (i, at the right) | How On less Off and its ± are worked out, and when a change is named. | To check the method. |

**Complication Test**, under **New Take** in the Watches view, sets the
same test up for the watch picked there, and saves the take into it
(chapter 22).

## Running a Test

1. Put the watch on the stand in the position you want, and set the
   Current Position card to it. The test stays in that position.
2. **New Test**, then **Start Test** on the Listen card, with the
   complication **off**. The banner reads "Complication Off (1 of 3)".
3. When the first stretch is done, the banner says "Turn the
   complication on". Start the chronograph, then press **Start On
   Measurement**.
4. When On is done, stop the chronograph and press **Start Off
   Measurement**.
5. After the second Off, the test reads the three together and stops
   listening, as Stop does.

Each stretch settles, then is measured. On the Paper Strip the time the
test doesn't use is marked **Not Used for Complication Test**.

## The Results

| Part | What it shows |
| --- | --- |
| The table | A row for each stretch, **Off**, **On** and **Off**, with **Amplitude** (°) first, then **Rate** (s/d) and **Beat Error** (ms). Then **On less Off**, each with its ±. |
| The plots | Three plots on one time axis (min:s): Amplitude, Rate, Beat Error. Each stretch is a point at its middle, with its ±. A dashed line joins the two Offs; a ring on it marks where On would sit with the complication off; a bracket runs from the ring to On: the change. A track over the plots names each stretch, lighter while it settles. Hovering a point gives its figure. |
| Each plot's title | Its finding in words, such as "Amplitude · 5 ± 1° lower with the complication on". When the change is within twice its ± of zero: "no clear change", with the figure and ±. |

**No ± yet.** A ± needs enough readings near the stretch, and the rate's
needs about five minutes of take. Until then its title gives the figure
and "no ± yet (too few readings)", and no verdict.

## How On less Off Is Worked Out

**On less Off** is the On stretch less the two Offs joined in a straight
line, read at the On stretch's middle:

```text
Off at On = w₁ × Off₁ + w₂ × Off₂,   w₁ = (t₂ − t_on) ÷ (t₂ − t₁),   w₂ = 1 − w₁
On less Off = On − Off at On
```

where `t₁`, `t_on` and `t₂` are the three stretches' middles. With the
stretches evenly spaced, as the guided test spaces them, `w₁ = w₂ = ½`
and Off at On is the mean of the two Offs. A run-down that moves the
watch steadily over the test moves both Offs and On alike, and drops out.

The three stretches are read apart, each after its own settling, so their
errors are independent and add in quadrature:

```text
± = √(SE_On² + w₁² × SE_Off₁² + w₂² × SE_Off₂²)
  = √(SE_On² + (SE_Off₁² + SE_Off₂²) ÷ 4)    evenly spaced
```

Each stretch's ± is its reading's (M7). The line is never carried past
the Offs: an On stretch outside them gives no figure. The joined line is
the one the plots draw, so the gap they show is the table's figure, even
when you waited before pressing a Start button.

**Named only when clear:** a change is called lower, slower or larger
only when it is more than twice its ± from zero. Otherwise the title
says "no clear change": a change too small to tell from the noise, not
proof of none.

## Why This Way

- **Off, On, Off rather than Off, On:** the amplitude falls as the
  mainspring lets down. With one Off, that fall would be read as the
  complication's. With an Off either side, a steady fall cancels.
- **Amplitude first:** the drop in amplitude is usually what the test is
  for.
- **One position:** a change of position moves all three figures (chapter
  20). Keeping the watch still leaves the complication as the only
  change.

## When You'd Use This

- **Servicing a chronograph:** before and after, to see whether the drop
  with it running has come down.
- **A watch that loses amplitude, or stops, with the chronograph on.**

## What It Can't Tell You

- **Which part of the complication** takes the power.
- **Whether the drop is too large.** The app gives no limit; compare
  with the maker's figure, or with the same watch before.
- **An uneven run-down.** Only a straight-line change cancels; one that
  bends between the stretches is left in the figure. Short, evenly
  spaced stretches keep that small.

---

Previous: [Dynamic Poise Test](20b-dynamic-poise-test.md) ·
Next: [Power Reserve Test](21-power-reserve-test.md)
