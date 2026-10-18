# User-guide rewrite: technical review notes

The rewritten [user guide](user-guide/README.md) is intended for an
experienced hobby watchmaker who wants both operating instructions and
an explanation of the measurements. Chapters 8a and 8b give Steadiness
and Isochronism enough space to explain every pane, rather than just
identify its controls.

## Implementation Baseline

The measurement audit used documentation-branch commit
`a29b4e6602e4ff47716de31185a8ff2a7cd93c98` (version 0.4.2). Before submission,
the rewrite incorporated the concurrent documentation commits `975fa6e`
and `ef3fb97`. The subsequent main-branch changes through `9da1b46` did
not alter the audited measurement code.

The guide preserves the intended desktop behaviour in
[PR #258](https://github.com/peterhudson/timegrapher/pull/258), checked
against its diff: independent Parallel Guides, strip widths up to ±60 s,
and New Session while microphone capture is active. Those changes were
still pending when this review was written. Chapters 2, 6 and 11 explain
the difference from earlier builds. Recheck these passages when merging
the app and guide branches. No application code is changed by this rewrite.

## Details Worth Preserving During Review

| Topic | Basis and consequence |
| --- | --- |
| Escapement terminology | Chapters 1 and 15 distinguish the impulse face of the pallet stone from the lever, and mechanical drop from the final acoustic locking/banking group. Witschi and Horopedia references are linked. |
| Beat error | `timing.rs` and `measure.rs`: half the long/short interval difference; distinguish unlock-based error, the detected near-drop reference, magnitude and sign. |
| Amplitude and profiles | `amplitude.rs`, `profile.rs`, app `profiles.rs`: short-window card summaries are not measurements of the long displayed profile. The profile is a trimmed envelope mean; its percentile band is not a confidence band. Own Drop aligns the detection reference, not necessarily the measured Drop edge. |
| Precision | `confidence.rs`, `amplitude.rs` and `docs/measurement-precision.md`: successive-window estimates rely on a noise model. They do not measure all watch variation or establish 95% coverage. Clipping is not guaranteed to leave rate unaffected. |
| Analysis windows | `steadiness.rs` and `steady_views.rs`: common 10 s summaries versus 2 s amplitude/beat-error plots and folds. Do not say every pane analyses the displayed chart or follows Average Over. |
| Clock basis | App background analysis uses the sample clock. The CLI can resolve corrections automatically; Positions Test can use a stored correction. Shared engine does not imply identical figures with different settings or intervals. |
| Position joins | App `steady.rs`: selected-position stretches are joined with 5 s separators. This is a constructed timeline, not continuous hours since winding. |
| Periods, Cycle and FFT | A search score is not an effect size; a wheel label is an association. Cycle can borrow a period or fall back to a catalogue wheel. FFT uses peak-to-peak only in converted rate mode; timing, amplitude and beat-error spectra use sine amplitude. Gaps are not amplitude-renormalised. |
| Allan and CUSUM | The reference lines depend on statistical assumptions. The sampled Allan minimum is recording-specific; CUSUM returns to zero because it uses the whole-series mean. |
| Isochronism | `steadiness.rs` and app `isochronism.rs`: 2° bins, at least 10 pairs each and 3 bins; dots are medians and bars IQR. Quadratic curve and separately fitted linear slope are different summaries. |
| Wind comparison | The one-recording table requires 2 h of rate history. Its true 24 h comparison needs the full later half hour, hence slightly more than 24½ h from a full-wind start. Otherwise it uses the last half hour. Desktop assumes wind offset zero. |
| Position conventions | Crown directions map to Swiss hour positions only under the stated case/crown convention. Im retains the sign of the largest absolute eligible change, excludes 12H; Ie is an absolute mean change. The NIHS reference is explicitly Witschi's account. |
| CLI defaults | Only some commands resolve catalogue BPH/lift. `long` and `series` use catalogue train data; `profile` and `shape` do not accept `--calibre`. `synth` has no `--json`. New `record` sessions use 30 s settling; the short-take quick check has a half-take cap. |
| Saving and privacy | Save is a snapshot updated by Save Again. Periodic scratch flushes are not a no-loss guarantee. MCP results enter the chat even though ordinary file analysis does not upload the audio. Tool approval behaviour belongs to the host's settings. |

## Validation

- Built the CLI from the audited branch with
  `cargo build --offline --locked -p timegrapher-cli`.
- Ran `analyze`, `profile` and `doctor --file` on an existing 48 kHz FLAC
  recording. Outputs were valid versioned JSON. These checks used files,
  without opening the live microphone.
- Ran the chapter 12 synthetic example: 600 s at 28,800 bph, 270°,
  +5 s/d, 0.4 ms beat error, with a 60 s repeating disturbance. Then ran
  `series --json --views`; it recovered a component near 60 s in rate
  and amplitude and returned all three sets of analysis views. This is
  an example check, not new empirical validation of the measurement engine.
- Rendered the packaged Linux documentation with the repository's
  `packaging/docs_html.py`, checked local files and heading links, and
  ran the ten existing `test_docs_html.py` tests. Inspected the rendered
  guide and edited figures visually.
- The new order-of-readings illustration is explicitly synthetic. Its
  two series are exact permutations, so their mean, SD and histogram
  match while their autocorrelation differs. Existing app screenshots
  remain illustrative and may need refreshing after the pending UI work.

The general statistical explanations and worked arithmetic are new prose.
No published instrument's tolerances have been turned into acceptance
limits for the user's watch, and no wheel association is presented as a
confirmed component diagnosis.
