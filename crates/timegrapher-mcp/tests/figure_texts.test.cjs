// The view's num and signed against core figure_text's fixed_text and
// signed_text as a screen writes them (true_minus), by the fixture core's
// own test reads (#1431).
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");

const html = fs.readFileSync(process.env.TIMEGRAPHER_TEST_VIEW_HTML ||
  path.join(__dirname, "../src/view.html"), "utf8");
const cases = JSON.parse(fs.readFileSync(
  path.join(__dirname, "../../timegrapher-core/tests/fixtures/figure_texts.json"), "utf8"));
const definition = (name) => {
  const found = html.match(new RegExp(`^function ${name}\\([^\\n]*\\{[\\s\\S]*?^\\}`, "m"));
  if (!found) throw new Error(`Missing renderer function: ${name}`);
  return found[0];
};
// Core's minus, as the server puts it in the page.
const { MINUS } = require("./core_digits.cjs");
const context = vm.createContext({ MINUS });
vm.runInContext(["fixedDigits", "num", "signed", "hoverFigure"].map(definition).join("\n"), context);

test("the fixture has cases", () => assert.ok(cases.length > 0));
for (const c of cases) {
  test(`${c.v} to ${c.digits} places reads ${c.fixed} and ${c.signed}, as core writes them`, () => {
    assert.equal(context.num(c.v, c.digits), c.fixed);
    assert.equal(context.signed(c.v, c.digits), c.signed);
  });
}

test("a chart's hover writes its figure with core's true minus", () => {
  assert.equal(context.hoverFigure(-3.25, { decimals: 1, unit: " s/d" }), `${MINUS}3.2 s/d`);
  assert.equal(context.hoverFigure(-0.04, { decimals: 1 }), "0.0");
  assert.equal(context.hoverFigure(272.1, { unit: "°" }), "272°");
});
