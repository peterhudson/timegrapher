// Execute the actual renderer and formatting functions, without a browser,
// acoustic processing, microphone, network, or third-party JavaScript packages.
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");

const html = fs.readFileSync(process.env.TIMEGRAPHER_TEST_VIEW_HTML ||
  path.join(__dirname, "../src/view.html"), "utf8");
const cases = JSON.parse(fs.readFileSync(path.join(__dirname, "fixtures/analysis_rates.json"), "utf8"));
function definition(name, optional = false) {
  const single = html.match(new RegExp(`^function ${name}\\([^\\n]+\\}\\s*$`, "m"));
  const multi = html.match(new RegExp(`^function ${name}\\([^\\n]*\\{[\\s\\S]*?^\\}`, "m"));
  const source = single || multi;
  if (!source && !optional) throw new Error(`Missing renderer function: ${name}`);
  return source ? source[0] : "";
}
// The no-beat heading the server puts in the page: core's
// measure::NO_WATCH_TITLE, read from core so the test holds the view to it.
const NO_BEAT_TITLE = fs.readFileSync(path.join(__dirname, "../../timegrapher-core/src/measure.rs"), "utf8")
  .match(/pub const NO_WATCH_TITLE: &str = "([^"]+)";/)[1];
const context = vm.createContext({
  NO_BEAT_TITLE,
  fileName: () => "test.wav", timeLabel: String, positionName: (p) => `name of ${p}`,
  stripCards: () => "", limitsCard: (l) => (l && l.length ? "<limits>" : ""),
  profileCard: (p, title, dropBeatError) => `<profile drop="${dropBeatError}">`,
});
vm.runInContext(["esc", "num", "signed", "abs", "tile", "rateBasis", "recordingView", "readingTiles", "recordingCards", "takeTiles"]
  .map(n => definition(n)).concat(definition("analyzeRate", true)).join("\n"), context);

function render(doc) { return context.recordingView(doc, null, null); }
function rateValue(markup) {
  return Number(markup.match(/Rate<\/div>\s*<div class="value">([^<]+)/)[1]);
}
for (const c of cases) {
  test(`rate and basis: ${c.name}`, () => {
    const doc = structuredClone(c.doc);
    const before = structuredClone(doc);
    const markup = render(doc);
    assert.equal(rateValue(markup), c.expected_rate);
    assert.ok(markup.includes(context.esc(c.expected_basis)), markup);
    assert.deepEqual(doc, before, "rendering must preserve raw and alternative readings");
  });
}

for (const invalid of [null, "7", Infinity, NaN]) {
  test(`invalid authoritative rate falls back to the card's clock, not a rate the CLI didn't choose: ${String(invalid)}`, () => {
    const doc = { overall: { rate_s_per_day: 6 }, rate_corrected_s_per_day: 4,
      clock: { rate_s_per_day: invalid } };
    assert.equal(rateValue(render(doc)), 6);
    doc.rate_corrected_s_per_day = invalid;
    assert.equal(rateValue(render(doc)), 6);
  });
}

test("no-watch result never draws a rate even if an inconsistent document contains one", () => {
  const markup = render({ ...cases.find(c => c.name === "stored-before-USB").doc, watch_heard: false });
  assert.ok(markup.includes(`<b>${NO_BEAT_TITLE}</b>`));
  assert.ok(!markup.includes('class="tile"'));
});

test("corrected headline identifies raw percentile basis and escapes the source", () => {
  const doc = structuredClone(cases.find(c => c.name === "stored-before-USB").doc);
  doc.clock.from = '<img src=x onerror="bad">';
  doc.rate_p05 = 5;
  doc.rate_p95 = 8;
  const markup = render(doc);
  assert.equal(rateValue(markup), 7);
  assert.ok(markup.includes("Card-clock 90% of windows +5.0 to +8.0"));
  assert.ok(markup.includes("&lt;img"));
  assert.ok(!markup.includes("<img"));
});

// A file its take lists: the whole file read alone says +9.0 s/d, the take's
// settled stretch +5.0, as `session` reads it.
function listed(readings) {
  return { overall: { rate_s_per_day: 9, beat_error_ms: 0.9 }, amplitude_deg: 250, beat_error_unlock_ms: 0.8,
    beats_found: 1000, watch_heard: true, take: { session_file: "session.toml", readings } };
}
const reading = (position, rate, extra = {}) => ({ position, label: ".", shape: { from_s: 20 },
  measurement: { rate_s_per_day: rate, amplitude_deg: 280, beat_error_ms: 0.4, beat_error_unlock_ms: 0.3,
    beats: 800, start_s: 20, end_s: 120, watch_heard: true, calibrated: true, confidence: { rate: { se: 0.2 } } }, ...extra });

test("a file its take lists shows the take's reading, not the whole file's", () => {
  const markup = render(listed([reading("DU", 5)]));
  assert.equal(rateValue(markup), 5);
  assert.ok(markup.includes("name of DU · 20 to 120 · as the take reads it"), markup);
  assert.ok(markup.includes(">280°<") && !markup.includes(">250°<"), markup);
  assert.ok(markup.includes(">0.30<") && markup.includes(">800<"), markup);
  assert.ok(markup.includes('<profile drop="0.4">'), "the profile takes the reading's drop beat error");
  // The take read it on a measured clock, so its rate says so.
  assert.ok(markup.includes("corrected for the sound card&#39;s clock"), markup);
  const raw = listed([reading("DU", 5)]);
  raw.take.readings[0].measurement.calibrated = false;
  assert.ok(render(raw).includes("on the sound card&#39;s clock"));
});

test("each of a take's readings of the file gets its own tiles", () => {
  const markup = render(listed([reading("DU", 5, { label: ". (0–90 s)" }), reading("DD", -3, { label: ". (90–180 s)" })]));
  const rates = [...markup.matchAll(/Rate<\/div>\s*<div class="value">([^<]+)/g)].map((m) => Number(m[1]));
  assert.deepEqual(rates, [5, -3]);
  assert.ok(markup.includes("name of DD · . (90–180 s)"), markup);
});

test("a take reading with no regular beat draws no figures", () => {
  const markup = render(listed([reading("DU", 5, { measurement: { watch_heard: false } })]));
  assert.ok(markup.includes(`<b>${NO_BEAT_TITLE}</b>`) && !markup.includes('class="tile"'), markup);
});

test("a take that couldn't be read leaves the whole file's figures", () => {
  const doc = listed(undefined);
  doc.take = { session_file: "session.toml", error: "unreadable" };
  assert.equal(rateValue(render(doc)), 9);
});

// The ± is core's text, handed over by the server (#806): the view no longer
// rounds 0.4° up to "± 1°", and draws no ± of its own from a bare se.
test("the tiles show the server's ± text, and none without it", () => {
  const doc = listed(undefined);
  doc.take = undefined;
  doc.amplitude_se_deg = 0.4;
  doc.confidence = { rate: { se: 0.2 } };
  const bare = render(doc);
  assert.ok(!bare.includes("±"), bare);
  const markup = context.recordingView(doc, null, null, null, { rate: "0.2", amplitude: "0.4" });
  assert.ok(markup.includes("± 0.4°") && markup.includes("± 0.2 seconds per day"), markup);
  const take = context.recordingView(listed([reading("DU", 5), reading("DD", -3)]), null, null, null,
    { readings: [{ rate: "0.2", amplitude: null }, { rate: "1.5", amplitude: "3" }] });
  assert.ok(take.includes("± 0.2 seconds per day") && take.includes("± 1.5 seconds per day") && take.includes("± 3°"), take);
});

// Whether clipping is worth saying is core's call (Clipping::noted, the
// `clipped` caveat the server turns into limits), not a threshold of the
// view's own (#808).
test("clipping is said only in the server's limits", () => {
  const doc = { clipping: { beats: 30, beat_fraction: 0.3 } };
  assert.ok(!context.recordingCards(doc, null, null).includes("Clipping"));
  const said = context.recordingCards(doc, null, ["30% of the beats have a clipped sample."]);
  assert.ok(said.includes("<limits>"), said);
});
