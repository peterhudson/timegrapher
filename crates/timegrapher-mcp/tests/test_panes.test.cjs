// The session view's Positions Test panes (#1729): one per state of wind,
// headed in core's words (a reading whose wind was not given is never shown
// as full wind, #1665), the table from core's position lines with the
// averages under it, a chart per measure each shown by its toggle, and core's
// figures as they are: the view writes none of its own.
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");
const html = fs.readFileSync(process.env.TIMEGRAPHER_TEST_VIEW_HTML || path.join(__dirname, "../src/view.html"), "utf8");
function definition(name) {
  const match = html.match(new RegExp(`^function ${name}\\([^\\n]+\\}\\s*$`, "m")) || html.match(new RegExp(`^function ${name}\\([^\\n]*\\{[\\s\\S]*?^\\}`, "m"));
  if (!match) throw new Error(`Missing ${name}`);
  return match[0];
}
function constant(name) {
  const match = html.match(new RegExp(`^const ${name} = [^\\n]+$`, "m"));
  if (!match) throw new Error(`Missing ${name}`);
  return match[0].replace(/^const /, "var ");
}
const context = vm.createContext({ NO_BEAT_TITLE: "", POSITIONS: null, GROUP_NAMES: { horizontal: "Horizontal", vertical: "Vertical" },
  MINUS: "−", tile: () => "", signed: String, num: String, abs: Math.abs, limitsCard: () => "",
  CHAR_W: 6.6, CALL_OUT_W: 1.2, DASH: { dashed: "4 3" }, ICONS: null, paneWs: {}, chartWidth: () => 860 });
vm.runInContext(["PANE_MIN_CHART", "PANE_CHAR_W", "paneShown", "PANE_KINDS", "LANE_H", "MARKS"].map(constant).join("\n"), context);
vm.runInContext(["esc", "fixedDigits", "axisLabel", "gridDown", "markOrder", "markPlace", "markPlaceRow", "markWidth", "markText", "laneAbove",
  "sessionView", "testPane", "icon", "columnHead", "paneChartWidth", "plHead", "paneToggles", "positionsPane", "positionLineChart", "valueTick", "tickLabelsShown"].map(definition).join("\n"), context);

const lane = (position, group, value, text, full) => ({ position, group, name: `${position} name`, value, text, figure: text.split(" ")[0].replace("°", ""),
  full_wind: full == null ? null : full, full_wind_text: full == null ? null : `+${full}.0 s/d`, hover: `${position} name ${text}` });
const line = (series, title, unit, lanes, key) => ({ series, title, unit, lanes, key: key || null,
  averages: [{ of: "all", name: "Average", value: 2, text: "+2.0 s/d", figure: "+2.0", hover: "Average +2.0 s/d" }],
  axis: [0, 4, 1] });
const state = (wind_h, name, key) => ({ wind_h, name,
  rate: line("rate", "Rate", "s/d", [lane("DU", "horizontal", 1, "+1.0 s/d", key && 3), lane("CU", "vertical", 3, "+3.0 s/d")], key),
  amplitude: line("amplitude", "Amplitude", "°", [lane("DU", "horizontal", 270, "270°"), lane("CU", "vertical", 260, "260°")]),
  beat_error: line("beat_error", "Beat Error", "ms", [lane("DU", "horizontal", 0.2, "0.20 ms"), lane("CU", "vertical", 0.3, "0.30 ms")]) });
const doc = { readings: [], report: {
  states: [{ wind_h: 0 }, { wind_h: 24 }, { wind_h: null }],
  position_lines: [state(0, "Full wind"), state(24, "24 h after winding", ["This test", "Full wind test"]), state(null, "Wind not given")],
} };

test("each state of wind is a pane of its own, headed in core's words", () => {
  const markup = context.sessionView(doc, null);
  const heads = [...markup.matchAll(/<h2>([^<]*)<\/h2>/g)].map((m) => m[1]);
  assert.deepEqual(heads, ["Full wind", "24 h after winding", "Wind not given"]);
});

test("the table gives core's bare figures for each lane, the units under the titles, then the averages", () => {
  const markup = context.positionsPane(doc.report.position_lines[0], [], "s0", false);
  const row = markup.match(/<tr data-pos="CU">(.*?)<\/tr>/)[1];
  assert.deepEqual([...row.matchAll(/<td>([^<]*)<\/td>/g)].map((m) => m[1]), ["CU name", "+3.0", "260", "0.30"]);
  const heads = [...markup.matchAll(/<th>([^<]*)(?:<span class="unit">([^<]*)<\/span>)?<\/th>/g)].map((m) => [m[1], m[2] || ""]);
  assert.deepEqual(heads, [["Position", ""], ["Rate", "s/d"], ["Amplitude", "°"], ["Beat Error", "ms"]]);
  assert.ok(/<tr class="avg"><td>Average<\/td><td>\+2\.0<\/td>/.test(markup), markup);
});

test("a chart per measure, each shown only while its toggle is on", () => {
  const charts = (m) => [...m.matchAll(/data-pl="s0-([a-z_]+)"/g)].map((x) => x[1]);
  assert.deepEqual(charts(context.positionsPane(doc.report.position_lines[0], [], "s0", false)), ["rate", "amplitude", "beat_error"]);
  context.paneShown.amplitude = false;
  try {
    const markup = context.positionsPane(doc.report.position_lines[0], [], "s0", false);
    assert.deepEqual(charts(markup), ["rate", "beat_error"]);
    assert.ok(markup.includes(`data-series="amplitude" aria-pressed="false"`), markup);
  } finally {
    context.paneShown.amplitude = true;
  }
});

test("a later state draws the full-wind figure hollow with an arrow, and keys it", () => {
  const chart = context.positionLineChart(doc.report.position_lines[1].rate, "s1-rate");
  assert.ok(chart.includes("This test") && chart.includes("Full wind test"), chart);
  assert.ok(chart.includes('marker-end="url(#pl-arrow-s1-rate)"'), "an arrow from the full-wind figure to this test's");
  assert.ok(chart.includes('data-val="+1.0 s/d · Full wind test +3.0 s/d"'), chart);
  assert.ok(!context.positionLineChart(doc.report.position_lines[0].rate, "s0-rate").includes("marker-end"));
});

test("lanes run under their group's name, and no position is joined to another", () => {
  const chart = context.positionLineChart(doc.report.position_lines[0].rate, "s0-rate");
  assert.ok(chart.indexOf(">Horizontal<") < chart.indexOf('data-pos="DU"'));
  assert.ok(chart.indexOf(">Vertical<") < chart.indexOf('data-pos="CU"'));
  assert.ok(!chart.includes("<path"), "no line through the dots");
});

test("a rate's ticks carry their sign and the unit follows the last", () => {
  const chart = context.positionLineChart(doc.report.position_lines[0].rate, "s0-rate");
  const ticks = [...chart.matchAll(/text-anchor="(?:middle|end)">([^<]*)<\/text>/g)].map((m) => m[1]);
  assert.deepEqual(ticks.slice(-5), ["0", "+1", "+2", "+3", "+4 s/d"]);
});

test("narrow steps never draw two tick labels touching; the last keeps the unit", () => {
  // A one-position session's axis (Walter on #1747): steps of 0.02 s/d.
  const l = { ...doc.report.position_lines[0].rate, axis: [-13.0, -12.9, 0.02] };
  const chart = context.positionLineChart(l, "s0-rate");
  const xs = [...chart.matchAll(/<text x="([\d.]+)" y="[\d.]+" text-anchor="(middle|end)">([^<]*)<\/text>/g)]
    .map((m) => ({ x: +m[1], end: m[2] === "end", text: m[3] }));
  assert.ok(xs[xs.length - 1].text.endsWith(" s/d"), "the last carries the unit");
  for (let i = 1; i < xs.length; i++) {
    const cw = context.PANE_CHAR_W;
    const right = (t) => (t.end ? t.x : t.x + t.text.length * cw / 2);
    const left = (t) => (t.end ? t.x - t.text.length * cw : t.x - t.text.length * cw / 2);
    assert.ok(right(xs[i - 1]) < left(xs[i]), `${xs[i - 1].text} touches ${xs[i].text}`);
  }
});

test("the toggles are the series' icons, titled, tinted in their colour", () => {
  const markup = context.positionsPane(doc.report.position_lines[0], [], "s0", false);
  const t = [...markup.matchAll(/<button class="icon-toggle" data-series="([a-z_]+)" aria-pressed="(true|false)" title="([^"]*)" aria-label="[^"]*" style="--tint:var\(([^)]*)\)">/g)].map((m) => [m[1], m[3], m[4]]);
  assert.deepEqual(t, [["rate", "Rate", "--rate"], ["amplitude", "Amplitude", "--amplitude"], ["beat_error", "Beat Error", "--beat-error"]]);
});

test("a lane hovers its full name, and the value in the series' colour", () => {
  const chart = context.positionLineChart(doc.report.position_lines[0].rate, "s0-rate");
  assert.ok(chart.includes('data-label="CU name"'), chart);
  assert.ok(/<span class="pl-val" style="color:var\(--rate\)">/.test(chart), chart);
});

test("a pane chart is drawn at its pixel size, so its text stays 12px", () => {
  const chart = context.positionLineChart(doc.report.position_lines[0].rate, "s0-rate");
  const m = chart.match(/<svg viewBox="0 0 (\d+) (\d+)" width="(\d+)" height="(\d+)"/);
  assert.ok(m && m[1] === m[3] && m[2] === m[4], chart);
});
