# 9. Calibres and Lift Angles

Two of the figures depend on knowing the watch's movement, its
**calibre**:

- the **amplitude**, which is worked out from the lift angle, a figure set
  by the calibre's design (see
  [How a Timegrapher Hears a Watch](01-how-it-works.md#amplitude)); and
- the **names of cycles** in [Steadiness](08-over-time.md#steadiness),
  which come from how long each wheel of the calibre's train takes to
  turn.

Timegrapher carries a list of some 4,500 calibres, each with the sources
its figures come from, and lets you add your own.

## Choosing the Calibre

Under **Watch**, the **Calibre** menu offers:

- **Not Set**;
- **Recent**: the calibres you picked lately;
- **Short List**: common and modern calibres, by maker;
- **Your Calibres**: ones you have added.

![The Calibre menu open in the Watch pane: Not Set, then Rolex 3235 under Recent, then the Short List by maker](images/calibre-menu.png)

For anything not on those lists, **Manage Calibres…**, at the foot of the
menu and as a button under it, searches the whole list.

### What a Pick Does

- It fills in the calibre's **lift angle**, where the list gives one.
- It names the cycles Steadiness finds after the calibre's own wheels.
- If the watch beats at a different rate from the calibre's, a line under
  the menu says so.
- Under the menu, a line gives the maker, beat rate, lift angle and escape
  wheel, or says which of them the list doesn't give.
- **Wheels and Sources**, folded until you open it, lists each wheel's time
  for one turn and where the figures come from (see below).

With **Not Set** and the lift angle left at 52°, a line under the menu says
that the amplitude is worked out from that default, and the Amplitude
card says **lift angle 52°, the default (calibre not set)**.

The pick is about the watch on the microphone, so the app doesn't keep it
between runs.

## Manage Calibres

![Manage Calibres with "3235" typed in the search box: five calibres found, from Rolex, Peacock (Dandong), Shanghai Jinghe and Shanghai, each with its beat rate and lift angle](images/manage-calibres-search.png)

**Manage Calibres…** opens a window over the app with every calibre in a
list on the left, by maker (common and modern makers first, your own
calibres at the top), and the one you select in full on the right.

- **All**, **Short List** and **Yours** choose what the list shows.
- **The search box** searches the whole list. Every word you type must
  match a calibre's number, one of its other names, or its maker under any
  of the maker's names, so "dd 3235", "sw200" and "seagull st19" all
  work. The best matches come first.
- **Use for This Watch** sets the selected calibre under Watch.
- **New Custom Calibre…** adds one, named as typed in the search box.
- **Done**, Escape or a click outside closes the window.

### Reading a Calibre's Entry

![A calibre selected in Manage Calibres: its maker, beat rate, lift angle and escape wheel; each wheel's time for one turn; its sources as links; how its figures are known; and its notes](images/manage-calibres.png)

The entry on the right gives:

- **the maker, beat rate, lift angle and escape wheel**, and the calibre's
  other names;
- **each wheel's time for one full turn**: the balance, escape wheel,
  fourth wheel, third wheel, centre wheel and barrel, then any other the
  list names. Rest the pointer on a wheel to see where its figure comes
  from. A blank one isn't known;
- **Sources**: links to where the figures were found;
- **how the figures are known**: from a published list, the maker's
  page, tooth counts or the beat rate, or measured;
- **notes**, which say where sources disagree.

**Read the sources and notes.** Lift angles in particular are not always
agreed: the same calibre can appear in different sources, or in different
revisions of the maker's documents, with different figures, and the list
says so where it knows. The entry shown above is an example: its notes
record two lift angles in circulation for that calibre, and the list
carries a second entry with the other figure. A difference of a couple of
degrees moves the amplitude by about 8°, so pick the entry whose source
you trust, or type the lift angle you have a source for.

### The Short List

The short list starts as the list's common and modern calibres. In Manage
Calibres a tickbox beside every calibre puts it on the short list or takes
it off, and **Tick All** and **Untick All** beside a maker do a whole maker
at once. **Reset the Short List** goes back to the one the app comes with.
The short list and your recent picks are kept with the app's settings.

## Adding What You Know

- **A wheel's period**: in a calibre's entry, type a period into a blank
  wheel. It is kept for that calibre and marked "yours", with **Clear** to
  go back. **Another wheel** adds one by name.
- **A custom calibre**: **New Custom Calibre…** in Manage Calibres. Its
  entry also has its name, maker, beat rate and lift angle, and **Delete
  Calibre**.
- **Share with the Project…** opens a GitHub issue in your browser, filled
  in with what you added to a calibre, for you to look over and send.
  Nothing is sent from the app.

Your additions are kept in `calibres.json` in the settings folder (see
[Problems and Reports](14-problems-and-reports.md#where-things-are-kept)),
which the command line and the Claude Desktop extension read too, so a
calibre you add in the app can be used by name everywhere.

## Setting the Lift Angle by Hand

**Lift Angle** under Watch is 52° unless a calibre sets it. Type the
calibre's angle and press Enter, or pick a common one from the list beside
it. The amplitude is worked out again at once, history included.

## Beat Rate

**Beat Rate** under Watch is **Auto** (found from the first seconds of
beats) unless you set it to one of the standard rates from 12,000 to
72,000 bph. Changing it starts the readings again.

## From the Command Line

`timegrapher calibres` lists the table, and `--calibre NAME` on `analyze`,
`long`, `series`, `record` and `session` uses a calibre's beat rate, lift
angle and wheels. See [The Command Line](12-command-line.md#calibres).

---

Previous: [Over Time](08-over-time.md) ·
Next: [Positions and the Positions Test](10-positions.md)
