// Execute the view's own Cycle renderer: the dots coloured by when their
// cycle came, core's boxes round each phase's middle half, and the note.
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");
const html = fs.readFileSync(process.env.TIMEGRAPHER_TEST_VIEW_HTML || path.join(__dirname, "../src/view.html"), "utf8");
const palette = fs.readFileSync(path.join(__dirname, "../../timegrapher-core/src/palette.rs"), "utf8");
const VIRIDIS = JSON.parse(palette.match(/pub const VIRIDIS: \[\[u8; 3\]; 17\] = (\[[\s\S]*?\]);/)[1].replace(/,\s*\]$/, "]"));
function definition(name) {
  const match = html.match(new RegExp(`^function ${name}\\([^\\n]*\\{[\\s\\S]*?^\\}`, "m"));
  if (!match) throw new Error(`Missing renderer ${name}`);
  return match[0];
}
const context = vm.createContext({
  VIRIDIS, secs: (t) => `${t} s`, wheelTitle: String, linTicks: () => [], lagText: String, TIME_STEPS: [],
  spreadRange: (v) => [Math.min(...v), Math.max(...v)], chartWidth: () => 600, CHAR_W: 6.6,
  plotArea: () => ({ id: "c", svg: "" }),
});
vm.runInContext(html.match(/const SERIES = \{[\s\S]*?\n\};/)[0] + "\n" + html.match(/^const TIME_BANDS = .*$/m)[0] + "\n" +
  html.match(/^const DASH = .*$/m)[0] + "\n" +
  ["esc", "stepText", "viridisRgb", "steadyNote", "steadyPlot", "xyChart"].map(definition).join("\n") +
  "\nthis.realChart = xyChart;", context);
const plain = (v) => JSON.parse(JSON.stringify(v));

// 24 cycles of a 30 s fold in three bins; core's columns and spread.
const fold = {
  period_s: 30, from: "own", wheel: null, profile: [1, 3, 2], cycles: 24,
  raster: Array.from({ length: 24 }, (_, r) => [1 + r / 100, 3, r % 2 ? 2 : null]),
  columns: [[1.05, 1.115, 1.17], [3, 3, 3], null], swing: 2, middle_half: 0.06,
};

test("the Cycle view colours the dots by when they came and boxes each phase's middle half", () => {
  let chart;
  context.xyChart = (c) => { chart = plain(c); return "<svg/>"; };
  context.steadyPlot({ series: "rate", views: { fold } }, "cycle", {});
  assert.equal(chart.dots.length, 12, "twelve bands of time");
  assert.equal(chart.dots[0].pts.length, 2 * 2 + 1, "the first two cycles' dots, empty cells left out");
  assert.equal(chart.dots[0].rgb, `rgb(${plain(context.viridisRgb(0.15)).join(",")})`);
  assert.equal(chart.dots[11].rgb, `rgb(${plain(context.viridisRgb(0.9)).join(",")})`);
  // A box per phase with its quartiles, none where core gave none.
  assert.deepEqual(chart.rects.map((r) => [r.y0, r.y1]), [[1.05, 1.17], [3, 3]]);
  assert.ok(Math.abs(chart.rects[0].x1 - chart.rects[0].x0 - 6) < 1e-9, "0.3 of a bin either side");
  assert.equal(chart.corner.text, "30 s cycle, 24 cycles folded\nThe median line swings 2.000 s/d; the boxes' median height is 0.060 s/d");
  // A fold from before core gave columns draws as before, with no note.
  const old = { ...fold, columns: undefined, swing: undefined, middle_half: undefined };
  context.steadyPlot({ series: "rate", views: { fold: old } }, "cycle", {});
  assert.deepEqual(chart.rects, []);
  assert.equal(chart.corner.text, "30 s cycle, 24 cycles folded");
});

test("a chart draws outlined boxes and a note of two lines", () => {
  const svg = context.realChart({
    title: "Cycle", x: [0, 10], y: [0, 10], xTicks: () => [], yTicks: () => [],
    rects: [{ x0: 2, x1: 4, y0: 1, y1: 5, color: "--text3" }],
    dots: [{ pts: [[1, 1]], rgb: "rgb(1,2,3)", r: 1.4 }],
    corner: { text: "first\nsecond" },
  });
  assert.match(svg, /<rect [^>]*fill="none" stroke="var\(--text3\)"/);
  assert.ok(svg.includes('<g fill="rgb(1,2,3)">'));
  const ys = [...svg.matchAll(/<text [^>]*y="([\d.]+)"[^>]*>(first|second)</g)].map((m) => [m[2], +m[1]]);
  assert.equal(ys.length, 2);
  assert.ok(ys[0][1] < ys[1][1], "the second line under the first");
});
