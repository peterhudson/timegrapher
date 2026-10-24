//! The longer explanations behind the "?" buttons, one per part of the
//! window, so they read alike and live in one place.

pub const MICROPHONE: &[&str] = &[
    "The app looks for microphones only when the microphone menu opens, Rescan is \
     pressed or listening starts, and at launch only when Listen When the App Opens is \
     on (Settings > General): asking an input what it supports briefly opens it, which \
     could disturb another program recording from it.",
    "Input Level is the microphone's gain. The meter has plain marks at -10 and -6 dBFS \
     for reference. At 0 dBFS the sound clips and the meter turns red.",
    "For the system default input this is the sound server's input volume, which it \
     puts back on the microphone every time it opens it; for a direct device it is the \
     card's own capture level.",
    "The Peak meter shows the loudest sample in the last half second, in dBFS \
     (decibels below the most the input can carry). It turns red when the sound clips.",
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
     paper strip; smaller means less scatter. Background noise, a muffled sound and \
     the stand add to it as well as the watch, so figures from different stands and \
     microphones differ. tg doesn't show it.",
    "True Clock, when shown, is the rate corrected for the sound card's own clock. A \
     card's crystal runs a steady 10 to 50 parts per million off, which moves every \
     rate read on it by 1 to 4 seconds a day. The app times the card's clock against \
     the computer's while it listens. After 20 minutes, once the timing is known to \
     half a part per million, it offers the result under the rate with a Keep button; a \
     busy computer delivers sound less evenly, so the offer can come later. The result \
     is only right if the computer's clock is kept by internet time. Once the error is \
     kept (by Keep, `timegrapher clock measure --save`, or `clock from-log --save` \
     after a recording with a clock log), the app shows the corrected rate under the \
     big figure. Until then the rate is read on the card's clock alone, as tg reads \
     it. The big figure stays on the card's clock, the same clock tg and an analysis of the \
     recording read, so the two compare directly.",
    "The ± is how far the figure could be off from the scatter of 10-second rates over \
     the run so far: a property of the measurement, not of the watch. \
     It needs about 30 s of unbroken beats. Anything the ± can't show (a short stretch, \
     missing beats) is named beside the title. Stopped after a long run, a line \
     under the figure gives the whole take as timegrapher session reads it, with \
     the 5th to 95th percentile of its 10-second rates.",
];

pub const CLIPPING_BANNER: &[&str] = &[
    "The loudest part of each tick reaches the top of the input's range and is cut \
     flat. That moves where the drop seems to be, so the amplitude and the beat error \
     read wrong. The rate comes from when the beats arrive, which clipping doesn't \
     change.",
    "It shows whenever a beat in the readings has a clipped sample in its sound, \
     counted the same way `timegrapher analyze` counts them. Input Level is in \
     Settings > Microphone.",
];

pub const AMPLITUDE: &[&str] = &[
    "How far the balance swings each way from rest, in degrees, averaged over the \
     ticks and tocks of the averaging time.",
    "It is worked out from the time between the unlock and the drop of each beat (the \
     solid green and indigo lines on the Tick Tock Profile) and the lift angle, so it is \
     only as right as the lift angle, the Lift on this card.",
    "The ± figure is roughly how far the amplitude could be off from the scatter of \
     its 2-second pieces alone. It is precision, not accuracy: the amplitude moves \
     with the lift angle in proportion (1° of lift moves it about 5° at 260° with a \
     52° lift, less at lower amplitudes), so a lift angle that is a little out matters \
     more.",
    "Tick and Tock are the amplitude measured from each kind of beat alone.",
    "Anything the ± can't show (a lift angle assumed, clipped beats, a short stretch) \
     is named beside the title.",
];

pub const BEAT_ERROR: &[&str] = &[
    "How unevenly the tick and the tock are spaced, in milliseconds: zero when the \
     balance's rest point sits exactly midway between the pallet stones' lifting.",
    "The big figure is timed from the unlock, the first sound of each beat, as tg \
     measures it (checked against tg in the regression tests). The smaller one is timed from the beat as a \
     whole, nearer the drop: it is the gap between the two lines of dots on the strip.",
    "It is shown as a size. Which beat is the tick depends on where the analysis first \
     locks on, so which side is longer can flip from one run to the next. \
     The ± is the scatter of its 2-second pieces; anything it can't show (clipped \
     beats, a short stretch) is named beside the title.",
];

pub const POSITION: &[&str] = &[
    "The position the watch is in on the stand: Dial Up, Dial Down, or standing on its \
     edge with the crown up, down, left or right. While you listen, the arrows around it \
     tip the watch as seen from above: up tips it back, so the side away from you goes \
     down; down tips it forward; left and right roll it to that side; the corner arrow \
     turns it over. From Dial Up, up gives Crown Left (12 down), left Crown Up. Keys 1 \
     to 6 pick a position too: DU, DD, CU, CD, CL and CR, in that order.",
    "Looking at a take, or during a Positions Test or a Power Reserve Test, the card only \
     shows the position: the one at the strip's latest beat on show, or the one the test \
     set. A take's positions can't be changed afterwards.",
    "Changing it while listening marks a new stretch on the strip and the charts, and the \
     readings, profile, distributions and Steadiness then use only this position's beats. \
     The settling after a move is left out of the readings. A saved take notes each \
     position.",
];

pub const LISTEN: &[&str] = &[
    "Listen starts the microphone: with a watch picked in Watches, a new take of it, saved \
     into it at Stop; with none, or Unsorted Takes picked, Quick Listen, kept only if you \
     Save it. Stop lets go of the microphone and keeps everything on show.",
    "Save keeps what is on show in one of your watches or in a folder. Average sets how \
     many seconds of beats each reading is worked out over: longer is steadier, shorter \
     follows changes sooner.",
];

pub const BEAT_RATE: &[&str] = &[
    "How many beats the watch makes an hour, ticks and tocks counted alike: 28,800 \
     for a balance swinging 4 times a second each way, 21,600 for 3, 18,000 for 2.5.",
    "With the Beat Rate menu on this card on Auto, it is the standard rate nearest to the beats \
     of the first few seconds. Set it there when the watch beats at an unusual \
     rate, or to read a watch that runs far off.",
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
     or bend away from it. Each can be shown without the other.",
    "Amplitude, rate and beat error can be drawn over the strip as lines against the \
     same time, each on its own scale: the scale's ends are written in the line's colour \
     above the strip (or beside it when the strip lies across). Seeing amplitude beside \
     the dots shows at once whether a wobble in the rate comes with a change in \
     amplitude.",
    "With Direction: Across and the charts set to the strip's length (Time Span: Strip), the \
     strip and the charts below it share one time axis, and the pointer's time is \
     marked on all of them at once.",
    "Mouse: the wheel changes the length about the time under the pointer, Ctrl and the \
     wheel (or a pinch) the width; \
     drag along the time to look back, drag across to slide the trace; double-click \
     for the newest beats, centred.",
    "Mask leaves stretches out of the readings, such as a bump of the stand: with it \
     on, drag along the strip over the beats to mask them, drag a masked stretch's edge \
     to move it, drag with the right button to look back, and click a masked stretch to \
     bring it back; a right click brings one \
     back with Mask on or off. Nothing is read inside a masked stretch and no reading \
     reaches across one; the charts, the steadiness tests and a Positions Test leave it \
     out too. A take saved from the app keeps them; a recording opened from disk keeps \
     them when it is saved. Each point on the strip's line is the reading over the \
     Average Over that ends there, so just after a masked stretch the line is blank \
     for about one Average Over, while the sound after it builds up. While Mask is on, dragging masks instead of sliding \
     the trace.",
];

pub const PROFILE: &[&str] = &[
    "The typical sound of a tick (blue) and of a tock (orange) over the averaging \
     time, up to 60 s: the line is the beats' typical loudness at each moment, and the \
     shaded band is where the middle 80% of beats fall. Time runs from before the \
     unlock on the left to after the drop on the right, in milliseconds from the drop (see \
     Time From). Both sides share one scale, so a quieter side \
     shows as smaller.",
    "Each beat has three sounds: 1, the unlock, when the pallet stone lets go of the \
     escape wheel; 2, the impulse, when the escape wheel pushes the fork; 3, the drop, \
     when the next tooth lands on the other stone.",
    "The solid lines are the edges the readings come from: unlock (green) where the \
     beat first rises above the noise, drop (indigo) where the drop starts, and the \
     drop's peak (a thinner indigo line). Amplitude comes from the time between unlock and drop; the beat \
     error from the unlocks.",
    "The dashed brown lines mark where each of the three sounds rises halfway up its own \
     climb. They come from a separate measurement of the beat's shape, so they sit \
     near but not exactly on the edges. Both \
     are measured on the same averaged sound, not on single beats.",
    "A dashed sound line is missing when that sound can't be told apart from its \
     neighbour: a soft unlock that merges into the impulse, say.",
    "The flat dashed line is the noise floor: the level before the beat starts.",
    "Time From: Shared puts the tick and the tock on one clock. Zero is where a watch \
     in beat would drop, and each side sits half the beat error from it, so the two \
     drop lines stand the beat error from the drop apart and the two unlock lines the \
     beat error from the unlock. Own Drop measures each side from its own drop, so both \
     drops sit at 0 ms and only the unlock lines show a difference.",
    "The two beat errors differ when the tick and the tock take different times from \
     unlock to drop, the same imbalance that gives them different amplitudes. The \
     unlock figure is the beat error tg reports; the drop \
     figure adds that imbalance on top.",
    "Linear shows the loudness as measured; dB shows it in decibels below the loudest \
     point, which makes the quiet unlock easier to see.",
];

pub const CHARTS: &[&str] = &[
    "Rate, amplitude and beat error over time, each point a reading over the averaging \
     time. The charts' time axes move together.",
    "Time Span: Session shows everything since the session started, growing as it \
     goes. Strip shows the same stretch of time as the paper strip and moves with \
     it, so with the charts under a strip lying across, every time axis lines up.",
    "Mouse: the wheel zooms time around the pointer, Ctrl and the wheel the vertical \
     scale, dragging pans, and a double-click fits the whole session again (and \
     follows new beats while listening). Clicking a moment shows it on the strip and \
     in the readings.",
];

pub const STEADINESS: &[&str] = &[
    "How the rate, the amplitude and the beat error (from the unlock) each change over \
     the session: cycles, steps, trends and how each reading relates to the last. A \
     distribution can't show \
     a cycle, a step or a slow wander because it throws the time order away; these \
     tests keep it. They are the same as `timegrapher series`.",
    "Each series gets a sentence saying what the tests found: a cycle (named after \
     the wheel whose turn it matches; the fourth wheel turns once a minute), two \
     levels the readings switch between, a level that steps and stays, a slow trend, \
     correlation between readings, or none of these. When two levels come from the \
     unlock mark hopping between two edges of the sound, the sentence says so. Under \
     it are the statistics: readings, median, standard deviation, the scatter from one \
     reading to the next, the lag-1 correlation and the Ljung–Box p-value, and the \
     trend and period when found.",
    "Each view is a pane of its own, with its icon under Steadiness at the top, so two \
     can sit side by side. Rate, Amplitude and Beat Error, in each pane's toolbar, \
     choose which series that pane shows; at least one stays on.",
    "Readings Over Time: every reading in time order (10 s rate readings, 2 s \
     amplitude and beat error readings), with the stretches of constant level and the \
     two state levels. Periods: how strongly the readings repeat at each period, with \
     the 1% false-alarm line dashed and the wheels' periods marked; Log Scale in its \
     toolbar draws the scores on a log scale, so a small peak beside a tall one shows. \
     Cycle: the readings \
     folded at the cycle found. Autocorrelation: how much a reading resembles the one \
     a lag later, with the band independent readings stay inside. Allan Deviation: the \
     scatter of averages against the dashed line independent readings follow, on \
     logarithmic axes; the lowest point is the averaging time that gives the most \
     repeatable reading. CUSUM: the running sum of distances from the mean, which \
     stays inside the dashed lines unless the level changes. FFT: the size of the sine \
     wave at each period, like Witschi's FFT, in the series' own units, the rate's as \
     the swing peak to peak it makes in 10 s rate readings, in s/d, from 20 s up (from \
     the beat timing: a timing sine of a ms at period P swings the rate by \
     2 × a × 2π / P × 86.4 s/d, of which a 10 s reading keeps sin(x)/x, x = π × 10 / P). \
     Timing in its toolbar draws rate as the beat timing's own spectrum, in ms, at every \
     period instead. In both, each wheel's period is a bold line, and half of \
     it, twice a turn as an off-centre wheel shows, is a fainter dashed line marked \
     ×2, in the wheels' own green; the STFT and Wavelet Maps dash the wheels' turns \
     across the map. With no calibre picked, or none of its wheels known, one minute \
     and one hour are marked in grey instead. Wheel Periods in the toolbar shows or \
     hides these marks.",
    "The tests need about 5 minutes of beats, and say more the longer the session: a \
     cycle has to come round several times to count. They run over the whole session \
     in the background, again every minute while it grows; for a recording opened, \
     they cover all of it. Rates are on the \
     sound card's clock.",
];

pub const SCOPE: &[&str] = &[
    "The sound of the latest beats as it came in, high-passed so hum and rumble don't \
     hide it: what an oscilloscope on the microphone would show. While listening it \
     shows the last few seconds; for a recording, the moment on the time slider or the \
     strip.",
    "Span: 20 ms shows one sound in detail, from before the unlock to after the drop, \
     with the Tick above the Tock and the unlock and drop marks from the Tick Tock \
     Profile. 5 and 10 ms show the drop closer still, and 50 ms the sound and what rings \
     on after it, with the same marks. One Beat runs from just before one beat to just \
     before the next, at the watch's beat rate. 200 ms shows the whole beat and the \
     quiet after it, the Tick above the Tock. 400 ms shows a Tick, the Tock after it and \
     the next Tick in one row.",
    "Beats: how many of the latest beats are drawn over each other, the older ones \
     fainter. One beat shows exactly what was heard; several show how much the beats \
     vary, which the Tick Tock Profile averages away.",
];

pub const POSITIONS_TEST: &[&str] = &[
    "Start Test leads the watch through the six positions in turn: dial up, dial down, \
     crown down, crown left, crown up and crown right. In each, the watch settles for \
     the time under Settle (20 s unless set otherwise), then is measured for the time \
     under Measure (60 s unless set otherwise); the banner says which position comes \
     next, and Start Measurement goes on once the watch is turned. The table fills in \
     as each position is measured, with the figures `timegrapher session` gives for the \
     same stretches: each rate, its difference from the mean, amplitude and beat error, \
     then the means, the largest difference between the rates and vertical less \
     horizontal.",
    "Pause holds the test: what the position measured so far isn't used, and Resume \
     settles and measures it again from the start. Restart Measurement does the same at \
     once, as after a knock. Stop ends the test; the session and its positions stay. \
     The Paper Strip marks Before Test, Settling, each measured stretch and the time Not \
     Used for Positions Test.",
    "For the test 24 hours after winding: run it just after winding and keep its results \
     with Save Results for 24 Hours… (saving the session keeps them too, as \
     positions-test.json). 24 hours later, without winding or wearing the watch in \
     between, Load Full-Wind Results… and start the test: the pane shows the earlier \
     results, this test's, and both side by side with the change in each position, the \
     largest change in rate with crown right left out (Im) and the change in the mean \
     rate (Ie).",
    "Save as CSV… and Copy write the watch's name, its calibre, beat rate and lift angle, \
     when the test began and its times, then the table, and in the 24 hour half both \
     tests side by side. Copy puts tabs between the columns, for a spreadsheet.",
];

pub const ISOCHRONISM: &[&str] = &[
    "How the rate changes with the amplitude as the mainspring lets down: a watch whose \
     rate holds as the amplitude falls is isochronous. Each dot is the median rate in a \
     band of amplitude, with a bar over its middle half; the line is the curve through \
     them, and the upright line marks where it turns, if it does.",
    "The curve needs the amplitude to fall through at least three 2° bands with 10 \
     readings in each, which a session of some hours from full wind gives. It comes \
     from the same tests as Steadiness, the same as `timegrapher series`.",
    "0 h / 24 h compares the first half hour of the session with the half hour 24 hours \
     later in one position: rate, amplitude and beat error, and how much each \
     changed. It needs at least 2 hours; under 24 hours, the \
     last half hour stands in for the 24 h one.",
];

pub const HISTOGRAMS: &[&str] = &[
    "How a reading's values spread over the session or the strip's length (Time Span). \
     An average gives one number; these show whether the values come from one steady \
     state or from two, say a high and a low amplitude as a rubbing part comes and \
     goes, which a single average hides.",
    "Show: Probability is probability paper, as in flood-frequency analysis. Each value \
     is plotted against its rank, on a scale spaced in standard deviations and labelled \
     in percent. Values from one steady state, scattered about one mean, fall along the \
     dashed straight line (drawn through the median, with a slope from the middle half \
     of the values). Two states draw two straighter stretches joined by a bend, or an S \
     across the line; a gap between two clusters shows as a flat step. The ends curling \
     away from the line are a few unusual readings.",
    "Cumulative draws the share of values at or below each value, on a plain scale: \
     one state rises in one steep stretch, two states rise twice with a flatter stretch \
     between.",
    "Neither plot keeps the time order, so a slow swing between two states shows only \
     as a bend. The strip and the charts show when it happens.",
    "Values: Readings counts each reading, averaged over Average Over as on the charts \
     and the strip, so states lasting longer than that stand out clearly. 2 s counts \
     each 2 seconds of beats, the finest the app measures amplitude and beat error: it \
     catches quicker changes, with more scatter. Rate always counts readings.",
    "The scale leaves out the most extreme 0.5% at each end, so a few stray readings \
     can't squeeze the rest together; the line above says how many were left off.",
];

pub const SIGNAL: &str = "How far the beats stand above the background noise: the \
     typical beat's peak over the typical level between beats. Pure noise reads about \
     2×; below 4× no beats are logged.\n\
     It measures loudness, not the shape of the tick: hum, knocks or a holder that \
     smears the sound are not caught by it.";

pub const NO_REGULAR_BEAT: &str = "The beats found don't come at regular intervals: \
     their timing scatter (the median jitter about a straight-line fit) is 2% of a beat \
     or more, as on an empty stand, where it is about 5%. No rate, amplitude or beat \
     error is shown then, since those figures would describe noise.";

pub const POWER_RESERVE: &[&str] = &[
    "Wind the watch fully, set it to a minute marker, put it on the stand in the \
     position picked under Position, and press Start Test. The app starts a new \
     session and listens until the watch stops. The run ends at the last beat heard \
     before ten minutes with none. Beats that come back sooner join the run, and the \
     pane says how many times they stopped and came back.",
    "While the test runs, the computer is kept awake whatever Keep Awake is set to, \
     the sound is kept, the position can't change, and Stop ends the test. A closed \
     lid or Sleep from a menu still stops the computer. A run that hasn't stopped \
     after 80 hours ends there.",
    "When it ends, the take is kept in the picked watch's history (or in Unsorted with \
     no watch picked), named by the date and time it started and \"Power Reserve\", \
     with its results in power-reserve.json: the run time from the first beat heard \
     to the last, in true time, and how it ended. With Analysis Only, the take is then \
     kept as its analysis alone and its sound deleted. Every pane reads the take as \
     any other.",
];
