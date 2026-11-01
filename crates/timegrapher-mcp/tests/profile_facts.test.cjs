// The Tick and Tock lines are core's facts (profile::facts_of, the app's
// rows) as the server sends them; the view words none of its own (#510).
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");

const html = fs.readFileSync(process.env.TIMEGRAPHER_TEST_VIEW_HTML ||
  path.join(__dirname, "../src/view.html"), "utf8");
const source = html.match(/^function profileSummary\([^\n]*\{[\s\S]*?^\}/m);
if (!source) throw new Error("Missing renderer function: profileSummary");
const context = vm.createContext({});
vm.runInContext(source[0], context);

const facts = {
  names: ["Beats", "Unlock", "Drop", "Unlock to Drop", "Amplitude"],
  a: ["120", "−0.27 ms", "+1.20 ms", "1.47 ms", "281°"],
  b: ["119", "–", "+1.00 ms", "–", "out of range"],
};

test("each side's line is the server's facts with their names", () => {
  assert.equal(context.profileSummary(facts, 0), "Beats 120 · Unlock −0.27 ms · Drop +1.20 ms · Unlock to Drop 1.47 ms · Amplitude 281°");
  assert.equal(context.profileSummary(facts, 1), "Beats 119 · Unlock – · Drop +1.00 ms · Unlock to Drop – · Amplitude out of range");
});

test("without the server's facts the line is empty, not the view's own", () => {
  assert.equal(context.profileSummary(undefined, 0), "");
  assert.equal(context.profileSummary({ names: facts.names }, 1), "");
});
