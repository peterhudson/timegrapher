// Execute the actual embedded FFT renderer; spy only on the chart drawing boundary.
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");
const html = fs.readFileSync(process.env.TIMEGRAPHER_TEST_VIEW_HTML || path.join(__dirname, "../src/view.html"), "utf8");
function definition(name, optional = false) {
  const match = html.match(new RegExp(`^function ${name}\\([^\\n]+\\}\\s*$`, "m")) || html.match(new RegExp(`^function ${name}\\([^\\n]*\\{[\\s\\S]*?^\\}`, "m"));
  if (!match && !optional) throw new Error(`Missing renderer ${name}`);
  return match ? match[0] : "";
}
// The Periods plot's words, as the server puts them in from core steady_views.rs.
const viewsRs = fs.readFileSync(path.join(__dirname, "../../timegrapher-core/src/steady_views.rs"), "utf8");
const coreStr = (name) => viewsRs.match(new RegExp(`pub const ${name}: &str = "([^"]*)";`))[1];
// The threshold's hover is core's threshold_hover, which views.rs's test
// checks the page carries; here a stand-in.
const SCORE_WORDS = { axis_name: coreStr("SCORE_AXIS_NAME"), axis_hover: coreStr("SCORE_AXIS_HOVER"), threshold_name: coreStr("THRESHOLD_NAME"),
  threshold_hover: "Peaks above this line are named." };
const context = vm.createContext({
  SCORE_WORDS, scoreTicks: (lo, hi) => [[1, `1 (${lo}..${hi})`]],
  fftTiming: true, showWheels: true, secs: String, logTicks: () => [], periodTicks: () => [], wheelMarks: () => [], wheelTitle: String,
  xyChart: (chart) => { context.last = JSON.parse(JSON.stringify(chart)); return "<svg data-test='FFT'></svg>"; },
});
vm.runInContext(html.match(/const SERIES = \{[\s\S]*?\n\};/)[0] + "\n" + html.match(/^const CALL_OUT_W = .*$/m)[0] + "\n" + html.match(/^const FAINT_CALL_OUT_W = .*$/m)[0] + "\n" + html.match(/^const BOX_OPACITY = .*$/m)[0] + "\n" + 
  ["esc", "spectrumRuns", "spectrumOf", "steadyNote", "steadyPlot", "mapNumber"].map(n => definition(n)).join("\n") + "\n" + definition("spectrumSupportText", true), context);
function render(support, timing = true, amplitude = [1, 5, 2]) {
  context.fftTiming = timing;
  const sp = { series: "rate", unit: "ms", unit_text: " ms", step_s: 1, period_s: [128, 64, 32], amplitude,
    reading_swing: { reading_s: 10, period_s: [128, 64, 32], swing_s_per_day: [8, 60, 40] } };
  if (support !== undefined) sp.support = support;
  const doc = { spectra: [sp], config: { rate_reading_s: 10 } };
  const before = structuredClone(doc);
  const markup = context.steadyPlot({ series: "rate" }, "spectrum", doc);
  assert.deepEqual(doc, before, "presentation must not rewrite spectrum arrays or metadata");
  return { markup, chart: context.last };
}
for (const timing of [true, false]) {
  test(`FFT availability qualification preserves plotted magnitudes (timing=${timing})`, () => {
    const full = render({ finite_bins: 256, total_bins: 256, description: "FFT input: 256 of 256 grid samples available." }, timing);
    const missing = render({ finite_bins: 128, total_bins: 256, description: "FFT input: 128 of 256 grid samples available. With samples missing, each size is a sine fitted to the samples present (Lomb–Scargle); nothing is filled in." }, timing);
    const legacy = render(undefined, timing);
    assert.ok(full.markup.includes("256 of 256 grid samples available"));
    assert.ok(!full.markup.includes("Lomb"));
    assert.ok(missing.markup.includes("128 of 256 grid samples available"));
    assert.ok(missing.markup.includes("each size is a sine fitted to the samples present"));
    assert.ok(legacy.markup.includes("FFT input support was not recorded."));
    assert.deepEqual(missing.chart, full.chart);
    assert.deepEqual(legacy.chart, full.chart, "legacy numeric plot and peak are preserved");
    assert.equal(full.chart.lines[0].pts.length, 3);
    assert.ok(full.chart.lines[0].pts.every(p => p.every(Number.isFinite)));
  });
}
test("the rate's swing in readings is the document's, as core works it out", () => {
  const { chart } = render({ finite_bins: 256, total_bins: 256 }, false);
  assert.deepEqual(chart.lines[0].pts, [[128, 8], [64, 60], [32, 40]].map(([t, w]) => [Math.log10(t), Math.log10(w)]));
});
test("the note is core's description, escaped, with no words of the view's own", () => {
  const { markup } = render({ finite_bins: 128, total_bins: 256, description: "Core's <b>own</b> words." });
  assert.ok(markup.includes("Core&#39;s &lt;b&gt;own&lt;/b&gt; words.") || markup.includes("Core's &lt;b&gt;own&lt;/b&gt; words."), markup);
  assert.ok(!markup.includes("grid samples available"));
  assert.ok(!markup.includes("not corrected for gaps"));
});
test("malformed support cannot claim complete FFT input", () => {
  for (const support of [null, {}, { finite_bins: true, total_bins: 10 }, { finite_bins: 11, total_bins: 10 }, { finite_bins: 0, total_bins: 0 }]) {
    assert.ok(render(support).markup.includes("FFT input support was not recorded."));
  }
});

test("the Periods score is drawn as it is, in whole scores, with its threshold named in core's words", () => {
  const views = { search: { period_s: [100, 99, 98, 97, 90], score: [1, 9, 0.5, 2, 1], threshold: 3 } };
  context.steadyPlot({ series: "rate", views }, "periods", { period_span: [1.9, 2.1] });
  const c = context.last;
  // No log or dB: the points are the scores.
  assert.deepEqual(c.lines[0].pts.map(([, y]) => y), [1, 9, 0.5, 2, 1]);
  assert.deepEqual(c.y, [0, 9 * 1.15]);
  assert.equal(c.yName, "Score");
  assert.equal(c.yHover, SCORE_WORDS.axis_hover);
  assert.ok(c.yHover.includes("If the noise is as expected"), "a chance only under the model");
  assert.deepEqual(c.hlines.map((h) => [h.y, h.name, h.hover, h.dash]),
    [[3, "Threshold", SCORE_WORDS.threshold_hover, "loose"]]);
});

test("the Periods label sits on its line's top, not a thinned slice's low point", () => {
  const views = { search: { period_s: [100, 99, 98, 97, 90], score: [1, 9, 0.5, 2, 1], threshold: 3 } };
  const mark = (period_s) => {
    context.steadyPlot({ series: "rate", period: { period_s }, views }, "periods", { period_span: [1.9, 2.1] });
    return context.last.marks[0];
  };
  assert.equal(mark(98).y, 9);
  assert.equal(mark(98).x, Math.log10(98));
  // Nothing drawn within 3.5%: the nearest drawn point, 90 s.
  assert.equal(mark(50).y, 1);
});

test("tiny FFT peaks keep their size in timing and rate units", () => {
  for (const timing of [true, false]) {
    context.fftTiming = timing;
    const sp = { series: "rate", unit: "ms", unit_text: " ms", period_s: [128, 64], amplitude: [0.0001, 0.00042],
      reading_swing: { period_s: [128, 64], swing_s_per_day: [0.0001, 0.00042] } };
    context.steadyPlot({ series: "rate" }, "spectrum", { spectra: [sp] });
    assert.ok(context.last.beside[0].text.endsWith(
      timing ? ": 0.00042 ms" : ": 0.00042 s/d peak to peak"), context.last.beside[0].text);
  }
});

test("FFT labels at the existing size boundaries stay unchanged", () => {
  context.fftTiming = true;
  for (const [peak, text] of [[10, "10"], [1, "1.0"], [0.1, "0.10"], [0.001, "0.001"]]) {
    const sp = { series: "rate", unit: "ms", unit_text: " ms", period_s: [128, 64], amplitude: [peak / 2, peak] };
    context.steadyPlot({ series: "rate" }, "spectrum", { spectra: [sp] });
    assert.ok(context.last.beside[0].text.endsWith(": " + text + " ms"), context.last.beside[0].text);
  }
});

test("an amplitude spectrum's tallest line is labelled in degrees with no gap, as core writes it (#1428)", () => {
  context.fftTiming = true;
  const sp = { series: "amplitude", unit: "deg", unit_text: "°", step_s: 1, period_s: [128, 64, 32], amplitude: [1, 5, 2] };
  context.steadyPlot({ series: "amplitude" }, "spectrum", { spectra: [sp], config: { rate_reading_s: 10 } });
  const label = context.last.beside[0].text;
  assert.ok(/: 5(\.0+)?°$/.test(label), label);
});
test("a size core withheld (null, #1638) leaves a gap: two lines, never bridged", () => {
  const { chart } = render({ finite_bins: 128, total_bins: 256 }, true, [1, null, 2]);
  assert.deepEqual(chart.lines.map((l) => l.pts), [[[Math.log10(128), 0]], [[Math.log10(32), Math.log10(2)]]]);
  assert.ok(chart.lines.flatMap((l) => l.pts).every((p) => p[0] !== Math.log10(64)));
});
