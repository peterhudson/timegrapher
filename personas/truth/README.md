# Truth sheets

What is really on each persona's recordings, taken only from the public
corpus (each take's `session.toml` and `README.md` in
peterhudson/timegrapher-corpus). Only the reviewer and the triage step read
these; personas, helpers and the app never see them.

The corpus references are tg (jnarvaezp/tg-mod) on the same files, with
rates on the uncorrected sound-card clock. The sound card on these takes ran
about 20 ppm slow, so a rate corrected for the card clock can differ from the
reference by up to about 2 s/d. Judge agreement within: rate ±3 s/d,
amplitude ±10°, beat error ±0.1 ms, at the same lift angle. A different lift
angle moves amplitude; that is not an error if the helper or app says which
lift angle it used.

Live readings come through the virtual stand (PulseAudio at 48 kHz, mono,
the file looped). They should match a file analysis closely, but a reviewer
who sees a live reading disagree with the reference should say it may be the
stand before calling it the program's fault, and the triage marks such a
note "check against the file".
