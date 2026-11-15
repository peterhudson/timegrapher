// The session view's Power Reserve pane (#1729): the hours table with core's
// figures and Read %, and a chart per measure of the hourly levels as steps,
// one line broken only where an hour has no figure; a thin figure, or a beat
// error not taken off the unlock, shown grey.
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
// Core's band fill, as the server puts it in.
const paletteRs = fs.readFileSync(path.join(__dirname, "../../timegrapher-core/src/palette.rs"), "utf8");
const BAND_FILL = +paletteRs.match(/pub const BAND_FILL: f64 = ([\d.]+);/)[1];
assert.ok(html.includes("const BAND_FILL = /*BAND_FILL*/null;"), "the server fills the band fill in");
const context = vm.createContext({ MINUS: "−", CHAR_W: 6.6, ICONS: null, paneWs: {}, chartWidth: () => 860, BAND_FILL });
vm.runInContext(["PANE_MIN_CHART", "PANE_CHAR_W", "paneShown", "PANE_KINDS"].map(constant).join("\n"), context);
vm.runInContext(["esc", "fixedDigits", "roundStep", "gridFor", "axisLabel", "gridDown", "gridAcross", "valueTick",
  "testPane", "icon", "columnHead", "paneChartWidth", "plHead", "paneToggles", "reservePane", "stepChart", "tickLabelsShown"].map(definition).join("\n"), context);

const step = (value, text, grey) => ({ value, text, figure: text && text.split(" ")[0].replace("°", ""), grey: !!grey });
const reserve = {
  title: "Power Reserve", hours_title: "Hours", read_title: "Read", read_unit: "%", position: "DU",
  hours: [0, 1, 2, 3].map((hour) => ({ hour, span_h: hour === 3 ? 0.5 : 1, read: hour === 1 ? null : "90" })),
  series: [
    { series: "rate", title: "Rate", unit: "s/d", axis: [-2, 4, 2],
      steps: [step(-1, "−1.0 s/d"), step(null, null), step(2, "+2.0 s/d", true), step(3, "+3.0 s/d")] },
    { series: "amplitude", title: "Amplitude", unit: "°", axis: [260, 300, 10],
      steps: [step(290, "290°"), step(285, "285°"), step(280, "280°"), step(270, "270°")] },
    { series: "beat_error", title: "Beat Error", unit: "ms", axis: null,
      steps: [step(null, null), step(null, null), step(null, null), step(null, null)] },
  ],
};

test("the table gives each hour core's bare figures and its Read, the units under the titles", () => {
  const markup = context.reservePane(reserve, "r");
  assert.ok(markup.includes("<h2>Power Reserve</h2>"), markup);
  const head = [...markup.match(/<thead>(.*?)<\/thead>/)[1].matchAll(/<th>([^<]*)(?:<span class="unit">([^<]*)<\/span>)?<\/th>/g)].map((m) => [m[1], m[2] || ""]);
  assert.deepEqual(head, [["Hours", ""], ["Rate", "s/d"], ["Amplitude", "°"], ["Read", "%"]]);
  const row = (i) => [...markup.match(new RegExp(`<tr data-pos="h${i}">(.*?)</tr>`))[1].matchAll(/<td[^>]*>([^<]*)<\/td>/g)].map((m) => m[1]);
  assert.deepEqual(row(0), ["0", "−1.0", "290", "90"]);
  assert.deepEqual(row(1), ["1", "–", "285", "–"]);
  assert.ok(/<td class="thin">\+2\.0<\/td>/.test(markup), "a grey figure is grey");
});

test("each measure's hours are one line of steps, broken where an hour has none", () => {
  const chart = context.stepChart(reserve.series[0], reserve.hours, "r-rate");
  const paths = [...chart.matchAll(/<path d="([^"]*)"/g)].map((m) => m[1]);
  assert.equal(paths.length, 1, "one line");
  assert.equal((paths[0].match(/M/g) || []).length, 2, "hour 1 has no rate: the line breaks there");
  assert.ok(/V[\d.]+ H/.test(paths[0]), "hours 2 and 3 joined by a riser");
  assert.ok(!chart.includes("<rect class=\"bar"), "no bars");
});

test("a measure's toggle hides its chart, and one not measured has none", () => {
  const charts = (m) => [...m.matchAll(/data-pl="r-([a-z_]+)"/g)].map((x) => x[1]);
  assert.deepEqual(charts(context.reservePane(reserve, "r")), ["rate", "amplitude"]);
  context.paneShown.rate = false;
  try {
    assert.deepEqual(charts(context.reservePane(reserve, "r")), ["amplitude"]);
  } finally {
    context.paneShown.rate = true;
  }
});

test("the rate's ticks carry their sign, the unit after the last", () => {
  const chart = context.stepChart(reserve.series[0], reserve.hours, "r-rate");
  const ticks = [...chart.matchAll(/text-anchor="end">([^<]*)<\/text>/g)].map((m) => m[1]);
  assert.deepEqual(ticks.slice(0, 4), ["−2", "0", "+2", "+4 s/d"]);
});

test("no two hour labels touch, at the narrowest chart", () => {
  context.chartWidth = () => 380 + 320;
  try {
    const hours = Array.from({ length: 5 }, (_, hour) => ({ hour, span_h: hour === 4 ? 0.55 : 1, read: "90" }));
    const chart = context.stepChart({ ...reserve.series[1], steps: hours.map(() => step(280, "280°")) }, hours, "r-amplitude");
    // The hours' labels sit under the plot (T 8 + 140 + 15).
    const xs = [...chart.matchAll(/<text x="([\d.]+)" y="163" text-anchor="(middle|end)">([^<]*)<\/text>/g)]
      .map((m) => ({ x: +m[1], end: m[2] === "end", text: m[3] }));
    assert.ok(xs.length >= 2 && xs[xs.length - 1].text.endsWith(" h"), "the last carries the unit");
    const cw = context.PANE_CHAR_W;
    for (let i = 1; i < xs.length; i++) {
      const right = (t) => (t.end ? t.x : t.x + t.text.length * cw / 2);
      const left = (t) => (t.end ? t.x - t.text.length * cw : t.x - t.text.length * cw / 2);
      assert.ok(right(xs[i - 1]) < left(xs[i]), `${xs[i - 1].text} touches ${xs[i].text}`);
    }
  } finally {
    context.chartWidth = () => 860;
  }
});

test("the first hour's label starts at the axis, clear of the lowest value's", () => {
  const chart = context.stepChart(reserve.series[0], reserve.hours, "r-rate");
  assert.ok(/<text x="52" y="163" text-anchor="start">0<\/text>/.test(chart), chart);
});

test("the grey key shows under the table only when core sends it", () => {
  assert.ok(!context.reservePane(reserve, "r").includes('class="map-key"'));
  const key = "Grey beat errors: from the drop, or source not recorded.";
  const markup = context.reservePane({ ...reserve, grey_key: key }, "r");
  assert.ok(markup.includes(`<div class="map-key">${key}</div>`), markup);
});

test("each hour's ± band lies under its step, in its line's colour or grey with it", () => {
  const steps = reserve.series[0].steps.map((st, i) => ({ ...st, band: [[-1.5, -0.5], [0, 1], [1, 3], null][i] }));
  const chart = context.stepChart({ ...reserve.series[0], steps }, reserve.hours, "r-rate");
  const washes = [...chart.matchAll(/<path class="band" d="([^"]*)" fill="([^"]+)" fill-opacity="([\d.]+)"\/>/g)];
  // Hour 1 has no figure and hour 3 no band: hour 0's band in the line's
  // colour, hour 2's grey, each ink one path.
  assert.deepEqual(washes.map((w) => w[2]), ["var(--rate)", "var(--text3)"], chart);
  // Axis −2 to 4 over 140 px from y 8: −0.5 is at 113.0, −1.5 at 136.3.
  assert.match(washes[0][1], /^M52\.0 113\.0H[\d.]+V136\.3H52\.0Z$/);
  assert.equal((washes[1][1].match(/M/g) || []).length, 1);
  assert.ok(washes.every((w) => +w[3] === BAND_FILL));
  assert.ok(chart.indexOf('class="band"') < chart.indexOf('<path d='), "under the line");
  // Two hours' bands of one ink are one path: no seam between them.
  const both = reserve.series[0].steps.map((st, i) => ({ ...st, grey: false, band: st.value == null ? null : [st.value - 0.5, st.value + 0.5] }));
  const one = context.stepChart({ ...reserve.series[0], steps: both }, reserve.hours, "r-rate");
  const paths = [...one.matchAll(/<path class="band" d="([^"]*)"/g)];
  assert.equal(paths.length, 1);
  assert.equal((paths[0][1].match(/M/g) || []).length, 3);
});
