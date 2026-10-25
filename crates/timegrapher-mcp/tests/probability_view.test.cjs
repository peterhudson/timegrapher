// Execute the view's own probability plot: the share labels sit at core's
// normal scores, and the paper reaches just past core's highest point.
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");
const html = fs.readFileSync(process.env.TIMEGRAPHER_TEST_VIEW_HTML || path.join(__dirname, "../src/view.html"), "utf8");
function definition(name) {
  const match = html.match(new RegExp(`^function ${name}\\([^\\n]*\\{[\\s\\S]*?^\\}`, "m"));
  if (!match) throw new Error(`Missing renderer ${name}`);
  return match[0];
}

test("the server fills the marks in and the page works out no normal scores", () => {
  assert.ok(html.includes("const PROBABILITY_MARKS = /*PROBABILITY_MARKS*/null;"));
  assert.ok(!html.includes("normalQuantile"));
});

// Core's normal scores for the app's marks, as the server writes them.
const MARKS = [["0.1%", -3.090232], ["1%", -2.326348], ["5%", -1.644854], ["10%", -1.281552], ["25%", -0.67449],
  ["50%", 0], ["75%", 0.67449], ["90%", 1.281552], ["95%", 1.644854], ["99%", 2.326348], ["99.9%", 3.090232]];
const context = vm.createContext({
  PROBABILITY_MARKS: MARKS, chartWidth: () => 600, CHAR_W: 6.6, DASH: { loose: "2 3" },
  plotArea: () => ({ id: "c", svg: "" }), axisLabel: (v) => String(v), gridFor: () => ({ ticks: [], step: 1 }),
  esc: String,
});
vm.runInContext(definition("probabilityChart"), context);

test("the share labels sit at core's scores up to just past the highest point", () => {
  // Twenty values: core's highest point is at the score of 39/40, 1.959964.
  const e = { n: 20, edges: [0, 10], points: [[1, -1.959964], [5, 0], [9, 1.959964]], normal_line: null };
  const svg = context.probabilityChart(e, "--rate", "s/d");
  const labels = [...svg.matchAll(/<text[^>]*>([^<]*)<\/text>/g)].map((m) => m[1]);
  // 95% would crowd 90% at this height, so it is left out.
  assert.deepEqual(labels.sort(), ["10%", "25%", "50%", "75%", "90%"].sort());
  // The paper runs to 1.959964 + 0.25 each way, so 50% sits halfway down and the top dot near the top.
  const y = (t) => Number(svg.match(new RegExp(`y="([\\d.]+)" text-anchor="end">${t}<`))[1]) - 4;
  const plotH = 190 - 4 - 20, zTop = 1.959964 + 0.25;
  assert.ok(Math.abs(y("50%") - (4 + plotH / 2)) < 1e-6);
  assert.ok(Math.abs(y("90%") - (4 + (zTop - 1.281552) / (2 * zTop) * plotH)) < 1e-6);
});
