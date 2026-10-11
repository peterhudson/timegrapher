# Truth sheets

What is really on each persona's recordings, taken only from the public
corpus (each take's `session.toml` and `README.md` in
peterhudson/timegrapher-corpus). Only the reviewer and the triage step read
these; personas, helpers and the app never see them.

The corpus references are tg (jnarvaezp/tg-mod) on the same files, with
rates on the uncorrected sound-card clock. Compare like with like: the
program's card-clock rate against tg, never its clock-corrected rate. The
card on these takes ran about 19 ppm slow, and a slow card reads a watch
fast, so the corrected rate is about 1.6 s/d lower than the card-clock rate
by design. Judge agreement within: rate ±3 s/d, amplitude ±10°, beat error
±0.1 ms, at the same lift angle.

The references are medians over whole files, and these watches wander within
a file: a short reading can sit well away from the median and still be right.
Where a truth sheet gives a file's own range, a reading inside it is not an
error. A different lift
angle moves amplitude; that is not an error if the helper or app says which
lift angle it used.

Live readings come through the virtual stand (PulseAudio at 48 kHz, mono,
the file looped from its start when the watch is placed; the persona's stand
log has the time of each placing). A live reading covers only the stretch of
the file it heard, so compare it with the file's range, not its median, and
before calling a difference the program's fault, check it against that
stretch of the file. The triage marks such notes "check against the file".

The software presents facts and grades nothing (project decision, 2026-10-11): the
app, the command line and the MCP server show measurements with their ±,
spreads and what was heard. Verdicts, tolerance pass or fail and words like
Good, Poor or Low Confidence in the program's own output are findings
against it. Interpretation belongs to the helper and the watchmaker skill,
and must be sourced.

When interpreting, grade amplitude only against Witschi's tolerances, the table the program and
the watchmaker skill use (`docs/sessions.md`, "Findings"): fully wound, 260–320°
dial up or down (horizontal) and 240–280° crown up, down, left or right
(vertical), with over 330° overbanking. Make no per-calibre amplitude claim
("a 3235 should be above 250°") without a source. The Yacht-Master 42's
readings in the corpus (221–254°) are below those ranges in every position,
so a helper that calls its amplitude low is right, not alarmist; the rate is
still its bigger fault.
