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
const context = vm.createContext({
  fftTiming: true, secs: String, logTicks: () => [], periodTicks: () => [], wheelMarks: () => [], periodsLog: false, wheelTitle: String,
  xyChart: (chart) => { context.last = JSON.parse(JSON.stringify(chart)); return "<svg data-test='FFT'></svg>"; },
});
vm.runInContext(html.match(/const SERIES = \{[\s\S]*?\n\};/)[0] + "\n" +
  ["esc", "spectrumOf", "steadyNote", "steadyPlot"].map(n => definition(n)).join("\n") + "\n" + definition("spectrumSupportText", true), context);
function render(support, timing = true) {
  context.fftTiming = timing;
  const sp = { series: "rate", unit: "ms", step_s: 1, period_s: [128, 64, 32], amplitude: [1, 5, 2],
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
    const full = render({ finite_bins: 256, total_bins: 256 }, timing);
    const missing = render({ finite_bins: 128, total_bins: 256 }, timing);
    const legacy = render(undefined, timing);
    assert.ok(full.markup.includes("256 of 256 grid samples available"));
    assert.ok(!full.markup.includes("not corrected for gaps"));
    assert.ok(missing.markup.includes("128 of 256 grid samples available"));
    assert.ok(missing.markup.includes("Missing samples affect peak heights and shapes; values are not corrected for gaps."));
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
test("malformed support cannot claim complete FFT input", () => {
  for (const support of [null, {}, { finite_bins: true, total_bins: 10 }, { finite_bins: 11, total_bins: 10 }, { finite_bins: 0, total_bins: 0 }]) {
    assert.ok(render(support).markup.includes("FFT input support was not recorded."));
  }
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
