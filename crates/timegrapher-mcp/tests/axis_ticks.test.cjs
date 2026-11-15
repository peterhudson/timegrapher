// The view's value-tick rule (valueTick) against core chart_scale::axis_tick,
// by the fixture core's own test reads (#1779): bare, a rate's signed, a
// minus a true one, one that rounds to zero bare.
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");

const html = fs.readFileSync(process.env.TIMEGRAPHER_TEST_VIEW_HTML ||
  path.join(__dirname, "../src/view.html"), "utf8");
const cases = JSON.parse(fs.readFileSync(
  path.join(__dirname, "../../timegrapher-core/tests/fixtures/axis_ticks.json"), "utf8"));
function definition(name) {
  const match = html.match(new RegExp(`^function ${name}\\([^\\n]+\\}\\s*$`, "m")) || html.match(new RegExp(`^function ${name}\\([^\\n]*\\{[\\s\\S]*?^\\}`, "m"));
  if (!match) throw new Error(`Missing ${name}`);
  return match[0];
}
const context = vm.createContext({ MINUS: "−" });
vm.runInContext(["fixedDigits", "axisLabel", "valueTick"].map(definition).join("\n"), context);

test("the fixture has cases", () => assert.ok(cases.length > 5));
for (const c of cases) {
  test(`${c.signed ? "a rate's" : "a"} tick at ${c.v} on a step of ${c.step} reads ${c.text}, as core writes it`, () => {
    const series = { series: c.signed ? "rate" : "amplitude", unit: "x" };
    assert.equal(context.valueTick(series, c.v, c.step, false), c.text);
  });
}
