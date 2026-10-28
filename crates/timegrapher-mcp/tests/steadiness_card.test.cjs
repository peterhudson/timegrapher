// Execute the view's own Steadiness card: each plot's title line carries
// core's summary, the full text sits in the Info panel (closed until asked
// for), and the Readings view keys its dots.
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");
const html = fs.readFileSync(process.env.TIMEGRAPHER_TEST_VIEW_HTML || path.join(__dirname, "../src/view.html"), "utf8");
function definition(name) {
  const match = html.match(new RegExp(`^function ${name}\\([^\\n]*\\{[\\s\\S]*?^\\}`, "m"));
  if (!match) throw new Error(`Missing renderer ${name}`);
  return match[0];
}
const constant = (name) => html.match(new RegExp(`^const ${name} = [\\s\\S]*?;\\n(?=\\S|\\n)`, "m"))[0];
const context = vm.createContext({ fmtTime: (t) => `${t / 60}:00`, secs: (t) => `${t} s`, spectrumOf: () => null, steadyView: "readings",
  periodsLog: false, fftTiming: false, mapsLog: false, showWheels: true, showInfo: false });
vm.runInContext([constant("SERIES"), constant("STEADY_VIEWS"), constant("INFO_HINT"), constant("WHEELS_HINT"),
  ...["esc", "num", "pText", "steadyStats", "steadySwitch", "dotKey", "ordinal", "steadinessCard"].map(definition)].join("\n"), context);

const rate = { series: "rate", unit: "s/d", readings: 179, step_s: 10, median: 1.5, autocorrelation: { p_value: 0.5 },
  headline: "The rate repeats every 46 s, 4.0 s/d peak to peak.", summary: "repeats every 46 s, 4.0 s/d peak to peak" };
const doc = { duration_s: 1800, series: [rate] };

test("each plot's title line carries the series and core's summary; the full text waits in Info", () => {
  context.steadyPlot = () => "<svg/>";
  context.showInfo = false;
  const shut = context.steadinessCard(doc);
  assert.ok(shut.includes('title="Rate · repeats every 46 s, 4.0 s/d peak to peak">Rate<span class="summary"> · repeats every 46 s, 4.0 s/d peak to peak</span>'), shut);
  assert.ok(!shut.includes(rate.headline) && !shut.includes("Tested on"), "closed: no headline, no statistics");
  assert.ok(shut.includes('data-switch="info" aria-pressed="false"'));
  context.showInfo = true;
  const open = context.steadinessCard(doc);
  assert.ok(open.includes(rate.headline) && open.includes("Tested on 179 10 s readings") && open.includes("Tested over 30:00 of beats"), open);
  context.showInfo = false;
});

test("the Readings view's key says what a dot is and how many are drawn", () => {
  assert.equal(context.dotKey({ step_s: 10, stride: 1 }, 179), "10 s readings (179)");
  assert.equal(context.dotKey({ step_s: 2, stride: 1 }, 912), "2 s windows (912)");
  assert.equal(context.dotKey({ step_s: 2, stride: 3 }, 3000), "2 s windows, every 3rd (3,000)");
  assert.equal(context.dotKey({ step_s: 20, stride: 1 }, 90), "20 s readings (90)");
  assert.equal(context.dotKey({ step_s: 5, stride: 1 }, 40), "5 s windows (40)");
  assert.equal(context.ordinal(12), "12th");
});

test("the Long Recording view says what limits its readings, as the analyze and session views do", () => {
  const c = vm.createContext({ steadinessCard: () => "<steady/>", distributionCards: () => "<dist/>", fileName: () => "a.wav", timeLabel: String });
  vm.runInContext(["esc", "limitsCard", "seriesView"].map(definition).join("\n"), c);
  const limits = [{ where: null, text: "The beat error's sign differs either side of a cut in the stretch, so it is given as a size." }];
  const html = c.seriesView({ duration_s: 60 }, null, limits);
  assert.ok(html.includes("<steady/><div class=\"card\"><h2>What Limits These Readings</h2>") && html.includes("given as a size.</li>"), html);
  assert.ok(!c.seriesView({ duration_s: 60 }, null, []).includes("What Limits"));
});
