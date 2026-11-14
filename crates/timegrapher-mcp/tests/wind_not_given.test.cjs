// A session reading whose wind was not given is said so in core's words and
// listed after the given ones, never shown as 0 h, full wind (#1665): its
// Positions Test pane is headed in core's words, after the full-wind one,
// and a session whose one state is wind not given is headed so too.
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
// The words core heads the state with: reading_text::WIND_NOT_GIVEN, capitalised
// (wind_state_title), as the server's position_lines[].name carries them.
const WIND_NOT_GIVEN = fs.readFileSync(path.join(__dirname, "../../timegrapher-core/src/reading_text.rs"), "utf8")
  .match(/pub const WIND_NOT_GIVEN: &str = "([^"]+)";/)[1];
const TITLE = WIND_NOT_GIVEN[0].toUpperCase() + WIND_NOT_GIVEN.slice(1);
const context = vm.createContext({ NO_BEAT_TITLE: "", POSITIONS: null, limitsCard: () => "", tile: () => "", signed: String, num: String,
  testPane: (title) => `<h2>${title}</h2>`, positionsPane: (sl, readings, id, titled) => (titled ? `<h2>${sl.name}</h2>` : "<h2></h2>") });
vm.runInContext(["esc", "sessionView"].map(definition).join("\n"), context);

const lines = (wind_h, name) => ({ wind_h, name });

test("a reading with no wind given has its own pane, after the full-wind one", () => {
  const markup = context.sessionView({ readings: [], report: {
    states: [{ wind_h: 0 }, { wind_h: null }],
    position_lines: [lines(0, "Full wind"), lines(null, TITLE)],
  } }, null);
  assert.deepEqual([...markup.matchAll(/<h2>([^<]*)<\/h2>/g)].map((m) => m[1]), ["Full wind", TITLE]);
});

test("a session whose one state is wind not given still says so", () => {
  const markup = context.sessionView({ readings: [], report: {
    states: [{ wind_h: null }], position_lines: [lines(null, TITLE)],
  } }, null);
  assert.deepEqual([...markup.matchAll(/<h2>([^<]*)<\/h2>/g)].map((m) => m[1]), [TITLE]);
});

test("a session whose one state is full wind needs no heading", () => {
  const markup = context.sessionView({ readings: [], report: {
    states: [{ wind_h: 0 }], position_lines: [lines(0, "Full wind")],
  } }, null);
  assert.deepEqual([...markup.matchAll(/<h2>([^<]*)<\/h2>/g)].map((m) => m[1]), [""]);
});
