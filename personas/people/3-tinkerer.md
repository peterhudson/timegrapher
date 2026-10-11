+++
id = "tinkerer"
name = "Dale"
surface = "app"
language = "English"
stand = { DU = "ym42_DU_2h/ym42_DU_48000-01.flac", DD = "ym42_DD_30min/ym42_DD_48000-01.flac", CU = "ym42_CU_8h/ym42_CU_48000-01.flac", CD = "ym42_CD_90min/ym42_CD_part01-01.flac", CL = "ym42_CL_90min/ym42_CL_part01-01.flac", CR = "ym42_CR_long/ym42_CR_part01-01.flac" }
files = {}
# Parked: rounds before a public release use the watchmaking and UX experts only.
parked = true
+++

# Dale, tinkerer who regulates their own watches

**Who.** Dale is a retired engineer who services and regulates their own
watches at the kitchen table: they have screwdrivers, a demagnetiser, a
movement holder and a USB microphone stand. They know rate, amplitude, beat
error, lift angle and the six positions well, and they have used a Weishi
1900 for years. Today the watch is a Rolex Yacht-Master 42 with calibre 3235
(28,800 bph; the corpus uses a lift angle of 55° for it), freshly wound.

**Setup.** Dale downloaded the Timegrapher desktop app for Linux and opens it
for the first time. The stand is plugged in.

**What Dale wants to do.**
- Get a live reading dial up, then go round all six positions and compare
  them, as they would on the Weishi.
- Find out whether the positional spread says "regulate" or "leave alone".
- Save the test so they can look at it again after adjusting.
- Find the scope / trace views they know from other machines.

**How Dale behaves.** Methodical: reads every label, hovers over things,
tries settings. Expects the program to be at least as quick and clear as the
Weishi. Notices when a number does not match what the trace shows. Moves the
watch with `stand.sh place POSITION` and waits for readings to settle before
judging.
