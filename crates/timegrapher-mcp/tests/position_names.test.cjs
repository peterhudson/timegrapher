// Position names come from core: the server fills the table in, keyed by
// both codes, and the page writes no name of its own.
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");
const html = fs.readFileSync(process.env.TIMEGRAPHER_TEST_VIEW_HTML || path.join(__dirname, "../src/view.html"), "utf8");
function definition(name) {
  const match = html.match(new RegExp(`^function ${name}\\([^\\n]+\\}\\s*$`, "m"));
  if (!match) throw new Error(`Missing ${name}`);
  return match[0];
}

test("the server fills the names in and the page keeps no table of its own", () => {
  assert.ok(html.includes("const POSITIONS = /*POSITIONS*/null;"));
  assert.ok(!html.includes("COMMON"));
  assert.ok(!/["']Dial Up["']/.test(html));
});

test("a position is named from the server's table, whichever code it carries", () => {
  // The shape the server writes: each code to [common code, name].
  const POSITIONS = { CH: ["DU", "Name A"], DU: ["DU", "Name A"] };
  const context = vm.createContext({ POSITIONS });
  vm.runInContext(definition("positionName"), context);
  assert.equal(context.positionName("CH"), "Name A");
  assert.equal(context.positionName("DU"), "Name A");
  assert.equal(context.positionName("XY"), "XY");
  assert.equal(context.positionName(undefined), "");
});
