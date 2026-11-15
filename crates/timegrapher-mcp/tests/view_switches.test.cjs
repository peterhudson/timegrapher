// A switch in a view's toolbar changes only that view (Hugh, 11-05), as
// the app's panes do: each Steadiness view keeps its own Wheel Periods,
// Log Scale, Timing and Info.
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");

const html = fs.readFileSync(process.env.TIMEGRAPHER_TEST_VIEW_HTML ||
  path.join(__dirname, "../src/view.html"), "utf8");
const pick = (re, what) => {
  const m = html.match(re);
  if (!m) throw new Error(`Missing ${what}`);
  return m[0];
};
const context = vm.createContext({ steadyView: "periods", fftTiming: false, mapsLog: false, showWheels: true, showMesh: true, showInfo: false });
vm.runInContext([
  pick(/^const SWITCH_DEFAULTS = [^\n]*;/m, "SWITCH_DEFAULTS"),
  pick(/^const viewSwitches = \{\};/m, "viewSwitches"),
  pick(/^function switchView\([^\n]*\{[\s\S]*?^\}/m, "switchView"),
].join("\n"), context);
const now = () => JSON.parse(vm.runInContext("JSON.stringify({ fftTiming, mapsLog, showWheels, showMesh, showInfo, steadyView })", context));

test("a view's switches stay with it, and a view not yet shown starts from the defaults", () => {
  vm.runInContext("showWheels = false; showMesh = false; showInfo = true;", context);
  vm.runInContext("switchView('stft')", context);
  assert.deepEqual(now(), { fftTiming: false, mapsLog: false, showWheels: true, showMesh: true, showInfo: false, steadyView: "stft" });
  vm.runInContext("mapsLog = true;", context);
  vm.runInContext("switchView('wavelet')", context);
  assert.equal(now().mapsLog, false, "the STFT's Log Scale is not the Wavelet's");
  vm.runInContext("switchView('periods')", context);
  assert.deepEqual(now(), { fftTiming: false, mapsLog: false, showWheels: false, showMesh: false, showInfo: true, steadyView: "periods" });
  vm.runInContext("switchView('stft')", context);
  assert.equal(now().mapsLog, true);
});
