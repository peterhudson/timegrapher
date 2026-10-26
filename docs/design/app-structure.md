# Timegrapher App Structure

Version 1, approved 2026-10-25. This is a peer of the [Design Guide](design-guide.md). The guide says how every part of the app looks and behaves. This document says what the parts are, how they nest, and how a user moves between them. Where today's app differs, the app changes. A case that isn't covered here is raised as an issue, and its answer is added here.

**Decided:** Watches is a view of the window, not a pane and not a dialog. A watch's takes do not appear on the Bench in the first version.

---

## 1. The app's two jobs

The app serves two first-class citizens, and its structure follows them:

1. **Quick Listen**, the bench instrument. Open the app, a watch is on the mic, and readings appear: one press, or none with Listen When the App Opens. Everything is about one take.
2. **Watches**, the collector's and watchmaker's history. Define a watch, record takes into it, find them again, compare them, file the ones made without a watch.

Audio files and folders of recordings are second class. They live in the File menu only.

## 2. The levels

From the top down, each level holds the next:

1. **The window.** One menu bar, one mode row, one status bar.
2. **Two views:** **Bench** and **Watches**. They take turns filling the window between the mode switch and the status bar.
3. **Panes.** The readings cards, the Paper Strip, Profile and Scope, the analysis panes and the tests. The user places them, and the arrangement is remembered between sessions. Each pane shows one take, and the Bench has one take at a time.
4. **The take.** Live or opened, it's what every pane on the Bench shows.

Watches sits at level 2 because it's about many takes and many watches, not one take. Giving it a pane on the Bench would put it at level 3, beside things that show a single take. Giving it a dialog would put it outside the window, as if it were a setting.

## 3. The views

### The mode switch

- A two-way segmented control, **Bench | Watches**, at the left of the mode row, under the menu and icon bar (guide, section 2), in every size down to 800 × 600. The rest of the row holds the current view's toolbar.
- ⌘1 (Ctrl+1) shows the Bench and ⌘2 (Ctrl+2) shows Watches. The View menu lists both at its top with those keys. Keys are covered in section 7.
- The Watches icon leaves the pane icon bar, and Watches leaves the pane list in the View menu.
- The app always opens on the Bench.

### What carries across both views

- **The status bar.** It is one line, as the guide sets out. It shows the take's state in both views, so a running take stays in sight while you browse: "Quick Listen · Listening · 12:31". It shows no readings in either view: readings live on the cards, and the cards are one ⌘1 away.
- **The take.** Switching views never stops, pauses or saves a take. Listening, a Positions Test and a Power Reserve Test keep running behind the Watches view.
- **The menus.** File, Edit, Watches and Help are the same in both views. Actions that need the Bench, such as the Paper Strip's commands, are dimmed in Watches, with a hint saying so.

### When the window switches by itself

The window switches to the Bench when an action in Watches has a result there, and nowhere else:

| In the Watches view | The window then shows |
|---|---|
| Open (or a double-click, or Return) on a take | The Bench, with that take open |
| Listen, Positions Test or Power Reserve Test in a watch's New Take group | The Bench, with that watch set and the take started |
| Find in Watches… (the last item of the Watch line's menu on the Calibre card, and also in the menu bar's Watches menu) | Watches, with the search field ready |

Nothing else switches views. Picking a watch in the list, editing it, or filing a take leaves you where you are.

### When a take is already running

The Bench holds one take. When Watches would start or open another:

| What's running on the Bench | Open on a take, or New Take |
|---|---|
| Nothing, or an opened take | Works as above. |
| A Quick Listen take | Works, after today's question about unsaved sound (Keep, Discard, Cancel). The Quick Listen take stops, then the new one opens or starts. |
| A watch's take, a Positions Test or a Power Reserve Test | Disabled, with the hint "Stop the take on the Bench first." Nothing is stopped from the Watches view. |

Picking a row in the list only selects it and shows its details. It never changes the watch on the mic, so browsing during a take can't change that take's watch.

## 4. The Watches view

![The Watches view filling the window, marks 1 and 2](structure/watches-view.png)

- **(1)** The mode switch, showing Watches.
- **(2)** The status bar, still showing the take running on the Bench.

The view's content is the redesign in the guide's addendum, "The guide applied to Watches":

- the list beside the watch;
- the New Take group;
- the takes table, with Open and "⋯" (it comes before Details, mark 11);
- Details and Photos;
- List Shows, and search that shows which field matched;
- Edit Watch.

The whole window gives it room. At 2560 × 1440 and 1280 × 820, the list and the watch sit side by side. At 800 × 600 the view shows one at a time, with "‹ All Watches".

The view's toolbar sits on the mode row, after the switch: New Watch…, the search field, Sort, List Shows and "?". It is the Watches view's equivalent of a pane toolbar, and the guide's toolbar zones apply.

## 5. The Bench

![The Calibre card with its Watch line, in three states, marks 3 to 5](structure/bench-watch-card.png)

The Calibre card keeps its name, its 134 pt height and its three rows (guide, section 10). The Watch line takes the title row:

- **(3)** The **Watch line** is the card's title: the watch on the mic, or "No Watch (Quick Listen)", with ▾. It is the one place on the Bench that says which watch a take belongs to. The figure row shows the calibre, and the controls row keeps Lift at the left and Beat Rate at the right, as in #432. Beat Rate still hides first when the card is narrow.
- **(4)** What the rows hold depends on the watch:
  - **A watch with a calibre** (left): calibre, lift and beat rate are the watch's, shown as text, not editable here (the either/or rule, guide addendum mark 43).
  - **A watch with no calibre** (middle), or a calibre whose table entry lacks a figure: that figure is a field here, as in Edit Watch. A change is saved to the watch, and its hint says so: "Saved to Seiko 6R15 Diver."
  - **No Watch** (right): the Calibre pop-up, Lift and Beat Rate work as today.
  - In every state, #432's rule stands: when the heard beat rate differs from the set one, the heard one is kept for the take, as today.
- The card is still the **Calibre card**: its hover and its "?" are titled "Calibre", and the "?" says the title picks the watch and the figure is its calibre.
- When the card is too narrow, Beat Rate hides first, as in #432. For a watch with no calibre, the beat rates head the title's menu, under "Beat Rate (hidden at this width)", just as they head the Calibre menu today. A change is saved to the watch. With No Watch they stay at the head of the Calibre pop-up, as in #432. The current rate has a ✓, as the watch on the mic does in the Recent list (blue is for the row under the pointer only), and the name in the title is cut before its ▾, never the ▾ itself.
- **(5)** The title's menu, in this order:
  1. No Watch (Quick Listen)
  2. Recent watches, up to five, most recent first
  3. New Watch…
  4. Find in Watches…

At narrow widths the Watch line shortens to the watch's name, cut with "…", and "No Watch (Quick Listen)" shortens to "No Watch". It never leaves the card, because without it a take could go into the wrong watch.

The card's width follows today's readings-card rules: 250 pt from about 1000 pt of window up (it never grows; the reading cards take the extra width), and 130 pt at 800 × 600, where its title steps down to 12 pt. Every state at each size:

![The Calibre card in its three states at 800, 1000 and 2560 wide, and its title menu](structure/calibre-card-states.png)

**Picking from the Watch line when a take is on show.** The card shows one take's figures, so picking a watch, or No Watch, puts the take on show away first:

- **Listening, or a test running:** the menu's other rows are greyed, and its top line says once: "Stop listening to pick another watch." Nothing is stopped from the menu.
- **A stopped take with no unsaved sound** (a watch's take, saved on Stop, or a take opened from disk): it is put away without a question.
- **A stopped take with unsaved sound** (a Quick Listen take not yet saved): today's unsaved-sound question comes first, exactly as for New Take, with Cancel leaving everything as it was. The question uses today's threshold, so every place that asks behaves the same: more than 5 s of unsaved sound asks, and less is put away silently, since it holds no reading.
- Then the picked watch goes on the mic, with its calibre, lift and beat rate on the card. A watch made with the menu's New Watch… goes on the mic the same way. Its order is New Watch sheet first, then the unsaved-sound question after Add Watch. Cancel on the question keeps the take on show, and the new watch exists but isn't picked. The sound is never put at risk for a sheet the user might still cancel.
- **Recent** in the Watch line's menu lists up to five watches by their newest take, newest first. This is the same order as the Watches list's Last Take sort, so the two never disagree. Picking a watch without taking a take doesn't reorder it. A pick is almost always followed by a take, so the two orders soon agree, and this needs no stored pick history.

Card order stays as decided: Position, Calibre (now with its Watch line), Listen, Rate, Amplitude, Beat Error.

## 6. A take's life, and where each step happens

| Step | Where | How |
|---|---|---|
| Pick the watch on the mic | Bench | The Watch line's menu. Refused while listening, as today. |
| Pick it while browsing | Watches | Its New Take group starts the take and switches to the Bench (section 3, "When a take is already running"). Picking the row alone changes nothing on the Bench. |
| Listen with no watch | Bench | Listen on the Listen card (Quick Listen). |
| Keep a Quick Listen take | Bench | Save on the Listen card: Keep in a Watch ▸ (recent watches, then New Watch…), Save in a Folder…, or Save…, which files it in Unsorted. |
| A test ends with no watch | Bench | As today: Keep in Unsorted (the default) or throw it away. |
| A watch's take ends | Bench | Saved into the watch on Stop, with no question, as today. |
| File takes from Unsorted | Watches | Unsorted Takes in the list; "⋯" > File Under a Watch ▸. |
| Find an old take | Watches | Search, sort, then Open. The window switches to the Bench. |
| Compare a watch's takes | Watches | The takes table, sorted by any column. |
| Open a recording from outside | File menu | As today: Add to a Watch or Just Open It, then the Bench. |

## 7. What changes in the app

For the Desktop app:

1. The window gets the mode switch and a second top-level view. The status bar is shared by both views.
2. The Watches pane leaves the pane system. Its icon leaves the icon bar, and its entry leaves the View menu's pane list.
3. The Watches view uses the guide's addendum design, laid out for the whole window.
4. The Calibre card gains the Watch line and its menu.
5. Keep in a Watch on the Listen card's Save menu lists recent watches first.
6. The auto-switches in section 3, and nothing else, with the running-take rules beside them.
7. **The menus:**
   - The View menu starts with Bench (⌘1) and Watches (⌘2), then the panes. Watches leaves its pane list.
   - The menu bar's Watches menu drops Show/Hide Watches and its list of every watch (the Watch line's menu already lists recent ones). It also drops Quick Listen (No Watch): the Watch line's menu starts with No Watch (Quick Listen). It keeps New Watch… and Find in Watches…, and its hints stop saying "in the Watches pane". Going to Watches is View > Watches (⌘2) only, so the command lives in one place.
   - **In the Watches view, the icon bar keeps everything that isn't a pane:** the Settings gear stays, in exactly the place it has on the Bench. Only the pane icons (Readings to Tests, and their group labels) are hidden, and their space stays empty, so the gear never moves when the view changes. The menus stay, with Bench-only items dimmed. Any icon added later that isn't a pane (Help, for example) follows the gear.
   - The app always opens on the Bench.
8. **Keys:**
   - The position keys 1 to 6 work only as bare digits: never with ⌘, Ctrl, Alt or Shift held. Today they ignore modifiers (`app.rs` ~13958), so ⌘1 would also move the watch to DU.
   - They are off in the Watches view and while any text field has focus.
   - ⌘1 and ⌘2 pick the view. ⌘⇧1 to ⌘⇧3 stay free (the Bench layouts are parked).
   - A test proves ⌘1 changes the view and not the position.
9. docs/app.md: the Watches pane section becomes the Watches view, and Quick Listen's description drops "shows Unsorted Takes in the Watches pane".

Unchanged: the library on disk, `watch.toml`, take folders, Unsorted, the CLI and MCP, and every action listed in the addendum's table.

### The Watches view, drawn for each size (Watches PR C)

These mock-ups are the build target for the Watches view. They follow the guide's addendum, with one change of order, explained at mark 11 (approved 2026-10-25). The marks run from 6, after §5's 3 to 5.

**At 2560 × 1440, dark**

![The Watches view at 2560 × 1440, dark, marks 6 to 12](structure/wv-2560-dark.png)

- **(6) The view's toolbar.** It sits on the mode row after the switch and a separator: New Watch…, the search field, then Sort ▾ and List Shows ▾, each labelled.
- **(7) "?"** sits at the right end of the mode row, in the Actions zone. It is the view's help, and it is the only thing at that end.
- **(8) The Settings gear** stays at the same x as on the Bench. The pane icons' room to its left stays empty (§7, menus).
- **(9) Edit… and Add Photos…** are word buttons at the right of the watch's header. The header's caption carries the calibre, beat rate, lift and the fields List Shows picks, once.
- **(10) New Take** has three equal buttons with captions. Listen is the default and is blue. **Positions Test** and **Power Reserve Test** are the entry points for the two tests. Each one switches to the Bench and starts there (§3).
- **(11) Takes come straight after New Take,** above Details and Photos. This changes the addendum's order. A watch's history is what the view is mostly opened for, so it should not sit below the fold at 1000 × 700. Details and Photos follow, and scroll.
- **(12) Open and "⋯"** sit at the right of the Takes title and act on the picked row.
- **Width.** The watch's content stops at 1100 pt and doesn't stretch to fill a 2560 window. A wider table puts numbers too far from their dates. Approved as mocked (2026-10-25); the empty space to the right at 2560 may be revisited later, with the readings cards' width.

**While a take runs, 2560 × 1440, light**

![The Watches view while a take runs, light, marks 13 to 15](structure/wv-2560-light-running.png)

- **(13) While a watch's take or a test runs,** the three New Take buttons stay in place, greyed out. The New Take title row says, at its right, "Stop the take on the Bench first." That row is always there, so nothing moves when the hint appears. Each button's hover says the same.
- **(14) Open is greyed out** too, with the same hover. "⋯" stays live, but its Open item is greyed out. Show in Finder and the rest still work on takes already saved.
- **(15) The status bar** shows the running take: the green Live dot, then the watch, the kind of take, Listening and the time. It is the same line as on the Bench (§3).
- A live **Quick Listen** take doesn't grey anything. Open and New Take ask today's unsaved-sound question, then stop it (§3).

**At 1000 × 700, dark**

![The Watches view at 1000 × 700, dark, marks 16 and 17](structure/wv-1000-dark.png)

- **(16)** The whole toolbar still fits, with the search field at 200 pt. The list is 280 pt, and the watch takes the rest.
- **(17)** The takes table keeps all seven columns, with 6 pt cell padding. Details and Photos are below it, reached by scrolling the watch side only. The list and the toolbar never scroll.

**At 800 × 600, light: the list, searching**

![The Watches view at 800 × 600, light, the list while searching, marks 18 and 19](structure/wv-800-light-list.png)

- **(18)** Below about 900 pt of window, Sort and List Shows fold into "⋯", in that order. New Watch…, the search field and "?" never fold.
- **(19)** The list fills the view. A search shows "1 of 3 watches" (Unsorted Takes is not a watch, so it is not counted) under the matches, and the matching text in the accent, as in the addendum (mark 41).

**At 800 × 600, dark: a watch**

![The Watches view at 800 × 600, dark, a watch, mark 20](structure/wv-800-dark-watch.png)

- **(20) ‹ All Watches** shows only at this size, where the list and the watch can't sit side by side. It is the first line of the watch's side, above the header, and it goes back to the list with the search kept.

## 8. Open questions

- **Bench layouts** (Live, Positions, Analysis): parked (2026-10-25). Panes stay placed by hand.
- **A watch's takes on the Bench**, as an optional pane listing the open take's siblings: left out of the first version. Revisit after use.
