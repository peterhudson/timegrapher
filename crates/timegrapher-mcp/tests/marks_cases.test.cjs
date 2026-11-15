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
const context = vm.createContext({ document: { getElementById: () => ({ clientWidth: 624 }) }, WHEEL_WORDS: { strong_hold: 100, name_clear: 11, name_row_out: 26 } });
vm.runInContext(["CHAR_W", "DASH", "MARKS", "NAME_CLEAR"].map(constant).join("\n") + "\nlet clipCount = 0;\n" +
  ["esc", "chartWidth", "plotArea", "gridDown", "gridAcross", "xyChart", "laneRows", "rowOut", "markOrder", "markPlace", "markPlaceRow", "markInside", "besideSpots", "crosses", "besidePlace", "roomAbove", "markWidth", "markText", "laneAbove", "namesInside", "laneBelow", "sideWidth", "laneRight", "laneRightAt", "markStackNames"]
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

test("names beside a plot keep a clear gap, a word to a line, a strong one on its line (#1764)", () => {
  context.NAME_LINE_H = 13;
  const ink = "--callout";
  const lines = [
    { y: 60, name: "Fourth Wheel", words: ["Fourth", "Wheel"], rank: "strong", ink },
    { y: 66, name: "Third Pinion Mesh", words: ["Third", "Pinion", "Mesh"], rank: "faint", ink },
  ];
  const svg = context.laneRight(lines, 300, 0, 200);
  const ys = [...svg.matchAll(/<text x="[\d.]+" y="([\d.]+)"[^>]*>([^<]+)</g)].map((m) => [m[2], Number(m[1])]);
  assert.deepEqual(ys.map(([w]) => w), ["Fourth", "Wheel", "Third", "Pinion", "Mesh"]);
  // The wheel's name centred on its line; within a name, one line apart.
  assert.ok(Math.abs(ys[0][1] - (60 - 13 + 9.5)) < 1, `${ys}`);
  assert.ok(Math.abs(ys[1][1] - ys[0][1] - 13) < 0.1);
  // Between two names, more than a line clear: the gap reads as a gap.
  assert.ok(ys[2][1] - ys[1][1] >= 13 + 11 - 1e-9, `${ys}`);
  assert.ok(context.sideWidth(lines) < context.sideWidth([{ name: "Third Pinion Mesh", rank: "faint" }]), "the lane as wide as its longest word");
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
  const { ROW_H, LANE_GAP } = context.MARKS, out = context.WHEEL_WORDS.name_row_out;
  // The lane's row is kept above the plot, which keeps its height; the
  // ×2's name can't sit beside the wheel's, so a second row comes, a row
  // and a clear line out (#1764).
  assert.match(svg, new RegExp(`height="${150 + ROW_H + LANE_GAP + out}"`));
  assert.match(svg, new RegExp(`<rect class="plot" x="[\\d.]+" y="${4 + ROW_H + LANE_GAP + out}"`));
  const one = plot({ names: true, vlines: [line(60, "Fourth Wheel", "strong")] });
  assert.match(one, new RegExp(`height="${150 + ROW_H + LANE_GAP}"`), "one row where the names fit");
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
  const top = 4 + ROW_H + LANE_GAP + out;
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

test("a mesh is named in a lane under the plot's tick labels, its line run on down to it (#1764)", () => {
  const mesh = line(7.5, "Fourth Pinion Mesh", "faint", { width: 1, dash: "mesh", below: true });
  const plain = plot({ names: true, vlines: [line(60, "Fourth Wheel", "strong")] });
  const svg = plot({ names: true, vlines: [line(60, "Fourth Wheel", "strong"), mesh] });
  const { LANE_GAP, ROW_H } = context.MARKS;
  // The chart keeps its height; the lane under the plot takes a row of it.
  const h = (s) => +s.match(/<svg[^>]*height="([\d.]+)"/)[1];
  assert.equal(h(svg), h(plain));
  const plotOf = (s) => s.match(/<rect class="plot" x="[\d.]+" y="([\d.]+)" width="[\d.]+" height="([\d.]+)"/).slice(1).map(Number);
  assert.deepEqual(plotOf(svg), [plotOf(plain)[0], plotOf(plain)[1] - ROW_H - LANE_GAP]);
  const [top, ph] = plotOf(svg);
  const outside = svg.split("clip-path")[0];
  const at = (name) => +outside.match(new RegExp(`<text x="[\\d.]+" y="([\\d.]+)" style="[^"]*">${name}<`))[1];
  assert.ok(at("Fourth Wheel") < top, "a wheel above");
  assert.ok(at("Fourth Pinion Mesh") > top + ph + 15, "a mesh under the tick labels");
  assert.equal((outside.match(/<polyline/g) || []).length, 2, "each name joined to its line");
});

test("names beside a plot keep their lines' order, a strong one on its line, as core's lane_stack (#1764)", () => {
  assert.ok(cases.lane_stack.length > 0);
  for (const c of cases.lane_stack) close(plain(context.markStackNames(c.names, c.top, c.bottom)), c.at, c.case);
});
