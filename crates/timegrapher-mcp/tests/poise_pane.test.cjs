// The session view's Dynamic Poise pane (#1729): the upright positions'
// rates in the table, and core's picture beside it along a line (Plot) or
// round a watch face (Dial), with core's words; the view writes none.
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
  const match = html.match(new RegExp(`^(?:const|let) ${name} = [^\\n]+$`, "m"));
  if (!match) throw new Error(`Missing ${name}`);
  return match[0].replace(/^(?:const|let) /, "var ");
}
const context = vm.createContext({ MINUS: "−", CHAR_W: 6.6, ICONS: null, paneWs: {}, chartWidth: () => 1240 });
vm.runInContext(["PANE_MIN_CHART", "PANE_CHAR_W", "poiseShown"].map(constant).join("\n"), context);
vm.runInContext(["esc", "fixedDigits", "axisLabel", "gridAcross", "valueTick", "testPane",
  "icon", "columnHead", "paneChartWidth", "plHead", "poisePane", "poiseDots", "poisePlot", "poiseDial", "poiseKey"].map(definition).join("\n"), context);

const hours = [[0, "12H"], [45, "1H30"], [90, "3H"], [135, "4H30"], [180, "6H"], [225, "7H30"], [270, "9H"], [315, "10H30"]];
const dot = (angle_deg, code, rate) => ({ angle_deg, code, rate_s_per_day: rate, rate_se: 0.2, text: `${rate} s/d`, figure: `${rate}` });
const picture = {
  points: [dot(0, "12H", 2), dot(90, "3H", 3), dot(180, "6H", -2), dot(270, "9H", -1), dot(45, "1H30", 3)],
  wave: [[0, 1], [180, -1], [360, 1]],
  fastest: { at: 60, rate_s_per_day: 3, name: "Fastest 2H" },
  slowest_deg: 240,
  mean: { at: 0.5, rate_s_per_day: 0.5, name: "Mean +0.5", figure: "+0.5" }, unit: "s/d", mean_title: "Mean", rate_title: "Rate",
  axis: [-4, 4, 2], key: ["Upright positions (5)", "Fitted wave"],
  heavy: ["Heavy spot", "8H at the top, 240 ± 20°"], hours,
  title: "Dynamic Poise", hour_axis: "Hour at the top", dial_key: "Further out = faster, s/d",
};
const p = { wind_h: 0, fit: { text: "Core words on the fit." }, picture };

test("the table gives each upright position's rate, then the mean", () => {
  const markup = context.poisePane(p, "p0", null);
  assert.ok(markup.includes("<h2>Dynamic Poise</h2>"), markup);
  assert.ok(markup.includes('<th>Rate<span class="unit">s/d</span></th>'), markup);
  assert.ok(/<tr data-pos="3H"><td>3H<\/td><td>3<\/td><\/tr>/.test(markup), markup);
  assert.ok(markup.includes('<tr class="avg"><td>Mean</td><td>+0.5</td></tr>'), markup);
  assert.ok(markup.includes("8H at the top, 240 ± 20°"), "the heavy spot stays in view");
  assert.ok(/<details class="info">[\s\S]*Core words on the fit\.[\s\S]*<\/details>/.test(markup), "core's words behind the i");
});

test("several states of wind name the pane for its state", () => {
  assert.ok(context.poisePane(p, "p0", "Full wind").includes("<h2>Dynamic Poise, Full wind</h2>"));
});

test("Plot draws 12H at both ends, the wave, the mean and the fastest hour", () => {
  const markup = context.poisePane(p, "p0", null);
  assert.ok(markup.includes('data-poise="plot" aria-pressed="true"'));
  const svg = context.poisePlot(picture);
  assert.equal((svg.match(/>12H</g) || []).length, 2, "12H at both ends");
  assert.equal((svg.match(/data-pos="12H"/g) || []).length, 1, "one lane");
  assert.equal((svg.match(/<g class="lane" data-pos="12H"[\s\S]*?<\/g>/)[0].match(/r="4.2"/g) || []).length, 2, "its point at each end");
  assert.ok(svg.includes(">Fastest 2H<") && svg.includes(">Mean +0.5<") && svg.includes(">Hour at the top<"));
  assert.ok(markup.includes(">Upright positions (5)<") && markup.includes(">Fitted wave<"), "the key over both charts");
});

test("Dial draws the hours round the face, the key, and a dot per position", () => {
  context.poiseShown.plot = false;
  context.poiseShown.dial = true;
  try {
    const markup = context.poisePane(p, "p0", null);
    assert.ok(markup.includes('data-poise="dial" aria-pressed="true"') && markup.includes('data-poise="plot" aria-pressed="false"'));
    assert.ok(markup.includes(">Further out = faster, s/d<"));
    for (const [, code] of hours) assert.ok(markup.includes(`>${code}<`), code);
    assert.equal((markup.match(/<g class="lane"/g) || []).length, 5);
  } finally {
    context.poiseShown.plot = true;
    context.poiseShown.dial = false;
  }
});

test("Plot and Dial both on draw both", () => {
  context.poiseShown.dial = true;
  try {
    const markup = context.poisePane(p, "p0", null);
    assert.ok(markup.includes(">Hour at the top<") && markup.includes(">Further out = faster, s/d<"));
  } finally {
    context.poiseShown.dial = false;
  }
});

test("Plot and Dial sit side by side where both fit", () => {
  context.poiseShown.dial = true;
  try {
    const markup = context.poisePane(p, "p0", null);
    assert.equal((markup.match(/<div class="poise-chart">/g) || []).length, 2);
    const widths = [...markup.matchAll(/<svg viewBox="0 0 (\d+) \d+" width="(\d+)"/g)].map((m) => +m[2]);
    assert.ok(widths.every((w) => w <= (1240 - 380 - 12) / 2), widths.join());
  } finally {
    context.poiseShown.dial = false;
  }
});

test("the hover sits in the chart's title row, above the heavy spot and the key", () => {
  const markup = context.poisePane(p, "p0", null);
  const head = markup.indexOf('<div class="pl-head"><b>Rate</b>');
  assert.ok(head > 0, markup);
  assert.ok(head < markup.indexOf("8H at the top") && head < markup.indexOf("poise-key"), "title row first");
});

test("no ring number sits on a point or its ± bar", () => {
  // A point right where the ring numbers run (half way from the slowest hour).
  const bearing = picture.slowest_deg + 180 / hours.length;
  // Its dot on the +2 ring, its ± bar reaching the +4 ring.
  const pic = { ...picture, points: [...picture.points, { ...dot(bearing, "X", 2), rate_se: 2 }] };
  const svg = context.poiseDial(pic, 400);
  const labels = [...svg.matchAll(/<text x="([\d.]+)" y="([\d.]+)" text-anchor="middle" style="fill:var\(--text3\)">([^<]*)</g)].map((m) => m[3]);
  assert.ok(labels.length, svg);
  for (const v of ["+2", "+4"]) assert.ok(!labels.includes(v), `${v} on the point: ${labels}`);
});
