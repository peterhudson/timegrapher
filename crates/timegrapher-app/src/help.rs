//! The longer explanations behind the "?" buttons, one per part of the
//! window, so they read alike and live in one place.

pub const WATCH: &[&str] = &[
    "The watch on the microphone. Its name and position are saved with a recording.",
    "Beat rate is how many beats the watch makes an hour (28,800 for most modern \
     automatics, 21,600 and 18,000 for older ones). Auto finds it from the first few \
     seconds of beats; set it by hand if Auto picks the wrong one.",
    "Lift angle is how far the balance turns while it is pushing the fork, a figure \
     fixed by the calibre's design. The app works out the amplitude from the time \
     between the unlock and the drop and this angle, so a wrong lift angle makes every \
     amplitude wrong by the same proportion. Look it up for the calibre; 52° is the \
     usual guess when it isn't known.",
    "Position is how the watch lies: dial up or down, or upright with the crown up, \
     down, left or right. Rate and amplitude change between positions, so each is a \
     separate measurement and changing it starts the readings again.",
];

pub const MICROPHONE: &[&str] = &[
    "Input level is the microphone's gain. Aim for the ticks to peak around -10 dBFS: \
     the light mark on the meter. The red mark is -6 dBFS, the most that leaves room \
     for a louder watch; past it the sound clips, the meter turns red, and amplitude \
     and beat error become unreliable.",
    "For the system default input this is the sound server's input volume, which it \
     puts back on the microphone every time it opens it; for a direct device it is the \
     card's own capture level.",
    "The meter shows the loudest sample in the last half second: green when it is \
     right, amber when very quiet or too hot, red when clipping.",
];

pub const READINGS: &[&str] = &[
    "Rate is how many seconds a day the watch gains (+) or loses (−), from how often it \
     beats compared with how often it should.",
    "Amplitude is how far the balance swings each way from rest, in degrees. The big \
     figure is the average; beside it are the amplitude measured from the ticks and \
     from the tocks.",
    "Beat error is how unevenly the tick and the tock are spaced, in milliseconds: \
     zero when the balance rests exactly midway. The big figure is timed from the \
     unlock, as tg and commercial timegraphers measure it; the smaller one is timed \
     from the beat as a whole, nearer the drop.",
    "Each reading is fitted to the beats of the last \"Average over\" seconds. A longer \
     time gives steadier figures that follow changes more slowly. Early in a session, \
     or after a pause or a gap of more than 2 s with no beats, there are fewer seconds \
     of beats than asked for: the readings use what there is, and say how much under \
     the rate. They never reach back across a pause, since the watch may have been \
     moved or handled.",
    "The tick is simply the first beat heard: the sound can't tell which pallet stone \
     made which beat.",
];

pub const RATE: &[&str] = &[
    "How many seconds a day the watch would gain (+) or lose (−) if it kept running \
     exactly as it did over the beats the reading was fitted to.",
    "It comes from a straight line fitted to the time of every beat: a watch on time \
     beats exactly as often as its beat rate says, a gaining one a little more often. \
     The fit takes the tick and the tock together, so beat error doesn't move it.",
    "The ± figure is roughly how far the rate could be off from the scatter of the \
     beats alone. It shrinks with a longer averaging time.",
    "Under it: how many beats the reading used over how many seconds, and the jitter: \
     how far single beats land from that straight line, as a robust standard deviation \
     in microseconds (millionths of a second). It is the spread of the dots across the \
     paper strip. Lower is steadier. It rises with background noise, a muffled sound \
     or a loose stand, as well as with a watch that runs unevenly (a rubbing part, a \
     worn tooth, low amplitude), so compare it on the same stand and microphone. \
     Commercial timegraphers rarely show it; tg doesn't.",
];

pub const AMPLITUDE: &[&str] = &[
    "How far the balance swings each way from rest, in degrees, averaged over the \
     ticks and tocks of the averaging time.",
    "It is worked out from the time between the unlock and the drop of each beat (the \
     solid green and red lines on the tick tock profile) and the lift angle, so it is \
     only as right as the lift angle under Watch.",
    "Tick and Tock are the amplitude measured from each kind of beat alone. They \
     should agree to a few degrees; a big difference usually means one side's sounds \
     were misread, not a fault in the watch.",
];

pub const BEAT_ERROR: &[&str] = &[
    "How unevenly the tick and the tock are spaced, in milliseconds: zero when the \
     balance's rest point sits exactly midway between the pallet stones' lifting. A \
     watchmaker corrects it by turning the hairspring stud or collet.",
    "The big figure is timed from the unlock, the first sound of each beat, as tg and \
     commercial timegraphers measure it. The smaller one is timed from the beat as a \
     whole, nearer the drop: it is the gap between the two lines of dots on the strip.",
];

pub const STRIP: &[&str] = &[
    "One dot for every beat, tick and tock in their colours, as a printing timegrapher \
     draws it on paper. Along the strip is time; across it is how early or late each \
     beat came.",
    "Early and late are measured against a perfect clock beating exactly at the beat \
     rate (every 125 ms at 28,800 bph). A beat that comes before that clock's beat is \
     early; after it, late. A watch on time keeps the same distance, so its dots run \
     parallel to the edges. A gaining watch beats a little early each time, so its dots \
     drift steadily towards the early side; a losing one towards late. The steeper the \
     drift, the bigger the rate. Witschi and other timegraphers use the same words.",
    "The ticks and the tocks make two lines; the gap between them is the beat error. \
     A line that runs off one edge comes back on the other, so the width is a zoom.",
    "The red rate line is the Rate reading drawn over the beats it was fitted to: if \
     the dots follow it, the reading describes them well. The faint red lines beside it \
     (Parallel Guides) run at the same slope across the whole strip, one per grid step, \
     so you can see at a glance whether the dots everywhere run parallel to the rate \
     or bend away from it.",
    "Amplitude, rate and beat error can be drawn over the strip as lines against the \
     same time, each on its own scale: the scale's ends are written in the line's colour \
     above the strip (or beside it when the strip lies across). Seeing amplitude beside \
     the dots shows at once whether a wobble in the rate comes with a change in \
     amplitude.",
    "Mouse: the wheel changes the length, Ctrl and the wheel (or a pinch) the width; \
     drag along the time to look back, drag across to slide the trace; double-click \
     for the newest beats, centred.",
];

pub const PROFILE: &[&str] = &[
    "The typical sound of a tick (blue) and of a tock (orange) over the averaging \
     time, up to 60 s: the line is the beats' typical loudness at each moment, and the \
     shaded band is where the middle 80% of beats fall. Time runs from before the \
     unlock on the left to after the drop on the right, in milliseconds from the beat's \
     reference point near the drop. Both sides share one scale, so a quieter side \
     shows as smaller.",
    "Each beat has three sounds: 1, the unlock, when the pallet stone lets go of the \
     escape wheel; 2, the impulse, when the escape wheel pushes the fork; 3, the drop, \
     when the next tooth lands on the other stone.",
    "The solid lines are the edges the readings come from: unlock (green) where the \
     beat first rises above the noise, drop (red) where the drop starts, and the drop's \
     peak (purple). Amplitude comes from the time between unlock and drop; the beat \
     error from the unlocks.",
    "The dashed gold lines mark where each of the three sounds rises halfway up its own \
     climb. They come from a separate measurement of the beat's shape, used to \
     recognise escapement faults, so they sit near but not exactly on the edges. Both \
     are measured on the same averaged sound, not on single beats.",
    "A dashed sound line is missing when that sound can't be told apart from its \
     neighbour: a soft unlock that merges into the impulse, say. That is a finding in \
     itself, not a fault in the app.",
    "The flat dashed line is the noise floor: the level before the beat starts.",
    "Linear shows the loudness as measured; dB shows it in decibels below the loudest \
     point, which makes the quiet unlock easier to see.",
];

pub const CHARTS: &[&str] = &[
    "Rate, amplitude and beat error over time, each point a reading over the averaging \
     time. The charts' time axes move together.",
    "Time Span: Session shows everything since the session started, growing as it \
     goes. Strip Length shows the same stretch of time as the paper strip and moves with \
     it, so with the charts under a strip lying across, every time axis lines up.",
    "Mouse: the wheel zooms time around the pointer, Ctrl and the wheel the vertical \
     scale, dragging pans, and a double-click fits the whole session again (and \
     follows new beats while listening). Clicking a moment shows it on the strip and \
     in the readings.",
];

pub const HISTOGRAMS: &[&str] = &[
    "How often each value of a reading came up, over the session or the strip's length \
     (Time Span). An average gives one number; a histogram shows whether the values \
     cluster around it or around two values. Two peaks suggest the watch moves between \
     two states, say a high and a low amplitude as a rubbing part comes and goes, which \
     a single average hides.",
    "Amplitude and beat error count one value for every 2 seconds of beats, the finest \
     the app measures them; rate counts one value per reading, each fitted over the \
     averaging time. The thin line is the median. Above each histogram: how many values, \
     the median, where the middle 80% lie, the bin width, and the peaks when there are \
     more than one.",
    "The bins are as wide as the spread of the values calls for (the Freedman–Diaconis \
     rule), rounded to a round number, so they get finer as values come in.",
];

pub const WINDOW: &[&str] = &[
    "Appearance: Auto follows the system's light or dark setting.",
    "The sidebar button at the left of the toolbar, or Ctrl+B (Cmd+B on a Mac), hides the \
     sidebar to give the panes the whole window, and brings it back. Clicking a card's \
     name folds the card away.",
    "Panes can be dragged by their tabs beside, above or below each other, or onto \
     another's tab to stack them, and resized by dragging the gaps. The × on a tab \
     hides a pane; its switch here brings it back where it was. Reset panes puts \
     everything back as it started.",
];

pub const SIGNAL: &str = "How far the beats stand above the background noise: the \
     typical beat's peak over the typical level between beats.\n\
     10× and above: good, all readings can be trusted.\n\
     5 to 10×: fair. Rate is reliable; amplitude and beat error may be off on some \
     watches. Press the watch more firmly against the pickup, or raise the input level \
     a little, to get above 10×.\n\
     3 to 5×: poor. Only the rate can be trusted; ignore amplitude and beat error. \
     Reposition the watch on the pickup, check the microphone cable, and raise the \
     input level.\n\
     Below 3×: no watch heard, nothing to trust (pure noise reads about 2×).\n\
     It measures loudness, not the shape of the tick: hum, knocks or a holder that \
     smears the sound are not caught by it.";
