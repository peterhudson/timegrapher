// Execute the full embedded script with deterministic browser/host doubles.
// This verifies bridge messages, not browser layout or real host mounting.
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");
const { setImmediate: flush } = require("node:timers/promises");

const html = fs.readFileSync(process.env.TIMEGRAPHER_TEST_VIEW_HTML ||
  path.join(__dirname, "../src/view.html"), "utf8");
const scripts = [...html.matchAll(/<script>([\s\S]*?)<\/script>/g)];
assert.equal(scripts.length, 1, "execute the entire embedded script");

function host({ disconnected = false, standalone = false, failMethod, synchronous = false } = {}) {
  const messages = [];
  const listeners = new Map();
  const root = { innerHTML: "Waiting for the reading…", clientWidth: 640,
    querySelectorAll: () => [] };
  const documentElement = { dataset: {}, scrollWidth: 640, scrollHeight: 200 };
  let resize;
  const parent = { postMessage(message) {
    if (disconnected || message.method === failMethod) throw new Error("host transport disconnected");
    messages.push(structuredClone(message));
    if (synchronous && message.method === "ui/initialize") {
      listeners.get("message")({ source: window.parent, data: {
        jsonrpc: "2.0", id: message.id, result: {
          protocolVersion: "2026-01-26", hostInfo: { name: "test-host", version: "1" },
          hostCapabilities: {}, hostContext: { theme: "light" },
        },
      } });
    }
  } };
  const window = { parent, matchMedia: () => ({ matches: false }),
    addEventListener(name, callback) { listeners.set(name, callback); } };
  if (standalone) {
    window.parent = window;
    window.postMessage = parent.postMessage;
  }
  const context = vm.createContext({ window,
    document: { documentElement, body: {}, getElementById: () => root },
    ResizeObserver: class { constructor(callback) { resize = callback; } observe() {} },
    // Timers are intentionally inert: no real polling or waiting in these tests.
    setTimeout() { throw new Error("unexpected timer during handshake"); },
    clearTimeout() {},
  });
  vm.runInContext(scripts[0][1], context, { filename: "embedded-view.js" });
  const init = messages.find(m => m.method === "ui/initialize");
  return { messages, root, documentElement, init, resize: () => resize(),
    pending: () => vm.runInContext("pending.size", context),
    ready: () => vm.runInContext("initialized", context),
    receive(message, source = window.parent) {
      listeners.get("message")({ source, data: structuredClone(message) });
    },
  };
}
function result(h, overrides = {}) {
  return { jsonrpc: "2.0", id: h.init.id, result: {
    protocolVersion: "2026-01-26", hostInfo: { name: "test-host", version: "1" },
    hostCapabilities: {}, hostContext: { theme: "dark" }, ...overrides,
  } };
}
function methods(h) { return h.messages.map(m => m.method); }

test("initialize uses appInfo and preserves the supported version/capabilities", () => {
  const h = host();
  assert.deepEqual(h.init.params, {
    protocolVersion: "2026-01-26", appInfo: { name: "timegrapher-charts", version: "1" },
    appCapabilities: { availableDisplayModes: ["inline", "fullscreen"] },
  });
  assert.equal(h.pending(), 1);
});

test("successful response applies theme and sends initialized once before sizing", async () => {
  const h = host();
  h.resize();
  assert.deepEqual(methods(h), ["ui/initialize"]);
  h.receive(result(h));
  await flush();
  assert.equal(h.documentElement.dataset.theme, "dark");
  assert.deepEqual(methods(h), ["ui/initialize", "ui/notifications/initialized",
    "ui/notifications/size-changed"]);
  assert.deepEqual(h.messages[2].params, { width: 640, height: 200 });
  assert.equal(h.pending(), 0);
  h.receive(result(h));
  h.resize();
  await flush();
  assert.equal(h.messages.length, 3, "duplicate response/unchanged size are inert");
});

test("rejected initialization never sends initialized or sizing", async () => {
  const h = host();
  h.receive({ jsonrpc: "2.0", id: h.init.id,
    error: { code: -32602, message: "invalid initialize" } });
  await flush();
  h.resize();
  h.receive(result(h));
  await flush();
  assert.deepEqual(methods(h), ["ui/initialize"]);
  assert.equal(h.pending(), 0);
  assert.match(h.root.innerHTML, /connection unavailable/i);
});

test("delayed host and unrelated messages cannot signal successful initialization", async () => {
  const h = host();
  h.resize();
  h.receive(result(h), {});
  h.receive({ ...result(h), id: h.init.id + 1 });
  h.receive({ ...result(h), jsonrpc: "1.0" });
  await flush();
  assert.deepEqual(methods(h), ["ui/initialize"]);
  assert.equal(h.pending(), 1);
  assert.equal(h.documentElement.dataset.theme, undefined);
  assert.match(h.root.innerHTML, /Waiting/);
  h.receive(result(h));
  await flush();
  assert.equal(methods(h).filter(m => m === "ui/notifications/initialized").length, 1);
});

test("no host response remains pending without a false ready notification", async () => {
  const h = host({ standalone: true });
  // A top-level page receives its own postMessage; it is not a host response.
  h.receive(h.init);
  h.resize();
  await flush();
  assert.deepEqual(methods(h), ["ui/initialize"]);
  assert.equal(h.pending(), 1);
  assert.match(h.root.innerHTML, /Waiting/);
});

test("a throwing/disconnected transport is handled and releases its pending request", async () => {
  const h = host({ disconnected: true });
  await flush();
  h.resize();
  assert.deepEqual(methods(h), []);
  assert.equal(h.pending(), 0);
  assert.match(h.root.innerHTML, /connection unavailable/i);
});


test("a failed initialized send is handled without readiness or sizing", async () => {
  const h = host({ failMethod: "ui/notifications/initialized" });
  h.receive(result(h));
  await flush();
  h.resize();
  assert.deepEqual(methods(h), ["ui/initialize"]);
  assert.equal(h.pending(), 0);
  assert.equal(h.ready(), false);
  assert.match(h.root.innerHTML, /connection unavailable/i);
});

test("a synchronous host response cannot race pending request registration", async () => {
  // Browser postMessage is asynchronous; this also protects transport doubles
  // and future transports that can reply immediately during send.
  const h = host({ synchronous: true });
  await flush();
  assert.equal(h.pending(), 0);
  assert.equal(h.ready(), true);
  assert.equal(h.documentElement.dataset.theme, "light");
  assert.deepEqual(methods(h), ["ui/initialize", "ui/notifications/initialized",
    "ui/notifications/size-changed"]);
});
