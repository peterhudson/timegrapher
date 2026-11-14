// A session reading whose wind was not given is said so in core's words and
// listed after the given ones, never shown as 0 h, full wind (#1665).
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
// The words the server puts in the page: core's reading_text::WIND_NOT_GIVEN.
const WIND_NOT_GIVEN = fs.readFileSync(path.join(__dirname, "../../timegrapher-core/src/reading_text.rs"), "utf8")
  .match(/pub const WIND_NOT_GIVEN: &str = "([^"]+)";/)[1];
const context = vm.createContext({ WIND_NOT_GIVEN, NO_BEAT_TITLE: "",
  ORDER: [], POSITIONS: null, tile: () => "", signed: String, num: String, abs: Math.abs,
  limitsCard: () => "", bars: () => "", chosenBeatError: () => ({}) });
vm.runInContext(["esc", "sessionView"].map(definition).join("\n"), context);

test("a reading with no wind given says so, after the full-wind one", () => {
  const markup = context.sessionView({ readings: [
    { position: "DU", wind_h: null, measurement: {} },
    { position: "DU", wind_h: 0, measurement: {} },
  ] }, null);
  const cells = [...markup.matchAll(/<td>([^<]*)<\/td><\/tr>/g)].map((m) => m[1]);
  assert.deepEqual(cells, ["0", WIND_NOT_GIVEN], markup);
});
