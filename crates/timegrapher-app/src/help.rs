//! The longer explanations behind the "?" buttons, one per part of the
//! window, so they read alike and live in one place.

use std::sync::LazyLock;
use timegrapher_core::beats::NO_WATCH_JITTER_FRACTION;
use timegrapher_core::distribution::{self, STRETCH_S};
use timegrapher_core::live::MIN_SNR;
use timegrapher_core::measure::{RATE_SE_MIN_S, RATE_WINDOW_S};
use timegrapher_core::steadiness::{ISO_BIN_DEG, ISO_MIN_BINS, ISO_MIN_READINGS};
use timegrapher_core::take::{DEFAULT_LIFT_DEG, SETTLE_S};

/// Lines that state a number core or the app sets, worded once with it,
/// for the "?" that takes plain text.
fn worded(lines: Vec<String>) -> Vec<&'static str> {
    lines.into_iter().map(|p| &*p.leak()).collect()
}

/// A share as the percent the help states it in.
fn percent(share: f64) -> f64 {
    share * 100.0
}

/// How one line of a "?" text is drawn (#1520). The first line is the
/// answer, which hovering shows alone; after it, the line's start sets its
/// kind: `## ` a group's heading, `- ` a bullet, `1. ` a step, anything
/// else a short paragraph.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Line<'a> {
    Answer(&'a str),
    Heading(&'a str),
    Bullet(&'a str),
    /// The step's number with its point ("1."), then its words.
    Step(&'a str, &'a str),
    Paragraph(&'a str),
}

/// The kind of the `i`th line of a "?" text, and its words.
pub fn line(i: usize, text: &str) -> Line<'_> {
    if i == 0 {
        return Line::Answer(text);
    }
    if let Some(t) = text.strip_prefix("## ") {
        return Line::Heading(t);
    }
    if let Some(t) = text.strip_prefix("- ") {
        return Line::Bullet(t);
    }
    let digits = text.bytes().take_while(u8::is_ascii_digit).count();
    if digits > 0 && text[digits..].starts_with(". ") {
        return Line::Step(&text[..=digits], &text[digits + 2..]);
    }
    Line::Paragraph(text)
}

pub const MICROPHONE: &[&str] = &[
    "Which microphone the app listens on, how loud it is set and how loud the beats arrive.",
    "## Finding Microphones",
    "- The app looks for microphones only when the microphone menu opens, Rescan is pressed \
     or listening starts.",
    "- At launch it looks only when Listen When the App Opens is on (Settings > General). \
     Asking an input what it supports briefly opens it, which could disturb another program \
     recording from it.",
    "## Controls",
    "- Input Level: the microphone's gain. For the system default input it is the sound \
     server's input volume, which it puts back on the microphone every time it opens it.",
    "- For a direct device, Input Level is the card's own capture level.",
    "## What It Shows",
    "- The meter has plain marks at -10 and -6 dBFS for reference. At 0 dBFS the sound \
     clips, and the words beside it say so.",
    "- The Peak meter shows the loudest sample in the last half second, in dBFS (decibels \
     below the most the input can carry). The words beside it say when the sound clips.",
    "## Quartz Correction",
    "- Quartz Correction lists the clock error kept for each input, by the name the system \
     gives it, with Forget beside each.",
    "- The Rate card shows a rate read on that input corrected as True Clock.",
    "- Each input keeps its own, so changing the microphone changes the correction used.",
];

pub static RATE: LazyLock<Vec<&'static str>> = LazyLock::new(|| {
    worded(vec![
        "How many seconds a day the watch would gain (+) or lose (−) if it kept running \
     exactly as it did over the beats the reading was fitted to."
            .into(),
        "## What It Shows".into(),
        "- It comes from a straight line fitted to the time of every beat. A watch on time \
     beats exactly as often as its beat rate says, a gaining one a little more often."
            .into(),
        "- The fit takes the tick and the tock together, so beat error doesn't move it.".into(),
        "- Under it: how many beats the reading used over how many seconds, and the jitter."
            .into(),
        "- The jitter is how far single beats land from that straight line, as a robust \
     standard deviation in microseconds (millionths of a second)."
            .into(),
        "- The jitter is the spread of the dots across the paper strip; smaller means less \
     scatter. tg doesn't show it."
            .into(),
        "- Background noise, a muffled sound and the stand add to it as well as the watch, \
     so figures from different stands and microphones differ."
            .into(),
        "## The ± Figure".into(),
        "- The ± is roughly how far the rate could be off from the scatter of the beats \
     alone. It shrinks with a longer averaging time."
            .into(),
        format!(
            "- The ± is how far the figure could be off from the scatter of {RATE_WINDOW_S:.0}-second \
     rates over the run so far: a property of the measurement, not of the watch."
        ),
        format!(
            "- It needs about {RATE_SE_MIN_S:.0} s of beats in the take; a shorter reading \
     takes the scatter from the beats nearest it."
        ),
        "- Anything the ± can't show (a short stretch, missing beats) is named beside the \
     title."
            .into(),
        format!(
            "- Stopped after a long run, a line under the figure gives the whole take as the \
     command line reads it, with the 5th to 95th percentile of its {RATE_WINDOW_S:.0}-second \
     rates."
        ),
        "## True Clock".into(),
        "- True Clock, when shown, is the rate corrected for the sound card's own clock. A \
     card's crystal runs a steady 10 to 50 parts per million off."
            .into(),
        "- That moves every rate read on it by 1 to 4 seconds a day. The app times the \
     card's clock against the computer's while it listens."
            .into(),
        "- After 20 minutes, once the timing is known to half a part per million, Settings \
     > Microphone > Quartz Correction offers the result with a Keep button."
            .into(),
        "- A busy computer delivers sound less evenly, so the offer can come later. The \
     result is only right if the computer's clock is kept by internet time."
            .into(),
        "- Once the error is kept (by Keep, or from the command line, which can also measure \
     it from a recording's clock log), the corrected rate is in the big figure's hover."
            .into(),
        "- Settings > Microphone > Quartz Correction lists each input's. Until then the rate \
     is read on the card's clock alone, as tg reads it."
            .into(),
        "## Reading It".into(),
        "- The big figure stays on the card's clock, the same clock tg and an analysis of \
     the recording read, so the two compare directly."
            .into(),
    ])
});

/// The rest of a "Clipped" chip's hover.
pub const CLIPPING: &str = "The loudest part of each tick reaches the top of the input's \
     range and is cut flat. That moves where the drop seems to be, so the amplitude and \
     the beat error read wrong; the rate comes from when the beats arrive, which clipping \
     doesn't change. Input Level is in Settings > Microphone.";

pub const AMPLITUDE: &[&str] = &[
    "How far the balance swings each way from rest, in degrees, averaged over the ticks and \
     tocks of the averaging time.",
    "## What It Shows",
    "- It is worked out from the time between the unlock and the drop of each beat (the \
     solid Unlock and dashed Drop lines on the Tick Tock Profile) and the lift angle.",
    "- So it is only as right as the lift angle, the Lift on the Calibre card.",
    "- Tick and Tock are the amplitude measured from each kind of beat alone.",
    "## The ± Figure",
    "- The ± is roughly how far the amplitude could be off from the scatter of its 2-second \
     pieces alone. It is precision, not accuracy.",
    "- The amplitude moves with the lift angle in proportion: 1° of lift moves it about 5° \
     at 260° with a 52° lift, less at lower amplitudes.",
    "- So a lift angle that is a little out matters more.",
    "- Anything the ± can't show (a lift angle assumed, clipped beats, a short stretch) is \
     named beside the title.",
];

pub const BEAT_ERROR: &[&str] = &[
    "How unevenly the tick and the tock are spaced, in milliseconds: zero when the balance's \
     rest point sits exactly midway between the pallet stones' lifting.",
    "## What It Shows",
    "- The big figure is timed from the unlock, the first sound of each beat, as tg \
     measures it (checked against tg in the regression tests).",
    "- The smaller one is timed from the beat as a whole, nearer the drop. It is the gap \
     between the two lines of dots on the strip.",
    "## Reading It",
    "- It is shown as a size. Which beat is the tick depends on where the analysis first \
     locks on, so which side is longer can flip from one run to the next.",
    "- The ± is the scatter of its 2-second pieces. Anything it can't show (clipped beats, \
     a short stretch) is named beside the title.",
];

pub static POSITION: LazyLock<Vec<&'static str>> = LazyLock::new(|| {
    worded(vec![
        "The position the watch is in on the stand: Dial Up, Dial Down, or standing on its \
     edge with the crown up, down, left or right."
            .into(),
        "## What It Shows".into(),
        "- The picture shows the watch as it lies, the hour that points up at the top. \
     Beside it are its name and codes, the common one then Witschi's (CD · 9H)."
            .into(),
        "- The 6 | 10 in the title adds a second row: the four stops halfway between the \
     standing ones, 1:30 Up (1H30), 4:30 Up, 7:30 Up and 10:30 Up."
            .into(),
        "- The second row is for a stand that stops every 45°, as a Dynamic Poise test turns \
     the watch."
            .into(),
        "## Controls".into(),
        "- While you listen, the six buttons DU, DD, CU, CD, CL and CR set it (on a narrow \
     window, one pop-up of them)."
            .into(),
        "- Looking at a take, or during a Positions Test or a Power Reserve Test, the buttons \
     only show the position: the one at the strip's latest beat on show, or the one the \
     test set."
            .into(),
        "- To correct a stretch, use the Paper Strip's Position Brush. Removing the painted \
     stretch reveals its recorded position."
            .into(),
        "- The brush is unavailable during a Power Reserve Test and during or after a \
     Positions Test."
            .into(),
        "## Keys".into(),
        "- Each position has a key, on the Bench and not while typing: U and D lying.".into(),
        "- Standing, the key is the last digit of the hour at the top: 3, 6, 9 and 2 for 3H, \
     6H, 9H and 12H, and 1, 4, 7 and 0 for the 45° stops."
            .into(),
        "- A 45° stop's key, or one painted or on show, turns the card to 10. Each button's \
     hover gives its key."
            .into(),
        "## Changing It".into(),
        "- Changing it while listening marks a new stretch on the strip and the charts. The \
     readings, profile, distributions and Steadiness then use only this position's beats."
            .into(),
        format!(
            "- The settling after a move, the Positions Test pane's Settle ({SETTLE_S:.0} s \
     unless set), is left out of the readings. A saved take notes each position."
        ),
    ])
});

pub const LISTEN: &[&str] = &[
    "Listen starts the microphone; Stop lets go of it and keeps everything on show.",
    "## Controls",
    "- Listen with a watch picked in Watches: a new take of it, saved into it as it goes.",
    "- Listen with none, or Unsorted Takes picked: Quick Listen, kept only if you Save it, \
     and from then saved as it goes.",
    "- A watch picked from Save or the Watch line while Quick Listen listens takes the take. \
     Listening goes on, and it is saved into that watch as it goes.",
    "- Save: keeps what is on show in one of your watches or in a folder.",
    "- Average: how many seconds of beats each reading is worked out over. Longer is \
     steadier, shorter follows changes sooner.",
];

pub static CALIBRE: LazyLock<Vec<&'static str>> = LazyLock::new(|| {
    worded(vec![
        "What the readings are worked out for: the watch's calibre, its beat rate and its \
     lift angle, the three in one place."
            .into(),
        "## Controls".into(),
        "- The title is the watch on the microphone, or No Watch (Quick Listen). Click it to \
     pick another, add one, or find one in Watches; the figure under it is its calibre."
            .into(),
        "- Calibre: pick it from the table and it sets the beat rate and the lift angle, \
     which then show here as text."
            .into(),
        "- Beat Rate: with no calibre set it is Auto, the standard rate nearest to the beats \
     of the first few seconds, shown beside it."
            .into(),
        format!("- Lift: with no calibre set, Lift is the default {DEFAULT_LIFT_DEG:.0}°."),
        "- Set either here, for a watch that beats at an unusual rate or runs far off, or \
     whose lift angle you know. For a watch with no calibre, what you set here is saved to \
     the watch."
            .into(),
        "## Words".into(),
        "- A beat rate is how many beats the watch makes an hour, ticks and tocks counted \
     alike."
            .into(),
        "- 28,800 is a balance swinging 4 times a second each way, 21,600 is 3, and 18,000 \
     is 2.5."
            .into(),
        "- The lift angle is what Amplitude is worked out from.".into(),
    ])
});

/// The group every pane toolbar's "?" ends with: how its icons work.
pub const TOOLBAR: &[&str] = &[
    "## The Toolbar",
    "- Hovering an icon names it, with what it does, and the key below draws each one beside \
     its name.",
    "- A switch stays pressed and tinted while it is on. A choice of two shows the one in \
     use, and a click changes it.",
    "- Of a row of icons, the tinted one is picked. A setting that holds a value shows its \
     name in words before its field.",
    "- When the pane is too narrow for them all, the last ones fold into the … menu at the \
     row's end, in words. They come back out when the pane is wide enough.",
];

pub const STRIP: &[&str] = &[
    "One dot for every beat, tick and tock in their colours, as a printing timegrapher draws \
     it on paper.",
    "## What It Shows",
    "- Along the strip is time; across it is how early or late each beat came.",
    "- Early and late are measured against a perfect clock beating exactly at the beat rate \
     (every 125 ms at 28,800 bph). Witschi and other timegraphers use the same words.",
    "- A beat that comes before that clock's beat is early; after it, late. A watch on time \
     keeps the same distance, so its dots run parallel to the edges.",
    "- A gaining watch beats a little early each time, so its dots drift steadily towards \
     the early side; a losing one towards late. The steeper the drift, the bigger the rate.",
    "- The ticks and the tocks make two lines; the gap between them is the beat error. A \
     line that runs off one edge comes back on the other, so the width is a zoom.",
    "- The track along the top (down the right when the strip runs down) shows each position \
     in teal, solid while the watch settles after a change, then fading. Masked stretches \
     are grey on it.",
    "## Lines Over It",
    "- The magenta rate line is the Rate reading drawn over the beats it was fitted to: if \
     the dots follow it, the reading describes them well.",
    "- The faint magenta lines beside it (Parallel Guides) run at the same slope across the \
     whole strip, one per grid step.",
    "- They show at a glance whether the dots everywhere run parallel to the rate or bend \
     away from it. Each can be shown without the other.",
    "- Amplitude, rate and beat error can be drawn over the strip as lines against the same \
     time, each on its own scale.",
    "- The scale's ends are written in the line's colour above the strip (or beside it when \
     the strip lies across).",
    "- Seeing amplitude beside the dots shows at once whether a wobble in the rate comes \
     with a change in amplitude.",
    "## Mouse",
    "- The wheel or a swipe moves along the time, as dragging along it does.",
    "- Cmd or Ctrl and the wheel (or a pinch) changes the length, out to the whole take. \
     Live, the newest beat stays at the edge; looking back, the time under the pointer stays \
     put.",
    "- Alt (Option on a Mac) and the wheel changes the width.",
    "- Drag across to slide the trace; double-click for the newest beats, centred.",
    "## Still Paper",
    "- Still Paper (the sheet with a wrapping trace, after Direction) keeps the paper put \
     while the pen sweeps along it.",
    "- It starts again at the start every Length, wiping the last lap just ahead of it, as \
     a scope draws. Off, the paper slides along under the pen, the newest beats at its edge.",
    "- The time under each stretch of ink is written beside it. On still paper, dragging \
     moves the pen through time the same way it moves moving paper.",
    "## Mask",
    "- Mask leaves stretches out of the readings, such as a bump of the stand. With it on, \
     drag along the strip over the beats to mask them (the pointer is a cross).",
    "- Drag a masked stretch's edge to move it (a two-way arrow at the edge), drag with the \
     right button to look back, and click a masked stretch to bring it back.",
    "- A right click brings one back with Mask on or off. While Mask is on, dragging masks \
     instead of sliding the trace.",
    "- Its beats stay on the strip, faint under the hatching. Nothing is read inside a \
     masked stretch and no reading reaches across one.",
    "- The charts, the steadiness tests and a Positions Test leave it out too. A take saved \
     from the app keeps them, and a recording opened from disk keeps them when it is saved.",
    "- Each point on the strip's line is the reading over the Average Over that ends there. \
     Just after a masked stretch the line is blank for about one Average Over.",
    "## Position Brush",
    "- Position Brush marks a stretch as one position: pick the position from its menu, then \
     drag along the strip.",
    "- Drag a change of position along the track to move it. Esc, or Stop Painting, puts the \
     brush down.",
];

pub const PROFILE: &[&str] = &[
    "The typical sound of a tick (blue) and a tock (deep orange).",
    "## What It Shows",
    "- The line is the beats' typical loudness, the band where the middle 80% fall. Time \
     runs in ms from the drop.",
    "- On a take kept without its sound, the line is the average of the 2 s profiles kept \
     in the Average Over window and the band the range of their bands.",
    "- A line from the top is an edge: Unlock, Drop or Peak. Amplitude comes from unlock to \
     drop, the beat error from the unlocks.",
    "- A bar across the curve marks where an edge meets it; a ring on the curve is a sound: \
     Unlock, Impulse or Drop Sound.",
    "- Hover a name, line or ring for its time; on a short plot, sound names show on hover.",
    "## Facts",
    "- Each plot's top-left gives its side's facts, at the same place on both. Hover it for \
     all four when it shows fewer.",
    "- The dashed line is the noise floor.",
    "## Controls",
    "- dB Scale: shows the level in decibels below the loudest point, so the quiet unlock \
     shows.",
    "- Time From: Shared puts Tick and Tock on one clock, the drops a beat error apart; Own \
     Drop puts both drops at 0 ms.",
];

pub const CHARTS: &[&str] = &[
    "Rate, amplitude and beat error over the whole take, one chart above the other, each \
     point a reading over the averaging time.",
    "## What It Shows",
    "- The charts' time axes move together.",
    "- With the pointer over a chart, each chart's strip above its plot reads every line's \
     value at that time.",
    "## Controls",
    "- The hare, the swing and the seesaw in the toolbar show or hide the Rate, Amplitude \
     and Beat Error charts.",
    "- The charts shown share the pane's height, and at least one stays on.",
    "- Fit in the toolbar shows the whole session again.",
    "## Mouse",
    "- The wheel, a swipe or dragging pans time within the take.",
    "- Cmd or Ctrl and the wheel (or a pinch) zooms it around the pointer, out to the whole \
     session.",
    "- A double-click fits the whole session again (and follows new beats while listening).",
    "- Clicking a moment shows it on the strip and in the readings.",
];

pub const STEADINESS: &[&str] = &[
    "How the rate, the amplitude and the beat error each change over the session, in time \
     order: cycles, steps and trends.",
    "## What It Shows",
    "- The tests need about 5 minutes of beats; the command line runs the same tests.",
    "- A rate dot is a 10 s reading; an amplitude or beat error dot is a 2 s window.",
    "- The wheels' periods are dotted.",
    "## Views",
    "- Each view has its icon under Analysis.",
    "- Periods: how strongly the readings repeat.",
    "- Cycle: the readings folded at the cycle found.",
    "- FFT: the sine wave's size at each period.",
    "## Controls",
    "- Info, the i at the right, gives what the tests found and their statistics.",
];

pub const SCOPE: &[&str] = &[
    "The sound of the latest beats as it came in, high-passed so hum and rumble don't hide \
     it: what an oscilloscope on the microphone would show.",
    "## What It Shows",
    "- While listening it shows the last few seconds; stopped, or for a recording, the last \
     beats.",
    "## Controls",
    "- Span 20 ms shows one sound in detail, from before the unlock to after the drop, with \
     the Tick above the Tock and the unlock and drop marks from the Tick Tock Profile.",
    "- Span 5 and 10 ms show the drop closer still, and 50 ms the sound and what rings on \
     after it, with the same marks.",
    "- Span One Beat (the drum) runs from just before one beat to just before the next, at \
     the watch's beat rate.",
    "- Span 200 ms shows the whole beat and the quiet after it, the Tick above the Tock. \
     400 ms shows a Tick, the Tock after it and the next Tick in one row.",
    "- Beats: how many of the latest beats are drawn over each other, the older ones \
     fainter.",
    "- One beat shows exactly what was heard; several show how much the beats vary, which \
     the Tick Tock Profile averages away.",
];

pub static POSITIONS_TEST: LazyLock<Vec<&'static str>> = LazyLock::new(|| {
    worded(vec![
        "Leads the watch through the six positions and measures each in turn.".into(),
        "## Steps".into(),
        "1. New Test sets the test up on a clear Bench. Start Test on the Listen card \
     starts it."
            .into(),
        "2. Turn the watch to the position the banner names: dial up, dial down, crown \
     down, crown left, crown up, then crown right."
            .into(),
        format!("3. It settles for the time under Settle ({SETTLE_S:.0} s unless set otherwise)."),
        format!(
            "4. Then it's measured for the time under Each Position ({:.0} s unless set \
     otherwise). Start Measurement goes on once the watch is turned.",
            crate::guided::MEASURE_S
        ),
        "5. After the sixth position the test ends by itself and keeps the take.".into(),
        "## What It Shows".into(),
        "- The table fills in as each position is measured: its rate, its difference from \
     the mean, amplitude and beat error, the same figures the command line gives."
            .into(),
        "- Then the means, the largest difference between the rates, and vertical less \
     horizontal."
            .into(),
        "- The Paper Strip marks Before Test, Settling, each measured stretch and the time \
     Not Used for Positions Test."
            .into(),
        "## Controls".into(),
        "- Pause: holds the test. What the position measured so far isn't used, and Resume \
     settles and measures it again from the start."
            .into(),
        "- Restart Measurement: the same at once, as after a knock.".into(),
        "- Stop Test… (Listen card): asks, then ends the test. The session and its \
     positions stay."
            .into(),
        "## The 24 Hour Test".into(),
        "1. Run the test just after winding, and keep its results with Save Results for 24 \
     Hours…. Saving the session keeps them too, as positions-test.json."
            .into(),
        "2. Leave the watch unwound and off the wrist. 24 hours later, without winding or \
     wearing it in between, set Wind to 24 Hours After and pick that test in Compare With."
            .into(),
        "3. With no watch, load its results with Load Full-Wind Results… instead. Then \
     press New Test and start the test."
            .into(),
        "- The pane shows the earlier results, this test's, and both side by side with the \
     change in each position."
            .into(),
        "- The largest change in rate with crown right left out is Im, and the change in \
     the mean rate Ie."
            .into(),
        "## Saving".into(),
        "- Save as CSV… and Copy write the watch's name, its calibre, beat rate and lift \
     angle, when the test began and its times, then the table."
            .into(),
        "- In the 24 hour half they write both tests side by side. Copy puts tabs between \
     the columns, for a spreadsheet."
            .into(),
    ])
});

pub static DYNAMIC_POISE: LazyLock<Vec<&'static str>> = LazyLock::new(|| {
    use timegrapher_core::poise;
    worded(vec![
        "Leads the watch through the eight upright positions, 45° apart, and measures each in \
     turn."
            .into(),
        "## Steps".into(),
        "1. New Test sets the test up on a clear Bench. Start Test on the Listen card starts \
     it."
        .into(),
        "2. Turn the watch the same way round each time, from 9H at the top: 9H, 7H30, 6H, \
     4H30, 3H, 1H30, 12H and 10H30."
            .into(),
        "3. Each settles and is measured for the times under Settle and Each Position, which \
     the Positions Test shares."
            .into(),
        "- The banner says which hour to turn to the top, and before the first is in it gives \
     the amplitude so far."
            .into(),
        "- The Current Position card turns to 10 for the test.".into(),
        "## What It Shows".into(),
        "- The table gives each position's rate, its difference from the mean, amplitude and \
     beat error, then the means."
            .into(),
        "- The picture has each rate at the hour that was at the top, with its ±.".into(),
        format!(
            "- Once {} positions are in, the wave the rate makes once round is fitted to them \
     and drawn in grey with its fastest hour.",
            poise::MIN_SHOWN
        ),
        "- The line over the picture states its size and fastest hour.".into(),
        "## Reading It".into(),
        "- The line under it is inferred: where the balance's heavy spot hangs when the wave \
     is fastest, from each position's own amplitude."
            .into(),
        format!(
            "- Poise moves the rate least at {}° amplitude and the other way above it. So \
     where any position may be near it, the line says the heavy spot is not stated.",
            timegrapher_core::figure_text::fixed_text(poise::NULL_AMPLITUDE_DEG, 1)
        ),
        "## Controls".into(),
        "- View: draws the same as a plot or round a watch face.".into(),
        "- Info: gives the fit in full, with the wave's twice-round terms, and the amplitudes \
     that show the heavy spot."
            .into(),
        "## Saving".into(),
        "- Copy writes the watch, its calibre, the table and the fit in words, with tabs \
     between the columns for a spreadsheet."
            .into(),
    ])
});

pub static ISOCHRONISM: LazyLock<Vec<&'static str>> = LazyLock::new(|| {
    worded(vec![
        "How the rate changes with the amplitude as the mainspring lets down: a watch whose \
     rate holds as the amplitude falls is isochronous."
            .into(),
        "## What It Shows".into(),
        "- Each dot is the median rate in a band of amplitude, with a bar over its middle \
     half."
            .into(),
        format!(
            "- The Curve view needs the amplitude to fall through at least {ISO_MIN_BINS} bands of \
     {ISO_BIN_DEG:.0}° with {ISO_MIN_READINGS} readings in each, which a session of some \
     hours from full wind gives."
        ),
        "- It comes from the same tests as Steadiness, which the command line runs too.".into(),
        "## The Walk".into(),
        "- View in the toolbar picks the curve or the walk.".into(),
        "- The walk shows the rate against the amplitude stretch by stretch through the \
     session, each dot coloured by its time from the start, from dark to bright as the key \
     says."
            .into(),
        "- A trail shading from each dot's colour to the next's joins them in time order. It \
     breaks wherever a stretch was masked or the session broke."
            .into(),
        "- A ring marks the start, a large dot the end, and a ring each 12 hours. Hover a dot \
     for its time, amplitude and rate."
            .into(),
    ])
});

pub static HISTOGRAMS: LazyLock<Vec<&'static str>> = LazyLock::new(|| {
    worded(vec![
        "How the rate's, amplitude's and beat error's values spread over the whole session \
     (masked stretches left out), one plot above the other."
            .into(),
        "## Reading It".into(),
        "- An average gives one number; these show whether the values come from one steady \
     state or from two."
            .into(),
        "- Two states might be a high and a low amplitude as a rubbing part comes and goes, \
     which a single average hides."
            .into(),
        "- Neither plot keeps the time order, so a slow swing between two states shows only \
     as a bend. The strip and the charts show when it happens."
            .into(),
        "## Probability".into(),
        "- Show: Probability is probability paper, as in flood-frequency analysis. Each value \
     is plotted against its rank, on a scale spaced in standard deviations and labelled in \
     percent."
            .into(),
        "- Values from one steady state, scattered about one mean, fall along the dashed \
     straight line (drawn through the median, with a slope from the middle half of the \
     values)."
            .into(),
        "- Two states draw two straighter stretches joined by a bend, or an S across the \
     line; a gap between two clusters shows as a flat step."
            .into(),
        "- The ends curling away from the line are a few unusual readings.".into(),
        "## Cumulative".into(),
        "- Show: Cumulative draws the share of values at or below each value, on a plain \
     scale."
            .into(),
        "- One state rises in one steep stretch, two states rise twice with a flatter stretch \
     between."
            .into(),
        "## Controls".into(),
        "- The hare, the swing and the seesaw in the toolbar show or hide each one; at least \
     one stays on."
            .into(),
        "- Values: Readings counts each reading, averaged over Average Over as on the charts \
     and the strip, so states lasting longer than that stand out clearly."
            .into(),
        format!(
            "- Values: 2 s counts each {STRETCH_S:.0} seconds of beats, the finest the app \
     measures amplitude and beat error. It catches quicker changes, with more scatter."
        ),
        format!("- For the rate, 2 s is the rate over each {STRETCH_S:.0} seconds of beats."),
        "## The Scale".into(),
        format!(
            "- The scale leaves out the most extreme {}% at each end, so a few stray readings \
     can't squeeze the rest together.",
            percent(distribution::TRIM)
        ),
        "- Off the scale, in each plot's facts box, says how many were left off.".into(),
    ])
});

pub static SIGNAL: LazyLock<&'static str> = LazyLock::new(|| {
    format!(
        "How far the beats stand above the background noise: the \
     typical beat's peak over the typical level between beats. Pure noise reads about \
     2×; below {MIN_SNR:.0}× no beats are logged.\n\
     It measures loudness, not the shape of the tick: hum, knocks or a holder that \
     smears the sound are not caught by it."
    )
    .leak()
});

pub static NO_REGULAR_BEAT: LazyLock<&'static str> = LazyLock::new(|| {
    format!(
        "The beats found don't come at regular intervals: \
     their timing scatter (the median jitter about a straight-line fit) is {:.0}% of a beat \
     or more, as on an empty stand, where it is about 5%. No rate, amplitude or beat \
     error is shown then, since those figures would describe noise.",
        percent(NO_WATCH_JITTER_FRACTION)
    )
    .leak()
});

/// The Power Reserve pane's "?", its limits from core's and the position
/// card named by its title at this width.
pub fn power_reserve(card: &str) -> Vec<String> {
    use timegrapher_core::power_reserve::STOP_AFTER_S;
    vec![
        "Measures how long the watch runs from full wind until it stops.".to_string(),
        "## Steps".to_string(),
        format!(
            "1. Wind the watch fully, set it to a minute marker, and put it on the stand in \
     the position set on the {card} card."
        ),
        "2. New Test sets the run up, and Start Test on the Listen card starts it.".to_string(),
        "3. The app starts a new session and listens until the watch stops.".to_string(),
        "## What It Shows".to_string(),
        format!(
            "- The run ends at the last beat heard before {} min with none.",
            STOP_AFTER_S / 60.0
        ),
        "- Beats that come back sooner join the run, and the pane says how many times they \
     stopped and came back."
            .to_string(),
        "## While It Runs".to_string(),
        "- The computer is kept awake whatever Keep Awake is set to, the sound is kept, and \
     the position can't change."
            .to_string(),
        "- Stop Test… on the Listen card asks, then ends the test, keeping the run so far."
            .to_string(),
        "- A closed lid or Sleep from a menu still stops the computer.".to_string(),
        format!(
            "- A run that hasn't stopped after {} ends there.",
            timegrapher_core::power_reserve::limit_text()
        ),
        "## Saving".to_string(),
        "- When it ends, the take is kept in the picked watch's history (or in Unsorted with \
     no watch picked), named by the date and time it started and \"Power Reserve\"."
            .to_string(),
        "- Its results are in power-reserve.json: the run time from the first beat heard to \
     the last, in true time, and how it ended."
            .to_string(),
        "- With Analysis Only, the take is then kept as its analysis alone and its sound \
     deleted. Every pane reads the take as any other."
            .to_string(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The "?" texts that give a default state the one core and the app
    /// use: the settle, the lift angle and a Positions Test's measure,
    /// under the control that sets it (#684).
    #[test]
    fn the_help_states_the_defaults_in_use() {
        let settle = format!("Settle ({SETTLE_S:.0} s unless");
        assert!(super::POSITION.concat().contains(&settle));
        let lift = format!("Lift is the default {DEFAULT_LIFT_DEG:.0}°");
        assert!(super::CALIBRE.concat().contains(&lift));
        let test = super::POSITIONS_TEST.concat();
        let measure = format!(
            "time under Each Position ({:.0} s unless",
            crate::guided::MEASURE_S
        );
        assert!(test.contains(&settle) && test.contains(&measure), "{test}");
        assert!(!test.contains("under Measure"), "{test}");
    }

    /// The "?" texts that state a limit core reads by give core's own
    /// figure: the rate's ± and its windows, the isochronism curve's
    /// bands, the Distributions scale's trim and its 2 s values, the
    /// no-watch scatter (#690) and the signal the live screen needs (#704).
    #[test]
    fn the_help_states_the_limits_core_reads_by() {
        let rate = RATE.concat();
        let window = format!("{RATE_WINDOW_S:.0}-second rates");
        let unbroken = format!("about {RATE_SE_MIN_S:.0} s of beats in the take");
        assert!(rate.contains(&window) && rate.contains(&unbroken), "{rate}");
        let iso = ISOCHRONISM.concat();
        let bands = format!(
            "at least {ISO_MIN_BINS} bands of {ISO_BIN_DEG:.0}° with {ISO_MIN_READINGS} readings"
        );
        assert!(iso.contains(&bands), "{iso}");
        let trim = format!(
            "the most extreme {}% at each end",
            percent(distribution::TRIM)
        );
        assert!(HISTOGRAMS.concat().contains(&trim));
        let window = format!("2 s counts each {STRETCH_S:.0} seconds of beats");
        assert!(HISTOGRAMS.concat().contains(&window));
        let floor = format!("below {MIN_SNR:.0}× no beats are logged");
        assert!(SIGNAL.contains(&floor), "{}", *SIGNAL);
        let scatter = format!("is {:.0}% of a beat", percent(NO_WATCH_JITTER_FRACTION));
        assert!(NO_REGULAR_BEAT.contains(&scatter), "{}", *NO_REGULAR_BEAT);
    }

    /// The Power Reserve "?" names the position card as it is captioned
    /// and gives core's limits: ten minutes with no beat, and 80 h.
    #[test]
    fn the_power_reserve_help_names_the_card_and_core_limits() {
        for card in ["Current Position", "Position"] {
            let text = super::power_reserve(card).join(" ");
            assert!(text.contains(&format!("set on the {card} card")), "{text}");
            assert!(text.contains("before 10 min with none"), "{text}");
            assert!(text.contains("after 80 h ends there"), "{text}");
        }
    }

    /// The sentences in `text`: each ends at a `.`, `?` or `!` (after an
    /// optional closing quote or bracket) followed by a space and a
    /// capital or a digit, or at the end.
    fn sentences(text: &str) -> usize {
        let b: Vec<char> = text.trim_end().chars().collect();
        let mut n = 1;
        for i in 0..b.len() {
            if !matches!(b[i], '.' | '?' | '!') {
                continue;
            }
            let mut j = i + 1;
            while j < b.len() && matches!(b[j], '"' | ')' | '\u{201d}') {
                j += 1;
            }
            if j + 1 < b.len()
                && b[j] == ' '
                && (b[j + 1].is_uppercase() || b[j + 1].is_ascii_digit())
            {
                n += 1;
            }
        }
        n
    }

    /// Every "?" text, by name, as it is drawn.
    pub(crate) fn every_help_text() -> Vec<(&'static str, Vec<String>)> {
        let own = |t: &[&str]| t.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        vec![
            ("MICROPHONE", own(MICROPHONE)),
            ("RATE", own(&RATE)),
            ("AMPLITUDE", own(AMPLITUDE)),
            ("BEAT_ERROR", own(BEAT_ERROR)),
            ("POSITION", own(&POSITION)),
            ("LISTEN", own(LISTEN)),
            ("CALIBRE", own(&CALIBRE)),
            ("STRIP", own(STRIP)),
            ("PROFILE", own(PROFILE)),
            ("CHARTS", own(CHARTS)),
            ("STEADINESS", own(STEADINESS)),
            ("SCOPE", own(SCOPE)),
            ("POSITIONS_TEST", own(&POSITIONS_TEST)),
            ("DYNAMIC_POISE", own(&DYNAMIC_POISE)),
            ("ISOCHRONISM", own(&ISOCHRONISM)),
            ("HISTOGRAMS", own(&HISTOGRAMS)),
            ("power_reserve", power_reserve("Current Position")),
            ("LIBRARY_HELP", own(crate::settings_window::LIBRARY_HELP)),
            ("MANAGE_HELP", own(crate::calibre_ui::MANAGE_HELP)),
            ("watches::HELP", own(crate::watches::HELP)),
        ]
    }

    /// Every "?" text has the shape of the design guide's section 6
    /// (Hugh, #1520): a one-sentence answer, then headed groups of short
    /// bullets and steps, no group over six bullets; and the toolbar's
    /// group that every pane's "?" ends with keeps the same rules.
    #[test]
    fn every_help_text_has_its_shape() {
        let mut texts = every_help_text();
        let mut toolbar = vec!["The answer.".to_string()];
        toolbar.extend(own_toolbar());
        texts.push(("TOOLBAR", toolbar));
        for (name, text) in &texts {
            assert!(!text.is_empty(), "{name}");
            let Line::Answer(answer) = line(0, &text[0]) else {
                unreachable!()
            };
            let marked = answer.starts_with("## ")
                || answer.starts_with("- ")
                || matches!(line(1, answer), Line::Step(..));
            assert!(!marked, "{name}: the answer is plain: {answer}");
            assert_eq!(sentences(answer), 1, "{name}: one sentence: {answer}");
            let headed = text
                .iter()
                .enumerate()
                .any(|(i, t)| matches!(line(i, t), Line::Heading(_)));
            assert!(text.len() <= 2 || headed, "{name}: no group heading");
            let mut bullets = 0;
            for (i, t) in text.iter().enumerate().skip(1) {
                match line(i, t) {
                    Line::Heading(h) => {
                        assert!(!h.is_empty(), "{name}: empty heading");
                        bullets = 0;
                    }
                    Line::Bullet(w) | Line::Step(_, w) | Line::Paragraph(w) => {
                        if matches!(line(i, t), Line::Bullet(_)) {
                            bullets += 1;
                            assert!(bullets <= 6, "{name}: a group over six bullets at {w}");
                        }
                        assert!(sentences(w) <= 2, "{name}: over two sentences: {w}");
                        assert!(
                            w.chars().count() <= 220,
                            "{name}: over 220 characters ({}): {w}",
                            w.chars().count()
                        );
                    }
                    Line::Answer(_) => unreachable!(),
                }
            }
        }
    }

    fn own_toolbar() -> Vec<String> {
        TOOLBAR.iter().map(|s| s.to_string()).collect()
    }

    /// A line's start sets how it is drawn: the first is the answer,
    /// then headings, bullets, numbered steps and plain paragraphs.
    #[test]
    fn a_lines_start_sets_its_kind() {
        assert_eq!(
            line(0, "## Not a heading"),
            Line::Answer("## Not a heading")
        );
        assert_eq!(line(1, "## Controls"), Line::Heading("Controls"));
        assert_eq!(line(1, "- Span: 20 ms"), Line::Bullet("Span: 20 ms"));
        assert_eq!(line(2, "12. Turn it"), Line::Step("12.", "Turn it"));
        assert_eq!(line(2, "24 hours later"), Line::Paragraph("24 hours later"));
        assert_eq!(line(2, "1.5 s"), Line::Paragraph("1.5 s"));
        assert_eq!(sentences("One. Two 3.5 s. Three…"), 3);
        assert_eq!(
            sentences("Save as CSV… and Copy (Listen card). 24 hours"),
            2
        );
    }

    /// The app's source, outside the tests and the help's own files: each
    /// file's path and text.
    fn app_sources() -> Vec<(String, String)> {
        fn walk(dir: &std::path::Path, out: &mut Vec<(String, String)>) {
            for e in std::fs::read_dir(dir).unwrap() {
                let p = e.unwrap().path();
                let name = p.to_string_lossy().to_string();
                if p.is_dir() {
                    walk(&p, out);
                } else if name.ends_with(".rs") && !name.contains("tests") {
                    out.push((name, std::fs::read_to_string(&p).unwrap()));
                }
            }
        }
        let mut out = Vec::new();
        walk(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
            &mut out,
        );
        out
    }

    /// The third argument of each call to `f(` in `text`, as written.
    fn third_args(text: &str, f: &str) -> Vec<String> {
        let mut found = Vec::new();
        for (at, _) in text.match_indices(f) {
            let mut depth = 0;
            let mut args = vec![String::new()];
            for c in text[at + f.len()..].chars() {
                match c {
                    '(' | '[' | '{' => depth += 1,
                    ')' | ']' | '}' if depth == 0 => break,
                    ')' | ']' | '}' => depth -= 1,
                    ',' if depth == 0 => {
                        args.push(String::new());
                        continue;
                    }
                    _ => {}
                }
                args.last_mut().unwrap().push(c);
            }
            if let Some(a) = args.get(2) {
                found.push(a.trim().trim_start_matches(['&', '*']).to_string());
            }
        }
        found
    }

    /// No "?" text escapes the shape test (Trevor's review of #1552):
    /// every help text the app's source names is in
    /// [`every_help_text`], and every call that draws a "?" passes one
    /// by name, or a local fed only from those.
    #[test]
    fn every_help_call_passes_a_text_the_shape_test_covers() {
        let covered: Vec<&str> = every_help_text().iter().map(|t| t.0).collect();
        let named = |path: &str| {
            let last = path.rsplit("::").next().unwrap_or(path);
            covered.contains(&last) || covered.contains(&path)
        };
        // Hovers of one string, not "?" texts.
        let hovers = ["CLIPPING", "SIGNAL", "NO_REGULAR_BEAT", "TOOLBAR"];
        // Locals that hold one of the texts above: the reading cards'
        // (RATE, AMPLITUDE, BEAT_ERROR, LISTEN), the sound panes'
        // (PROFILE, SCOPE) and Power Reserve's (`power_reserve`).
        let locals = ["help", "text", "paragraphs", "help_text"];
        let mut seen = 0;
        for (file, text) in app_sources() {
            if file.ends_with("help.rs") {
                continue;
            }
            for word in text.split(|c: char| !(c.is_alphanumeric() || c == '_' || c == ':')) {
                let path = word.trim_matches(':');
                let is_help = path.starts_with("help::")
                    && path[6..]
                        .chars()
                        .all(|c| c.is_ascii_uppercase() || c == '_')
                    && path.len() > 6;
                let is_named_help = path.ends_with("HELP") && path.contains("::");
                if (is_help || is_named_help) && !hovers.iter().any(|h| path.ends_with(h)) {
                    assert!(named(path), "{file}: {path} isn't in every_help_text");
                    seen += 1;
                }
            }
            for f in [
                "theme::help(",
                "pane_toolbar(",
                "pane_toolbar_pinned(",
                "pane_toolbar_info(",
                "help_with_key(",
            ] {
                // A definition's parameter, not a call.
                for arg in third_args(&text, f)
                    .into_iter()
                    .filter(|a| !a.contains(": "))
                {
                    let ok = locals.contains(&arg.as_str()) || named(&arg) || arg == "[]";
                    assert!(ok, "{file}: {f}… passes {arg}");
                }
            }
        }
        assert!(seen > 15, "{seen}");
    }
}
