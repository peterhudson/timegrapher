// The live view while record_position records (#1887), from the full
// embedded script with a host and timers the test drives: a library
// `watch` with no `folder` follows the take too, a poll given up on leaves
// nothing pending, and each call starts its live state afresh.
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");

const html = fs.readFileSync(process.env.TIMEGRAPHER_TEST_VIEW_HTML ||
  path.join(__dirname, "../src/view.html"), "utf8");
const script = [...html.matchAll(/<script>([\s\S]*?)<\/script>/g)][0][1];

function host() {
  const messages = [];
  const listeners = new Map();
  const timers = [];
  const root = { innerHTML: "", clientWidth: 640, querySelectorAll: () => [] };
  const parent = { postMessage(m) { messages.push(structuredClone(m)); } };
  const window = { parent, matchMedia: () => ({ matches: false }),
    addEventListener(name, cb) { listeners.set(name, cb); } };
  const context = vm.createContext({ window,
    document: { documentElement: { dataset: {}, scrollWidth: 640, scrollHeight: 200 }, body: {},
      getElementById: () => root },
    ResizeObserver: class { observe() {} },
    setTimeout(fn, ms) { timers.push({ fn, ms, done: false }); return timers.length; },
    clearTimeout(id) { if (timers[id - 1]) timers[id - 1].done = true; },
  });
  vm.runInContext(script, context, { filename: "embedded-view.js" });
  const input = (args) => listeners.get("message")({ source: parent, data: {
    jsonrpc: "2.0", method: "ui/notifications/tool-input", params: { arguments: args } } });
  const polls = () => messages.filter((m) => m.method === "tools/call" && m.params.name === "recording_status");
  const fire = (ms) => timers.filter((t) => !t.done && t.ms === ms).forEach((t) => { t.done = true; t.fn(); });
  return { messages, timers, input, polls, fire,
    pending: () => vm.runInContext("pending.size", context),
    live: () => vm.runInContext("live && { position: live.position }", context) };
}

test("a recording under a library watch, with no folder, has a live view", () => {
  const h = host();
  h.input({ watch: "Pepsi", position: "DU", seconds: 60 });
  assert.equal(h.polls().length, 1, "the view asks for the take as it is heard");
  assert.equal(h.live().position, "DU");
});

test("a poll given up on leaves nothing pending", async () => {
  const h = host();
  const before = h.pending();
  h.input({ folder: "/tmp/test", position: "DU" });
  assert.equal(h.pending(), before + 1);
  h.fire(5000);
  await new Promise((r) => setImmediate(r));
  assert.equal(h.pending(), before, "the timed-out request still waits in pending");
});

test("each call starts its live state afresh", () => {
  const h = host();
  h.input({ folder: "/tmp/test", position: "DU" });
  const first = h.timers.filter((t) => !t.done).length;
  h.input({ watch: "Pepsi", position: "CU" });
  assert.equal(h.live().position, "CU");
  h.input({ file: "/tmp/test/DU.wav" });
  assert.equal(h.live(), null, "a call that records nothing keeps no live state");
  assert.ok(first > 0);
});
