// Execute the view's own charts over time: each states the averaging time
// the server read them at, and takes its scale from the server.
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");
const html = fs.readFileSync(process.env.TIMEGRAPHER_TEST_VIEW_HTML || path.join(__dirname, "../src/view.html"), "utf8");
const definition = (name) => {
  const match = html.match(new RegExp(`^function ${name}\\([^\\n]*\\{[\\s\\S]*?^\\}`, "m"));
  if (!match) throw new Error(`Missing renderer ${name}`);
  return match[0];
};
const charts = [];
const context = vm.createContext({
  num: (v, d) => v.toFixed(d),
  signed: (v, d) => (v >= 0 ? "+" : "") + v.toFixed(d),
  lineChart: (c) => { charts.push(JSON.parse(JSON.stringify(c))); return "<svg/>"; },
});
vm.runInContext(definition("rateBasis") + "\n" + definition("stripCards"), context);

const strip = {
  duration_s: 30, average_s: 20,
  y_range: { rate: [4, 5], amplitude: [260, 280], beat_error: [-0.5, -0.3] },
  rate: [[2, 4.5]], amplitude: [[2, 269]], beat_error_unlock: [[2, -0.38]],
};

test("each chart over time names the averaging time and keeps the server's scale", () => {
  charts.length = 0;
  const markup = context.stripCards(strip);
  assert.equal(charts[0].series[0].name, "Rate in seconds per day, 20 s average");
  assert.deepEqual(charts.map((c) => c.yRange), [[4, 5], [260, 280], [-0.5, -0.3]]);
  assert.equal((markup.match(/20 s average<\/p>/g) || []).length, 3, "every chart's hint");
  assert.ok(markup.includes("on the sound card's clock, 20 s average"), markup);
});

test("a rate on a corrected clock says so, with the correction", () => {
  const markup = context.stripCards({ ...strip, clock_ppm: 20 });
  assert.ok(markup.includes("corrected for the sound card's clock (+20.0 ppm), 20 s average"), markup);
});

test("without an averaging time no average is claimed", () => {
  charts.length = 0;
  const markup = context.stripCards({ ...strip, average_s: null });
  assert.equal(charts[0].series[0].name, "Rate in seconds per day");
  assert.ok(!markup.includes("average"));
});
