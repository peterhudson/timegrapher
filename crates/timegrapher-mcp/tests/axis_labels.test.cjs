// The view's axis rule (axisLabel) against core chart_scale::axis_number,
// by the fixture core's own test reads (#815).
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");

const html = fs.readFileSync(process.env.TIMEGRAPHER_TEST_VIEW_HTML ||
  path.join(__dirname, "../src/view.html"), "utf8");
const cases = JSON.parse(fs.readFileSync(
  path.join(__dirname, "../../timegrapher-core/tests/fixtures/axis_numbers.json"), "utf8"));
const source = html.match(/^function axisLabel\([^\n]*\{[\s\S]*?^\}/m);
if (!source) throw new Error("Missing renderer function: axisLabel");
// The Steadiness view's labels (stepText) follow the same rule.
const step = html.match(/^function stepText\([^\n]+\}\s*$/m);
if (!step) throw new Error("Missing renderer function: stepText");
const context = vm.createContext({});
vm.runInContext(source[0] + "\n" + step[0], context);

test("the fixture has cases", () => assert.ok(cases.length > 0));
for (const c of cases) {
  test(`${c.v} on a step of ${c.step} reads ${c.text}, as core writes it`, () => {
    assert.equal(context.axisLabel(c.v, c.step), c.text);
    assert.equal(context.stepText(c.v, c.step), c.text, "the Steadiness view's labels");
  });
}

test("the unit follows the number", () => assert.equal(context.axisLabel(-0.04, 0.1, " ms"), "0.0 ms"));
