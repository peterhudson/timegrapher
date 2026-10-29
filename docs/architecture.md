# Timegrapher Architecture Guide

Version 1, approved 2026-10-28. This guide says how a figure gets from the sound to the screen, and where each decision on that path lives. Every PR that changes how a figure is read, kept, cached, worded or written is built to it and checked against it. Where today's code differs, the code changes, not the guide. A case the guide doesn't cover is raised, and its answer is added here.

Its peer for the interface is the [Design Guide](design/design-guide.md).

---

## 1. Principles

- **Core decides; the app, CLI and MCP draw.** Anything a user reads as a number, a yes or no, or a sentence of fact is decided in `timegrapher-core`.
- **One rule, one place.** This holds inside core too. A copy is a defect even when it matches today.
- **One way a thing happens.** Live and reopened, app, CLI and MCP all go through the same call. Two ways that agree today are still a defect, because they drift apart.
- **One take, one context, one clock.** Every reading of a take uses the same settings, clock, breaks, masks and windows.
- **Nothing kept goes stale.** A kept figure carries what it was read with, and it is read again when that changes.
- **Nothing changes quietly.** A fallback that changes a figure adds a caveat.

---

## 2. The layers

Every layer but the last is in core.

1. **Sound:** the part files, the clock log, the USB lines.
2. **Beats:** found by the stream engine. The live engine is only for listening; a saved take's record is the stream engine's beats. The good-beat rule lives here.
3. **Take context:** the settings, the clock, breaks, gaps, pauses and masks, each position's settle and measure window, and a revision.
4. **Readings:** rate, amplitude, beat error, ± and caveats, Power Reserve hours, distributions, Steadiness tests.
5. **Words:** decimals, when a ± shows, caveat text, fact sentences, and the writers of every take file.
6. **Surfaces (draw only):** app panes, CLI text and JSON, MCP views.

---

## 3. Who owns what

### Core owns

- **Every reading:** rate, amplitude, beat error and which one is shown, ±, distributions, Power Reserve hours, Steadiness tests and their marks.
- **Every threshold:** good beat, settle time, break step, minimum lengths, input limits.
- **Every window:** settle, measure, Average Over minimums, the halfway cap, hour boundaries on true time.
- **The clock:** one per take.
- **Breaks, gaps, pauses and masks:** one rule, live and saved.
- **Every take file:** its reader and its one writer.
- **Kept figures:** what is kept, its stamp, and when it is read again.
- **Fact words:** caveats, "No regular beat found", decimals, when a ± shows, and any help sentence that holds a number.
- **Defaults:** lift angle, escape teeth, a beat rate assumed when none is known.

### The app, CLI and MCP

- **May** draw and lay out what core gives, scales and labels included.
- **May** choose which core figure a pane shows.
- **May** take what the user types and pass it to core unchanged.
- **May** cache a core result, keyed on the take's revision.
- **May not** compare a reading with a number.
- **May not** pick a clock, cut a window, call a break or pick a sample.
- **May not** write a take file or the `timegrapher.analyze/2` document.
- **May not** word a fact, or type a core number into text.
- **May not** keep a figure core could read again.

---

## 4. One reading context per take

- **Core builds it once:** opened from a take's folder, or grown by the live engine while listening.
- **It holds:** the settings (lift, whether the lift is known, beat rate, escape teeth, calibre), the clock and where it came from, breaks, pauses, gaps and masks, each position's settle and measure window, and a revision.
- **Every reading takes it:** the cards, the whole take, the Positions table, Power Reserve hours, Distribution, Steadiness, `session`, `analyze`, `series`, `long` and every MCP tool.
- **Live is the same take, read sooner.** Once live and saved share one engine (#512), a live figure and the same figure reopened may differ only by the beats heard since, never by rule. Until then the stream engine's figure is the record.
- **The revision has parts:** settings, clock, breaks and masks. One place bumps each. A cache keys only on the parts it reads, so a mask stroke doesn't re-read every pane. Every kept figure carries the revision with the software version.

---

## 5. One clock everywhere

- Core picks each take's clock once, by the rule in `take::clock_from`: a clock log with USB lines, then a known correction, then USB lines alone, else the sound card.
- It lives in the context. Every pane and every surface shows it, live and reopened, the big live Rate included.
- When a correction arrives partway through a listen, the live Rate steps onto it, and the card says which clock it is on. This is wanted: the figure moves onto the true clock. A stored correction applies from the first second.
- No surface hands a clock to the live engine. Once the context carries the clock, `LiveAnalyzer::set_rate_clock` goes on the app's banned list.
- One named exception: `regress` reads on the sound card's clock to match its references (`EntrySettings::card_clock`).

---

## 6. Decided

- **History follows the newest engine.** After an update, a kept reading is read again when its take or its watch is opened, never in one sweep. Until then the row shows which version read it.
- **The stream engine is the record.** Live and kept figures can differ until #512 gives one engine.
- **One break rule, on the sound's own clock.** A step of 0.5 s or more between the sound card and the true clock is a break, live and saved. Microphone blocks that arrive late with no sound lost are not a break.
- **A changed meaning takes a new schema version.** A JSON field whose meaning moves to core's rule follows `docs/agent-interface.md`: additive, or a new version, never a silent change.

---

## 7. Do and don't

Examples from the code at 440da82 (2026-10-28).

- **Don't: two ways a break happens.** While listening, the app's `SLEPT_S` calls a gap of more than 5 s between microphone blocks a break. Reopened, core's `breaks::MIN_GAP_S` (0.5 s) decides. A 1 s step is a break only after reopening.
- **Don't: a default typed twice.** The app's Power Reserve pane and `record` each assume 18,000 bph when the rate isn't known.
- **Don't: a core number typed into words.** The app's help says "Settle (20 s unless set)". Change `take::SETTLE_S` and the help is wrong.
- **Don't: a second answer to "which beat error".** The Watches reading keeps the unlock figure only, so it can be blank where `session` shows the drop figure.
- **Don't: a copy inside core.** `quality > 0.4` is still typed in several core files beside `Beat::is_good` (#506).
- **Do: ask core's rule.** The paper strip picks its beats with `Beat::is_good` (#582).
- **Do: one path for every surface.** `regress` reads each position through `take::measure_entry`, the same path `session` uses.
- **Do: draw what core gives.** The MCP strip takes its amplitude from `amplitude::mean_of_sides`.

---

## 8. The core call behind each figure

| Figure or rule | Core call |
|---|---|
| Open a take, read a position | `take::open_entry`, `take::measure_entry` |
| The stretch measured | `take::measured`, `take::SETTLE_S` |
| The clock | `take::clock_from`, `take::clock` |
| Breaks | `take::breaks`, `breaks::MIN_GAP_S` |
| Good beat | `Beat::is_good` |
| Beat error shown | `Measurement::beat_error` |
| Defaults | `take::DEFAULT_LIFT_DEG`, `take::ESCAPE_TEETH` |
| Distribution | `distribution::distributions` |
| Power Reserve hours | `power_reserve::hours` |
| No regular beat | `session::no_regular_beat_in` |
| Take line, sizes, lengths | `library::Take::facts`, `library::size_text`, `library::length_text`, `timefreq::size_number` |

Still to build in core: the take context and its revision (#294), one writer per take file, caveat words and display rules (#510), true-time Power Reserve hours, the live break rule, the step-change mark from Steadiness's probability, and a per-cycle rate for the long-run report. Each surface switches to the call once it exists.

---

## 9. Guards

| # | Catches | Check |
|---|---|---|
| G1 | A surface calling a core building block | The `disallowed-methods` list in each surface's `clippy.toml`. A PR that switches a surface to a core call adds the building blocks it replaces. |
| G2 | Same take, different figure | Comparison tests in CI: the app against `session` on every corpus take, the MCP server against `session`, live-fed against saved, `series` and `long` against `session`, and synthetic edge takes. |
| G3 | A known difference added quietly | The KNOWN list in the app's corpus test may only shrink. |
| G4 | Figures kept and never read again | Every kept figure file carries the software version and the revision; a test changes each and checks the figure is read again. |
| G5 | Two kinds of seconds | Hours and run times come only from core, on true time; a long test take with a pause has hours that match `session`. |
| G6 | Which beat error | One accessor; a test take where the unlock is never found. |
| G7 | Caches cleared by hand | Every cache keys on the revision; a headless app test changes lift, clock and a mask and checks every pane's figure moves. |
| G8 | Silent fallbacks | A fallback that changes a figure adds a caveat; a test with an unreadable `session.json` gets it. |
| G9 | Several writers of one file | One core writer per take file, with a round-trip test. |
| G10 | Rules restated in words | A test over help and hint strings fails on a number with a unit typed in a surface. |
| G11 | Copied defaults and thresholds | A CI count of number comparisons and unround literals in surface code that may not rise; the three `clippy.toml` lists stay identical. |
| G12 | A second way a thing can happen | A code-path review of every PR that changes how a figure is made (section 10). |

---

## 10. Figure review

- **Which PRs:** every PR that changes how a figure is read, kept, cached, worded or written. A PR that also changes what the app shows gets its walkthrough too.
- **When:** the reviewer gets the branch at push and reviews while CI runs.
- **What:** for each figure the PR touches, the path is traced at the head and at main, and each rule removed and added is listed. The comparison tests run, and the CLI is compared with the app or MCP on a corpus take and on the edge takes that bear on the change.
- **Verdict:** CLEAR or HOLD, with one line: the second rule removed, none added, the surfaces agree on which takes, and what was run versus only read.
- No PR of this kind merges without a CLEAR, and a read-only CLEAR does not count for a PR that changes a figure.

---

## 11. Before a PR that touches a figure

- [ ] Every figure the PR shows or changes names its core call (section 8).
- [ ] No reading compared with a number, and no new number literal, outside core.
- [ ] No second way a thing happens: live and reopened, app, CLI and MCP share the call.
- [ ] Readings come from the take's context; no surface picks a clock.
- [ ] A kept figure carries the software version and the revision.
- [ ] Every cache keys on the revision.
- [ ] A fallback that changes a figure adds a caveat.
- [ ] No take file is written outside core's writer.
- [ ] No core number is typed into words.
- [ ] A surface that switches to a core call bans the building blocks in its `clippy.toml`.
- [ ] The KNOWN list has not grown.
- [ ] No function over 100 lines or complexity 25, and no file over 3,000 lines, is added or grown, except a fix's mark that names an issue (section 12).
- [ ] A split PR only moves code and adds no copy of data.
- [ ] The figure review is CLEAR.

---

## 12. Code shape

A long function is where a second rule hides: of the 23 longest app functions on 2026-10-29, 6 held a rule core should own. Shape is part of one source of truth.

### Limits

- **A function:** at most 100 lines and a cognitive complexity of at most 25, as Clippy counts them (`too_many_lines`, `cognitive_complexity`).
- **A file:** at most 3,000 lines. A file over that today may only shrink.
- **One file, one concern:** a pane, a sheet, a command or a core module.
- **Today's long functions** each carry `#[expect(clippy::too_many_lines)]` or `#[expect(clippy::cognitive_complexity)]`. Once a split makes the mark untrue, Clippy fails until it is removed, so the list only shrinks.
- **A fix may add a mark** when it tips a function over a limit, with a reason that names an issue. The fix lands first, and the split follows in the next PR. The hygiene audit counts open marks.

### What is split, and when

- **Now:** the 23 longest functions, and first the ones that hid a rule.
- **The rest:** when a PR already changes them.
- **`app.rs` by pane:** all at once, in one short window with no Desktop PR in flight, one pane per PR, each merged before the next. Never piece by piece alongside other work.

### Splitting a function

- **Rules first.** A rule the function hides moves to core in its own PR, and the split follows.
- **A split only moves code.** Existing tests pass unchanged, screenshots match pixel for pixel, and the CLI's JSON is identical. Each move goes in `.git-blame-ignore-revs`.
- **No new copies of data.** A piece takes only the fields it needs, never the whole app, so nothing is copied to satisfy the borrow checker. A split adds no copy of beats, readings or sound. The strip, chart and Steadiness splits time a frame on a 26 h take before and after.
- **The PR body gives the numbers:** lines and complexity per function, before and after.
- A split may reset the saved pane layout once. This is accepted while the app has no public release.

### Adjacent hygiene

- **Copy-paste:** the count of copied product blocks may only go down.
- **Dead code:** a public item with no caller outside its tests is removed, judged on all three platforms.
- **Tests:** no crate's product coverage drops, and a new product file is covered at least 50%. A change to a core reading file runs mutation testing on what changed.
- **Dependencies:** no unused dependency. Every library's licence is on the list that fits GPL-3.0. A vulnerability our code reaches is fixed at once.
- **Comments:** a comment that no longer matches the code is a defect.

### Enforced by

| Rule | Check | Where |
|---|---|---|
| Function length and complexity | Clippy, with today's long functions marked | CI |
| File length | The G11 count script lists files over 3,000 lines, which may not grow | CI |
| Copy-paste | jscpd's product block count against a checked-in number | CI |
| Unused dependencies, licences | `cargo machete`; `cargo deny` licence and bans checks | CI |
| Coverage, mutation testing, advisories, dead public items, open marks | Trends against the baseline | Hygiene audit |
| A split moves code only and adds no copies | Code-path review, screenshots, JSON comparison | Review |
