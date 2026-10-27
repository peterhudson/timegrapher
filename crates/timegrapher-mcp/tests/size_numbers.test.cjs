// The view's size rule (mapNumber) against core timefreq::size_number, by
// the fixture core's own test reads.
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");

const html = fs.readFileSync(process.env.TIMEGRAPHER_TEST_VIEW_HTML ||
  path.join(__dirname, "../src/view.html"), "utf8");
const cases = JSON.parse(fs.readFileSync(
  path.join(__dirname, "../../timegrapher-core/tests/fixtures/size_numbers.json"), "utf8"));
const source = html.match(/^function mapNumber\([^\n]*\{[\s\S]*?^\}/m);
if (!source) throw new Error("Missing renderer function: mapNumber");
const context = vm.createContext({});
vm.runInContext(source[0], context);

test("the fixture has cases", () => assert.ok(cases.length > 0));
for (const c of cases) {
  test(`size ${c.v} reads ${c.text}, as core writes it`, () => {
    assert.equal(context.mapNumber(c.v), c.text);
  });
}
