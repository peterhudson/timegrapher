// Execute the view's own time-frequency map renderer against the core's
// colour table and the colour scale the document gives each map.
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");
const html = fs.readFileSync(process.env.TIMEGRAPHER_TEST_VIEW_HTML || path.join(__dirname, "../src/view.html"), "utf8");
// The tables the server puts in the page, read from core palette.rs and
// period_ticks.rs.
const palette = fs.readFileSync(path.join(__dirname, "../../timegrapher-core/src/palette.rs"), "utf8");
const VIRIDIS = JSON.parse(palette.match(/pub const VIRIDIS: \[\[u8; 3\]; 17\] = (\[[\s\S]*?\]);/)[1].replace(/,\s*\]$/, "]"));
const ticksRs = fs.readFileSync(path.join(__dirname, "../../timegrapher-core/src/period_ticks.rs"), "utf8");
const PERIOD_TICKS = [...ticksRs.matchAll(/tick\(([\d.]+), "([^"]+)", (\d)\)/g)]
  .map(([, p, label, level]) => ({ period_s: +p, label, level: +level }));
function definition(name) {
  const match = html.match(new RegExp(`^function ${name}\\([^\\n]*\\{[\\s\\S]*?^\\}`, "m"));
  if (!match) throw new Error(`Missing renderer ${name}`);
  return match[0];
}
const context = vm.createContext({
  VIRIDIS, PERIOD_TICKS, mapPaints: [], mapsLog: false, showWheels: true, Uint8ClampedArray,
  steadyView: "periods", periodsLog: false, fftTiming: false,
  chartWidth: () => 600, logTicks: () => [], linTicks: () => [], fmtTime: (t) => `t${t}`, TIME_STEPS: [],
  plotArea: () => ({ id: "c", svg: "" }), CHAR_W: 6.6, secs: (t) => `${t} s`, wheelTitle: String,
});
assert.ok(html.includes("const VIRIDIS = /*VIRIDIS*/null;"), "the server fills the table in");
vm.runInContext(html.match(/const SERIES = \{[\s\S]*?\n\};/)[0] + "\n" + html.match(/^const DASH = .*$/m)[0] + "\n" + html.match(/^const WHEELS_HINT = .*$/m)[0] + "\n" +
  ["esc", "steadyNote", "steadyPlot", "steadySwitch", "periodTicks", "wheelMarks", "referenceLines", "viridisRgb", "scaleAt", "mapScale", "mapOf", "mapSlots", "mapSize", "mapChart", "mapPixels", "mapCell"]
    .map(definition).join("\n") + "\nthis.mapPaints = mapPaints;", context);

// Values made in the renderer's context, as plain ones.
const plain = (v) => JSON.parse(JSON.stringify(v));
const nan = null;
// Windows at 600, 900 and 1500 s: the one at 1200 s was left out (tfmap.rs's test).
const map = {
  series: "amplitude", method: "stft", unit: "deg", step_s: 2, window_s: 1200, hop_s: 300,
  period_s: [100, 10, 1], t_s: [600, 900, 1500],
  values: [[1, 2, nan], [0, 4, 4], [nan, nan, 2]],
  scale: { linear: { lo: 0, hi: 4 }, log: { lo: 4 / 300, hi: 4 } },
};

test("the colours are the core's viridis, between its points", () => {
  assert.deepEqual(plain(context.viridisRgb(0)), [68, 1, 84]);
  assert.deepEqual(plain(context.viridisRgb(1)), [253, 231, 37]);
  assert.deepEqual(plain(context.viridisRgb(-1)), plain(context.viridisRgb(0)));
  // palette.rs's own test: halfway between the first two points.
  assert.deepEqual(plain(context.viridisRgb(1 / 32)), [70, 13, 95]);
});

test("a map is coloured on the document's scale, its empty cells clear", () => {
  const sc = context.mapScale(map, false);
  assert.deepEqual(plain(sc), { lo: 0, hi: 4, log: false });
  const [t0, dt, w] = context.mapSlots(map);
  assert.deepEqual([t0, dt, w], [600, 300, 4]);
  const px = context.mapPixels({ m: map, sc, t0, dt, w });
  const at = (x, y) => Array.from(px.slice((y * w + x) * 4, (y * w + x) * 4 + 4));
  assert.deepEqual(at(0, 2), [0, 0, 0, 0], "not read");
  assert.deepEqual(at(2, 0), [0, 0, 0, 0], "the window left out");
  assert.deepEqual(at(1, 0), [68, 1, 84, 255]);
  assert.deepEqual(at(1, 1), [253, 231, 37, 255]);
  assert.deepEqual(at(3, 2), [...plain(context.viridisRgb(0.5)), 255]);
  // Under the pointer: the column's time, the row's period and its size.
  const p = { m: map, sc, t0, dt, w };
  assert.deepEqual(plain(context.mapCell(p, 910, 1)), { t: 900, period: 10, v: 4 });
  assert.deepEqual(plain(context.mapCell(p, 600, 2)), { t: 600, period: 1, v: null });
  assert.equal(context.mapCell(p, 1200, 1), null, "between windows");
  // On a log scale the bottom is the document's, not worked out here.
  const log = context.mapScale(map, true);
  assert.equal(log.lo, 4 / 300);
  assert.equal(context.scaleAt(log, 4), 1);
});

test("the STFT and Wavelet Maps draw each series' map or say why not", () => {
  const doc = { duration_s: 1800, time_frequency: [map], input: { settings: { named_wheels: [{ name: "fourth wheel", period_s: 60 }] } } };
  context.mapPaints.length = 0;
  const drawn = context.steadyPlot({ series: "amplitude" }, "stft", doc);
  assert.ok(drawn.includes('canvas class="map" data-map="0" width="4" height="3"'), drawn);
  assert.ok(drawn.includes("0.000°") && drawn.includes("4.0° and above"), drawn);
  assert.ok(drawn.includes("fourth wheel"));
  assert.equal(context.mapPaints.length, 1);
  assert.ok(context.steadyPlot({ series: "amplitude" }, "wavelet", doc).includes("needs a longer stretch of unbroken readings"));
  assert.ok(context.steadyPlot({ series: "rate" }, "stft", doc).includes("needs 20 minutes of readings"));
  // A map with nothing read has no scale.
  const blank = { ...map, scale: { linear: null, log: null } };
  assert.ok(context.steadyPlot({ series: "amplitude" }, "stft", { time_frequency: [blank] }).includes("Nothing in this stretch could be read"));
});

test("period axes are marked as core's period_ticks marks them", () => {
  assert.equal(PERIOD_TICKS.length, 19, "every mark read from period_ticks.rs");
  assert.ok(html.includes("const PERIOD_TICKS = /*PERIOD_TICKS*/null;"), "the server fills the list in");
  const labels = (px) => context.periodTicks(Math.log10(4), Math.log10(60000), px).map(([, l]) => l).join(", ");
  // period_ticks.rs's own test, at 64 px a mark: room for 20, 10 and 6.
  assert.equal(labels(20 * 64), "5 s, 10 s, 30 s, 1 min, 2 min, 5 min, 10 min, 30 min, 1 h, 2 h, 6 h, 12 h");
  assert.equal(labels(10 * 64), "5 s, 10 s, 30 s, 1 min, 5 min, 10 min, 30 min, 1 h, 6 h, 12 h");
  assert.equal(labels(6 * 64), "10 s, 1 min, 10 min, 1 h, 6 h");
  // Each mark sits at its period, in log10 seconds.
  assert.deepEqual(plain(context.periodTicks(Math.log10(50), Math.log10(70), 640)), [[Math.log10(60), "1 min"]]);
});

test("wheels are marked in their colour, else 1 min and 1 h in grey, and Wheel Periods turns them off", () => {
  const plainDoc = { input: { settings: { named_wheels: [], reference_lines: [
    { label: "1 min", period_s: 60 }, { label: "1 h", period_s: 3600 }] } } };
  context.showWheels = true;
  assert.deepEqual(plain(context.wheelMarks(plainDoc, [0, 5])), [
    { x: Math.log10(60), color: "--text3", width: 1.25, label: "1 min", ink: "--text2" },
    { x: Math.log10(3600), color: "--text3", width: 1.25, label: "1 h", ink: "--text2" },
  ]);
  // Only those inside the axis.
  assert.equal(context.wheelMarks(plainDoc, [0, 3]).length, 1);
  // A named wheel in the wheels' colour, and its half turn fainter; series
  // gives no reference lines then.
  const named = { input: { settings: { named_wheels: [{ name: "fourth wheel", period_s: 60 }], reference_lines: [] } } };
  assert.deepEqual(plain(context.wheelMarks(named, [0, 5])), [
    { x: Math.log10(60), color: "--wheel", width: 2, label: "fourth wheel", ink: "--wheel", left: false },
    { x: Math.log10(30), color: "--wheel", opacity: 0.6, width: 1.25, dash: "wheel", label: "fourth wheel ×2", ink: "--wheel" },
  ]);
  // On a map (1 to 100 s here), a dashed row at each one inside its periods, named.
  context.mapPaints.length = 0;
  const rows = (doc) => context.steadyPlot({ series: "amplitude" }, "stft", { duration_s: 1800, time_frequency: [map], ...doc })
    .split("map-layer")[1];
  assert.match(rows(plainDoc), /stroke="var\(--text2\)"[^>]*\/>.*>1 min</s);
  assert.ok(!rows(plainDoc).includes(">1 h<"));
  assert.match(rows(named), /stroke="var\(--wheel\)"[^>]*\/>.*>fourth wheel</s);
  // Wheel Periods off: nothing, on the axes or the maps.
  context.showWheels = false;
  assert.deepEqual(plain(context.wheelMarks(named, [0, 5])), []);
  assert.deepEqual(plain(context.wheelMarks(plainDoc, [0, 5])), []);
  assert.ok(!rows(named).includes("<line") && !rows(plainDoc).includes("<line"));
  context.showWheels = true;
});

test("Wheel Periods sits beside each period view's own switch, with the app's hint", () => {
  for (const [view, other] of [["periods", "log"], ["spectrum", "timing"], ["stft", "mapslog"], ["wavelet", "mapslog"]]) {
    context.steadyView = view;
    const bar = context.steadySwitch();
    assert.ok(bar.includes(`data-switch="${other}"`), bar);
    assert.ok(bar.includes('data-switch="wheels" aria-pressed="true"'), bar);
    assert.ok(bar.includes("one minute and one hour are marked in grey"), bar);
  }
  context.steadyView = "readings";
  assert.equal(context.steadySwitch(), "");
});
