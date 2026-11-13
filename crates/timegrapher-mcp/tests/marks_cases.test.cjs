// Run the view's copy of the app's rules for naming a plot's lines
// (marks.rs) on the cases the app's own tests run, and draw the names as the
// Periods and FFT views do.
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");
const html = fs.readFileSync(process.env.TIMEGRAPHER_TEST_VIEW_HTML || path.join(__dirname, "../src/view.html"), "utf8");
const cases = JSON.parse(fs.readFileSync(path.join(__dirname, "../../timegrapher-app/src/marks_cases.json"), "utf8"));
function definition(name) {
  const match = html.match(new RegExp(`^function ${name}\\([^\\n]*\\{[\\s\\S]*?^\\}`, "m"));
  if (!match) throw new Error(`Missing renderer ${name}`);
  return match[0];
}
function constant(name) {
  const match = html.match(new RegExp(`^const ${name} = .*$`, "m"));
  if (!match) throw new Error(`Missing ${name}`);
  return match[0];
}
const context = vm.createContext({ document: { getElementById: () => ({ clientWidth: 624 }) } });
vm.runInContext(["CHAR_W", "DASH", "MARKS", "NAME_CLEAR"].map(constant).join("\n") + "\nlet clipCount = 0;\n" +
  ["esc", "chartWidth", "plotArea", "gridDown", "gridAcross", "xyChart", "markOrder", "markPlace", "markPlaceRow", "markInside", "markStack", "besideSpots", "crosses", "besidePlace", "roomAbove", "markWidth", "markText", "laneAbove", "namesInside", "sideWidth", "laneRight"]
    .map(definition).join("\n") + "\nthis.MARKS = MARKS;", context);

const plain = (v) => JSON.parse(JSON.stringify(v));
const close = (got, want, what) => {
  assert.equal(got.length, want.length, what);
  got.forEach((g, i) => {
    if (want[i] === null) assert.equal(g, null, `${what}: ${JSON.stringify(got)}`);
    else assert.ok(g !== null && Math.abs(g - want[i]) < 1e-3, `${what}: ${JSON.stringify(got)}`);
  });
};

test("names in a lane of two rows go where the app's rules put them", () => {
  for (const c of cases.place) {
    const got = plain(context.markPlace(c.names, c.left, c.right, c.was));
    close(got.map((p) => p[0]), c.at.map((p) => p[0]), c.case);
    assert.deepEqual(got.map((p) => p[1]), c.at.map((p) => p[1]), c.case);
  }
});

test("names in a lane of one row go where the app's rules put them", () => {
  for (const c of cases.place_row) close(plain(context.markPlaceRow(c.names, c.left, c.right)), c.at, c.case);
});

test("names inside a short plot go where the app's rules put them, or nowhere", () => {
  for (const c of cases.inside) {
    const facts = c.facts || [-Infinity, -Infinity];
    const clear = (a, b) => c.trace.every(([lo, hi]) => b < lo || a > hi);
    close(plain(context.markInside(c.names, c.left, c.right, facts, clear)), c.at, c.case);
  }
});

test("names beside a map go where the app's rules put them", () => {
  for (const c of cases.stack) close(plain(context.markStack(c.ys, c.top, c.bottom)), c.at, c.case);
});

test("a note beside a point tries the places the app's rules try, in order", () => {
  for (const c of cases.beside_spots) {
    const got = plain(context.besideSpots(c.dot, c.size, c.top));
    assert.equal(got.length, c.spots.length, c.case);
    got.forEach((r, i) => close(r, c.spots[i], c.case));
  }
});

test("a trace crosses a note's place only where the app's rules say", () => {
  for (const c of cases.crosses) assert.equal(context.crosses(c.place, c.trace), c.crosses, c.case);
});

test("a note takes the place the app's rules give it, or none", () => {
  for (const c of cases.beside_place) {
    const spots = c.clear.map((_, i) => [i, 0, i + 0.5, 1]);
    assert.equal(context.besidePlace(spots, (r) => c.clear[r[0]], c.was), c.at, c.case);
  }
});

test("a plot keeps a note's room above its highest point as the app's does", () => {
  for (const c of cases.room_above) close([context.roomAbove(c.lo, c.y, c.top, c.h)], [c.at], c.case);
});

// A Periods or FFT plot, 1 s to 1000 s, its wheels' lines as wheelMarks
// gives them.
const line = (x, name, rank, extra = {}) => ({ x: Math.log10(x), color: "--callout", width: 1.2, dash: "dense", name, rank, ink: "--callout", hover: `${name}: ${x} s`, ...extra });
const plot = (o) => context.xyChart({
  x: [0, 3], y: [0, 1], xTicks: () => [], yTicks: () => [[0, "0"]], lines: [{ pts: [[0, 0.5], [3, 0.5]], color: "--rate" }],
  vlines: [line(60, "Fourth Wheel", "strong"), line(30, "Fourth Wheel ×2", "faint", { width: 1, dash: "sparse" })], ...o,
});

test("a plot's lines are dotted and named above it, joined by a stroke of their colour", () => {
  const svg = plot({ names: true });
  const { LANE, LANE_GAP } = context.MARKS;
  // The lane's room is kept above the plot, which keeps its height.
  assert.match(svg, new RegExp(`height="${150 + LANE + LANE_GAP}"`));
  assert.match(svg, new RegExp(`<rect class="plot" x="[\\d.]+" y="${4 + LANE + LANE_GAP}"`));
  // Dotted: round dots, 5 px apart for a wheel, 10 for its ×2.
  assert.match(svg, /stroke="var\(--callout\)" stroke-width="1.2" stroke-dasharray="0 5" stroke-linecap="round"/);
  assert.match(svg, /stroke="var\(--callout\)" stroke-width="1" stroke-dasharray="0 10" stroke-linecap="round"/);
  // Outside the plot's clip: each name over a patch of the card's colour,
  // the wheel's strong, the ×2's faint, each with its hover.
  const outside = svg.split("clip-path")[0];
  assert.match(outside, /<title>Fourth Wheel: 60 s<\/title><rect[^>]*fill="var\(--card\)"\/><text[^>]*style="fill:var\(--text\);font-weight:600[^"]*">Fourth Wheel<\/text>/);
  assert.match(outside, /<text[^>]*style="fill:var\(--text2\);[^"]*">Fourth Wheel ×2<\/text>/);
  assert.equal((outside.match(/<polyline/g) || []).length, 2);
  // Every name sits above the plot.
  const top = 4 + LANE + LANE_GAP;
  const names = [...outside.matchAll(/<text x="[\d.]+" y="([\d.]+)" style/g)];
  assert.equal(names.length, 2);
  for (const [, y] of names) assert.ok(+y < top, outside);
});

test("with Wheel Periods off a plot has no lane and draws no names", () => {
  const svg = plot({ names: false, vlines: [] });
  assert.match(svg, /height="150"/);
  assert.ok(!svg.includes("<polyline") && !svg.includes("Wheel"));
});

test("on a short plot the names go inside, along its top, clear of the trace", () => {
  const { INSIDE_TOP, ROW_H } = context.MARKS;
  const svg = plot({ names: true, height: 110 });
  assert.match(svg, /height="110"/, "no lane's room");
  const inside = svg.split("clip-path")[1];
  assert.match(inside, /<rect[^>]*fill="var\(--card\)" fill-opacity="0.8"\/><text[^>]*>Fourth Wheel<\/text>/);
  // A trace across the row of names leaves none named: the app judges it
  // by the trace's points, which are dense.
  const pts = Array.from({ length: 301 }, (_, i) => [i / 100, 0.9]);
  const high = plot({ names: true, height: 110, lines: [{ pts, color: "--rate" }] });
  assert.ok(!high.includes(">Fourth Wheel<"), high);
  assert.ok(INSIDE_TOP + ROW_H < 110);
});

test("a marked point's note sits on a plate beside its dot, clear of the trace, with a leader", () => {
  // A peak at 60 s rising to 0.9 of the plot: the trace falls away on
  // both sides, so the first place, above right, is clear.
  const pts = [[0, 0.1], [Math.log10(60), 0.9], [3, 0.1]];
  const svg = plot({ names: false, vlines: [], lines: [{ pts, color: "--rate" }], noteRoom: true,
    beside: [{ x: Math.log10(60), y: 0.9, text: "60 s, Fourth Wheel" }] });
  const inside = svg.split("clip-path")[1];
  assert.match(inside, /<line [^>]*stroke="var\(--text2\)" stroke-width="1"\/><rect [^>]*fill="var\(--card\)" fill-opacity="0.8"\/><text [^>]*>60 s, Fourth Wheel<\/text>/);
  // The plot is kept tall enough for the note's room above the peak: the
  // top of its range is above 1.
  const plate = inside.match(/<rect x="([\d.]+)" y="([\d.]+)" width="([\d.]+)" height="21.0" rx="6"/);
  assert.ok(plate && +plate[2] >= 5, inside);
  // On a short plot the note is the dot's hover.
  const short = plot({ names: false, vlines: [], height: 110, lines: [{ pts, color: "--rate" }], noteRoom: true,
    beside: [{ x: Math.log10(60), y: 0.9, text: "60 s, Fourth Wheel" }] });
  assert.ok(!short.includes(">60 s, Fourth Wheel</text>"));
  assert.match(short, /<circle [^>]*fill="transparent"><title>60 s, Fourth Wheel<\/title><\/circle>/);
});

test("a note keeps clear of the Ljung–Box note in the corner", () => {
  // A dot near the top right: above right is off the plot and above left
  // would cover the corner's note, so it goes below left.
  const svg = plot({ names: false, vlines: [], lines: [{ pts: [[0, 0.05], [3, 0.05]], color: "--rate" }],
    corner: { text: "Ljung–Box p = 0.001", top: true }, beside: [{ x: 2.9, y: 0.8, text: "repeats at 2:00" }] });
  const plate = svg.match(/<rect x="([\d.]+)" y="([\d.]+)" width="([\d.]+)" height="21.0" rx="6"/);
  assert.ok(plate, svg);
  const [, x, y, w] = plate.map(Number);
  const corner = svg.match(/<text x="([\d.]+)" y="([\d.]+)" text-anchor="end">Ljung/);
  const right = +corner[1], left = right - "Ljung–Box p = 0.001".length * 6.6;
  assert.ok(x + w <= left || x >= right || y >= +corner[2] + 4, `${plate[0]} vs corner at ${left}-${right}`);
  // The dot is 0.2 of the plot's 126 px below its top, at 4 px.
  assert.ok(y > 4 + 0.2 * 126, "below the dot");
});

test("a named line across a plot is named right of it, with its hover, and the y axis can carry one", () => {
  const svg = context.xyChart({ x: [0, 1], y: [0, 5], xTicks: () => [], yTicks: () => [[1, "1"], [2, "2"]],
    yHover: "Score: how far a peak stands above the noise expected at its period.",
    hlines: [{ y: 3, color: "--callout", width: 1.2, dash: "loose", name: "Threshold", hover: "Peaks above this line are named as periods." }],
    lines: [{ pts: [[0, 1], [1, 4]], color: "--rate" }] });
  // The plot gives up its right edge to the lane, and the name sits in it.
  const plot = svg.match(/<rect class="plot" x="([\d.]+)" y="[\d.]+" width="([\d.]+)"/);
  const right = +plot[1] + +plot[2];
  const name = svg.match(/<g><title>Peaks above this line are named as periods.<\/title><polyline[^>]*\/><text x="([\d.]+)"[^>]*>Threshold<\/text><\/g>/);
  assert.ok(name, svg);
  assert.ok(+name[1] > right, "named right of the plot");
  assert.match(svg, /stroke-dasharray="6 5"/);
  assert.match(svg, /<g><title>Score: how far a peak stands above the noise expected at its period.<\/title><text[^>]*>1<\/text><text[^>]*>2<\/text><\/g>/);
});

test("a y axis' name sits above its top tick, left of the plot, in the ticks' style and with their hover", () => {
  const chart = (names) => context.xyChart({ x: [0, 1], y: [0, 5], xTicks: () => [], yTicks: () => [[1, "1"], [5, "5"]],
    yName: "Score", yHover: "Score: how far a peak stands above the noise expected at its period.", names,
    vlines: names ? [{ x: 0.02, color: "--callout", name: "1 min", rank: "faint", ink: "--callout", hover: "1 min" }] : [],
    lines: [{ pts: [[0, 1], [1, 4]], color: "--rate" }] });
  for (const names of [false, true]) {
    const svg = chart(names);
    const plot = svg.match(/<rect class="plot" x="([\d.]+)" y="([\d.]+)"/).slice(1).map(Number);
    const name = svg.match(/<g><title>Score: how far[^<]*<\/title>(?:<text[^>]*>[^<]*<\/text>)*<text x="([\d.]+)" y="([\d.]+)" text-anchor="end">Score<\/text><\/g>/);
    assert.ok(name, svg);
    // Right of nothing in the plot: it ends where the tick labels end.
    assert.equal(+name[1], plot[0] - 5);
    // Above the plot's top, and inside the chart.
    assert.ok(+name[2] < plot[1] && +name[2] > 0, `${name[2]} vs ${plot[1]}`);
    // No style of its own: the tick labels' size and colour.
    assert.ok(!/style="[^"]*">Score</.test(svg));
  }
  // With no lane above, the plot keeps a row for it.
  assert.ok(+chart(false).match(/<rect class="plot" x="[\d.]+" y="([\d.]+)"/)[1] >= 14);
});
