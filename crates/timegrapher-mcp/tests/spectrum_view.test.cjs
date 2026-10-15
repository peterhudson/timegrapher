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
  fftTiming: true, secs: String, logTicks: () => [], wheelMarks: () => [],
  xyChart: (chart) => { context.last = JSON.parse(JSON.stringify(chart)); return "<svg data-test='FFT'></svg>"; },
});
vm.runInContext(html.match(/const SERIES = \{[\s\S]*?\n\};/)[0] + "\n" +
  ["esc", "spectrumOf", "readingSwing", "steadyNote", "steadyPlot"].map(n => definition(n)).join("\n") + "\n" + definition("spectrumSupportText", true), context);
function render(support, timing = true) {
  context.fftTiming = timing;
  const sp = { series: "rate", unit: "ms", step_s: 1, period_s: [128, 64, 32], amplitude: [1, 5, 2] };
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
test("malformed support cannot claim complete FFT input", () => {
  for (const support of [null, {}, { finite_bins: true, total_bins: 10 }, { finite_bins: 11, total_bins: 10 }, { finite_bins: 0, total_bins: 0 }]) {
    assert.ok(render(support).markup.includes("FFT input support was not recorded."));
  }
});
