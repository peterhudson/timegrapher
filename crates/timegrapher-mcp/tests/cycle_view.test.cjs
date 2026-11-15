// Execute the view's own Cycle renderer: the dots coloured by when their
// cycle came, core's boxes round each phase's middle half, and the note.
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");
const { MINUS } = require("./core_digits.cjs");
const html = fs.readFileSync(process.env.TIMEGRAPHER_TEST_VIEW_HTML || path.join(__dirname, "../src/view.html"), "utf8");
// Core's time colour map as the server puts it in (views.rs holds the
// fixture to core).
const TIME_MAP = JSON.parse(fs.readFileSync(path.join(__dirname, "fixtures/time_map.json"), "utf8"));
function definition(name) {
  const match = html.match(new RegExp(`^function ${name}\\([^\\n]*\\{[\\s\\S]*?^\\}`, "m"));
  if (!match) throw new Error(`Missing renderer ${name}`);
  return match[0];
}
const context = vm.createContext({ MINUS,
  TIME_MAP, document: { documentElement: { dataset: { theme: "dark" } } }, secs: (t) => `${t} s`, linTicks: () => [], lagText: String, TIME_STEPS: [],
  chartWidth: () => 600, CHAR_W: 6.6,
  plotArea: () => ({ id: "c", svg: "" }), gridDown: () => "", gridAcross: () => "",
});
vm.runInContext(html.match(/const SERIES = \{[\s\S]*?\n\};/)[0] + "\n" + html.match(/^const TIME_BANDS = .*$/m)[0] + "\n" +
  html.match(/^const DASH = .*$/m)[0] + "\n" + html.match(/^const CALL_OUT_W = .*$/m)[0] + "\n" + html.match(/^const FAINT_CALL_OUT_W = .*$/m)[0] + "\n" + html.match(/^const BOX_OPACITY = .*$/m)[0] + "\n" + html.match(/^const NAME_CLEAR = .*$/m)[0] + "\n" + 
  ["esc", "fixedDigits", "axisLabel", "stepText", "tableRgb", "timeRgb", "steadyNote", "steadyPlot", "xyChart"].map(definition).join("\n") +
  "\nthis.realChart = xyChart;", context);
const plain = (v) => JSON.parse(JSON.stringify(v));

// 24 cycles of a 30 s fold in three bins; core's columns and spread.
const fold = {
  period_s: 30, from: "own", wheel: null, profile: [1, 3, 2], cycles: 24,
  raster: Array.from({ length: 24 }, (_, r) => [1 + r / 100, 3, r % 2 ? 2 : null]),
  columns: [[1.05, 1.115, 1.17], [3, 3, 3], null], swing: 2, middle_half: 0.06, y_range: [0.7, 3.4],
};

test("the Cycle view colours the dots by when they came and boxes each phase's middle half", () => {
  let chart;
  context.xyChart = (c) => { chart = plain(c); return "<svg/>"; };
  context.steadyPlot({ series: "rate", views: { fold } }, "cycle", {});
  assert.equal(chart.dots.length, 12, "twelve bands of time");
  assert.equal(chart.dots[0].pts.length, 2 * 2 + 1, "the first two cycles' dots, empty cells left out");
  // Core's time map lifted into the theme, first band to last (Hugh's M3).
  assert.equal(chart.dots[0].rgb, `rgb(${TIME_MAP.dark[0].join(",")})`);
  assert.equal(chart.dots[11].rgb, `rgb(${TIME_MAP.dark[TIME_MAP.dark.length - 1].join(",")})`);
  // A box per phase with its quartiles, none where core gave none.
  assert.deepEqual(chart.rects.map((r) => [r.y0, r.y1]), [[1.05, 1.17], [3, 3]]);
  assert.ok(Math.abs(chart.rects[0].x1 - chart.rects[0].x0 - 6) < 1e-9, "0.3 of a bin either side");
  assert.equal(chart.corner.text, "30 s cycle, 24 cycles folded\nThe median line swings 2.000 s/d; the boxes' median height is 0.060 s/d");
  // A fold from before core gave columns draws as before, with no note.
  const old = { ...fold, columns: undefined, swing: undefined, middle_half: undefined };
  context.steadyPlot({ series: "rate", views: { fold: old } }, "cycle", {});
  assert.deepEqual(chart.rects, []);
  assert.equal(chart.corner.text, "30 s cycle, 24 cycles folded");
  // A wheel's cycle named as core title-cases it, the server's wheel_title
  // (#1779), not by the view's own rule.
  context.steadyPlot({ series: "rate", cycle: { wheel: "escape wheel", wheel_title: "Escape Wheel" }, views: { fold: old } }, "cycle", {});
  assert.equal(chart.corner.text, "30 s cycle, Escape Wheel, 24 cycles folded");
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

test("the Cycle view is drawn on core's scale, not one of its own", () => {
  let chart;
  context.xyChart = (c) => { chart = plain(c); return "<svg/>"; };
  context.steadyPlot({ series: "rate", views: { fold } }, "cycle", {});
  assert.deepEqual(chart.y, [0.7, 3.4]);
  assert.ok(!/function spreadRange|function percentileRange/.test(html), "the page works out no scale");
});

test("the Readings view draws level lines only where the words name the levels (#1112)", () => {
  let chart;
  context.xyChart = (c) => { chart = plain(c); return "<svg/>"; };
  const readings = { t_s: [0, 600, 1200, 1800], values: [0.35, 0.39, 0.37, 0.37], y_range: [0.3, 0.4], step_s: 10, stride: 1 };
  const segments = [
    { start_s: 0, end_s: 600, mean: 0.35 },
    { start_s: 600, end_s: 1200, mean: 0.39 },
    { start_s: 1200, end_s: 1800, mean: 0.37 },
  ];
  const plot = (named) =>
    context.steadyPlot({ series: "beat_error", views: { readings }, changes: { segments, named, note: "under the 20% a level change needs" } }, "readings", {});
  plot(false);
  assert.deepEqual(chart.lines, [], "stretches the level test rejects draw no line");
  plot(true);
  assert.deepEqual(chart.lines.map((l) => l.pts), segments.map((g) => [[g.start_s, g.mean], [g.end_s, g.mean]]));
});

test("a long series' readings draw each chunk as a filled band, lowest to highest, under its dot (#1038)", () => {
  let chart;
  context.xyChart = (c) => { chart = plain(c); return "<svg/>"; };
  // Whether it is banded, and its key, are core's (steady_views::banded,
  // readings_key), sent with the readings.
  const readings = { t_s: [30, 90], values: [1, 2], lo: [0.5, 1.5], hi: [1.4, 2.6], counts: [6, 6], chunk_s: 60, y_range: [0, 3], step_s: 10,
    banded: true, key: "10 s readings in 2 bands (12)" };
  context.steadyPlot({ series: "rate", views: { readings } }, "readings", {});
  assert.equal(chart.key.text, readings.key);
  assert.deepEqual(chart.rects, [
    { x0: 0, x1: 60, y0: 0.5, y1: 1.4, color: "--rate", fill: 0.6 },
    { x0: 60, x1: 120, y0: 1.5, y1: 2.6, color: "--rate", fill: 0.6 },
  ]);
  // A short series, one reading a point, has no bands.
  context.steadyPlot({ series: "rate", views: { readings: { ...readings, lo: [1, 2], hi: [1, 2], counts: [1, 1], banded: false } } }, "readings", {});
  assert.deepEqual(chart.rects, []);
  // Filled at the box opacity, with no outline.
  const svg = context.realChart({ x: [0, 1], y: [0, 1], xTicks: () => [], yTicks: () => [], rects: [{ x0: 0, x1: 1, y0: 0, y1: 1, color: "--rate", fill: 0.6 }] });
  assert.match(svg, /<rect [^>]*fill="var\(--rate\)" fill-opacity="0.6" stroke="none"\/>/);
});
