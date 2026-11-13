// Execute the view's own Steadiness card: each plot's title line carries
// core's summary, the full text sits in the Info panel (closed until asked
// for), and the Readings view keys its dots.
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");
const { MINUS } = require("./core_digits.cjs");
const html = fs.readFileSync(process.env.TIMEGRAPHER_TEST_VIEW_HTML || path.join(__dirname, "../src/view.html"), "utf8");
function definition(name) {
  const match = html.match(new RegExp(`^function ${name}\\([^\\n]*\\{[\\s\\S]*?^\\}`, "m"));
  if (!match) throw new Error(`Missing renderer ${name}`);
  return match[0];
}
const constant = (name) => html.match(new RegExp(`^const ${name} = [\\s\\S]*?;\\n(?=\\S|\\n)`, "m"))[0];
const context = vm.createContext({ MINUS, fmtTime: (t) => `${t / 60}:00`, secs: (t) => `${t} s`, spectrumOf: () => null, steadyView: "readings",
  periodsLog: false, fftTiming: false, mapsLog: false, showWheels: true, showInfo: false });
vm.runInContext([constant("SERIES"), constant("STEADY_VIEWS"), constant("INFO_HINT"), constant("WHEELS_HINT"),
  constant("SWITCH_DEFAULTS"), constant("viewSwitches"),
  ...["esc", "fixedDigits", "num", "signed", "pText", "steadySwitch", "switchView", "steadinessCard"].map(definition)].join("\n"), context);

const rate = { series: "rate", unit: "s/d", unit_text: " s/d", readings: 179, step_s: 10, median: 1.5, autocorrelation: { p_value: 0.5 },
  headline: "The rate repeats every 46 s, 4.0 s/d peak to peak.", summary: "repeats every 46 s, 4.0 s/d peak to peak",
  screen_headline: "The rate repeats every 46 s, 4.0 s/d peak to peak.", screen_summary: "repeats every 46 s, 4.0 s/d peak to peak",
  screen_stats: "Tested on 179 10 s readings · median 1.5 s/d · Ljung–Box p 0.500" };
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

test("Info shows core's statistics line as sent: the trend, its note and a true minus are core's (#1470)", () => {
  context.steadyPlot = () => "<svg/>";
  const line = `Tested on 179 10 s readings · median 1.5 s/d · trend ${MINUS}1.50 s/d/h, not significant after Holm's correction (p 0.031 alone)`;
  context.showInfo = true;
  const open = context.steadinessCard({ duration_s: 1800, series: [{ ...rate, screen_stats: line }] });
  context.showInfo = false;
  assert.ok(open.includes(`<p class="stats">${line.replace(/'/g, "&#39;")}</p>`), open);
});

test("the Long Recording view says what limits its readings, as the analyze and session views do", () => {
  const c = vm.createContext({ MINUS, steadinessCard: () => "<steady/>", walkCard: () => "", distributionCards: () => "<dist/>", fileName: () => "a.wav", timeLabel: String });
  vm.runInContext(["esc", "limitsCard", "seriesView"].map(definition).join("\n"), c);
  const limits = [{ where: null, text: "The beat error's sign differs either side of a cut in the stretch, so it is given as a size." }];
  const html = c.seriesView({ duration_s: 60 }, null, limits);
  assert.ok(html.includes("<steady/><div class=\"card\"><h2>What Limits These Readings</h2>") && html.includes("given as a size.</li>"), html);
  assert.ok(!c.seriesView({ duration_s: 60 }, null, []).includes("What Limits"));
});

test("a series' own caveat shows once, on that series, in the server's words", () => {
  const c = vm.createContext({ MINUS, fmtTime: (t) => `${t / 60}:00`, secs: (t) => `${t} s`, spectrumOf: () => null, steadyView: "readings",
    periodsLog: false, fftTiming: false, mapsLog: false, showWheels: true, showInfo: false, steadyPlot: () => "<svg/>",
    distributionCards: () => "", walkCard: () => "", fileName: () => "a.wav", timeLabel: String });
  vm.runInContext([constant("SERIES"), constant("STEADY_VIEWS"), constant("INFO_HINT"), constant("WHEELS_HINT"),
    constant("SWITCH_DEFAULTS"), constant("viewSwitches"),
    ...["esc", "fixedDigits", "num", "signed", "pText", "steadySwitch", "switchView", "steadinessCard", "limitsCard", "seriesView"].map(definition)].join("\n"), c);
  // The sentence core's PeriodMayBeNoise gives; the view draws it as sent.
  const noise = "On a take this short with its 2 s windows this correlated (ρ 0.74 from one to the next, over 30 min), the period search names a cycle in noise alone about 1 time in 24, not 1 in 100: this cycle may be noise.";
  const series = [rate, { ...rate, series: "amplitude", unit: "deg", unit_text: "°", caveats: [{ code: "period_may_be_noise", rho: 0.74, span_s: 1800 }] },
    { ...rate, series: "beat_error", unit: "ms", unit_text: " ms" }];
  const take = [{ where: null, text: "The beat error is given as a size." }];
  const out = c.seriesView({ duration_s: 1800, series }, null, take, [[], [noise], []]);
  assert.equal(out.split(noise).length - 1, 1, "said once");
  const at = out.indexOf(noise);
  assert.ok(out.lastIndexOf('<p class="series-name"', at) === out.indexOf('<p class="series-name" title="Amplitude'), "under the Amplitude title");
  assert.ok(at < out.indexOf('<p class="series-name" title="Beat Error'), "before the next series");
  const limitsCard = out.slice(out.indexOf("What Limits These Readings"));
  assert.ok(!limitsCard.includes(noise), "not with the take's caveats");
  // The document the tool returned is left as it was.
  assert.equal(series[1].limits, undefined);
  // With no series caveats from the server, none is drawn.
  assert.ok(!c.seriesView({ duration_s: 1800, series }, null, take).includes("may be noise"));
});

test("the title line and the headline show core's screen copies: a true minus on figures, the words as sent (#1437)", () => {
  context.steadyPlot = () => "<svg/>";
  const shift = { ...rate,
    headline: "Rate shifts level 1 time: -212.6 s/d, then -5.0 s/d over 2-second stretches.",
    summary: "shifts level 1 time: -212.6 s/d, then -5.0 s/d",
    screen_headline: `Rate shifts level 1 time: ${MINUS}212.6 s/d, then ${MINUS}5.0 s/d over 2-second stretches.`,
    screen_summary: `shifts level 1 time: ${MINUS}212.6 s/d, then ${MINUS}5.0 s/d` };
  context.showInfo = true;
  const out = context.steadinessCard({ duration_s: 1800, series: [shift] });
  context.showInfo = false;
  assert.ok(out.includes(`<span class="summary"> · shifts level 1 time: ${MINUS}212.6 s/d, then ${MINUS}5.0 s/d</span>`), out);
  assert.ok(out.includes(`<p class="headline">Rate shifts level 1 time: ${MINUS}212.6 s/d`), out);
  assert.ok(out.includes("2-second stretches"), out);
  assert.ok(!out.includes("-212.6") && !out.includes("-5.0"), "no ASCII minus on a figure");
});
