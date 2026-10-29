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
const context = vm.createContext({
  fileName: () => "test.wav", timeLabel: String,
  stripCards: () => "", profileCard: () => "", limitsCard: () => "",
  noRegularBeat: () => "No regular beat found.",
});
vm.runInContext(["esc", "num", "signed", "abs", "tile", "recordingView"]
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
  assert.ok(markup.includes("No Regular Beat Found"));
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
