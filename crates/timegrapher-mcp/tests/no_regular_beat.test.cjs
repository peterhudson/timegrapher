// The no-regular-beat words take the share of a beat core gives, not their own.
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
const context = vm.createContext({});
vm.runInContext(["num", "noRegularBeat"].map(definition).join("\n"), context);

test("the share of a beat is core's timing_scatter_fraction", () => {
  // 8,600 µs at 28,800 bph would be 6.9% of a beat; the document says 5.2%.
  assert.equal(
    context.noRegularBeat(8600, 0.052),
    "No regular beat found (timing scatter 8.6 ms, 5.2% of a beat). There are no rate, amplitude or beat error readings."
  );
  assert.equal(
    context.noRegularBeat(8600, null),
    "No regular beat found. There are no rate, amplitude or beat error readings."
  );
});
