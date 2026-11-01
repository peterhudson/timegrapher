// No regular beat is said in the server's words (core's), which it sends as
// `timegrapher/no_beat`; the view types none of its own.
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
// The no-beat heading the server puts in the page: core's
// measure::NO_WATCH_TITLE, read from core so the test holds the view to it.
const NO_BEAT_TITLE = fs.readFileSync(path.join(__dirname, "../../timegrapher-core/src/measure.rs"), "utf8")
  .match(/pub const NO_WATCH_TITLE: &str = "([^"]+)";/)[1];
const context = vm.createContext({
  NO_BEAT_TITLE,
  fileName: () => "take.flac", timeLabel: String, positionName: String,
  liveStrip: () => "", stripCards: () => "", trendStrip: () => "",
});
vm.runInContext(["esc", "num", "recordingView", "recordView"].map(definition).join("\n"), context);

const said = "No regular beat found (too few beats to time). There are no rate, amplitude or beat error readings.";

test("a recording with no regular beat says the server's sentence", () => {
  const markup = context.recordingView({ watch_heard: false, bph: 28800, lift_deg: 52, jitter_us: 8600 }, null, null, said);
  assert.ok(markup.includes(`<b>${NO_BEAT_TITLE}</b>${said}</div>`), markup);
});

test("a take's check with no regular beat says the server's sentence", () => {
  const markup = context.recordView({ position: "DU", seconds: 60, check: { watch_heard: false, bph: 28800, timing_scatter_ms: 8.6 } }, null, said);
  assert.ok(markup.includes(`<b>${NO_BEAT_TITLE}</b>${said}</div>`), markup);
});

test("the sentence is escaped", () => {
  const markup = context.recordingView({ watch_heard: false }, null, null, "<img src=x>");
  assert.ok(markup.includes("&lt;img") && !markup.includes("<img"), markup);
});
