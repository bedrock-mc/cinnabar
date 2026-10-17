import { test } from "node:test";
import assert from "node:assert/strict";
import { signInGestures, BrowserPreferences } from "./launcher.mjs";

/** Supplies listener ownership and trusted browser observations deterministically. */
function canvas() {
  const listeners = new Map();
  return {
    addEventListener: (name, callback) => listeners.set(name, callback),
    removeEventListener: name => listeners.delete(name),
    emit: (name, values = {}) => listeners.get(name)?.({ type: name, pointerId: 1, pointerType: "mouse", isTrusted: true, isPrimary: true, button: 0, clientX: 10, clientY: 20, preventDefault() {}, ...values }),
    listeners,
  };
}

test("sign-in uses current gesture coordinates and tears down every listener", () => {
  const target = canvas(), opened = [], reported = [];
  let hit = "open_sign_in_link", uri = "https://example.invalid/device", focused = hit;
  const stop = signInGestures(target, (x, y) => {
    assert.equal(x, 10); assert.equal(y, 20); return hit;
  }, () => focused, () => uri, message => reported.push(message), { open: (...args) => opened.push(args) });
  target.emit("pointerdown");
  assert.equal(opened.length, 1);
  assert.equal(opened[0][2], "noopener,noreferrer");
  hit = "cancel_sign_in";
  target.emit("pointerdown");
  target.emit("pointerdown", { isTrusted: false });
  target.emit("pointerdown", { button: 2 });
  assert.equal(opened.length, 1);
  target.emit("keydown", { key: "Enter" });
  assert.equal(opened.length, 2);
  target.emit("keydown", { key: "Enter", repeat: true });
  focused = "cancel_sign_in";
  target.emit("keydown", { key: "Enter" });
  assert.equal(opened.length, 2);
  hit = "open_sign_in_link";
  for (uri of ["javascript:alert(1)", "http://example.invalid", "https://name:secret@example.invalid", "bad"]) target.emit("pointerdown");
  assert.equal(reported.length, 4);
  assert.equal(opened.length, 2);
  stop(); stop();
  assert.equal(target.listeners.size, 0);
  target.emit("pointerdown");
  assert.equal(opened.length, 2);
});

test("preferences report quota/security errors and preserve unreadable state", () => {
  const reports = [], saved = [];
  const memory = new Map();
  const storage = { getItem: key => memory.get(key) ?? null, setItem: (key, value) => { memory.set(key, value); saved.push(value); } };
  const owner = new BrowserPreferences(json => JSON.stringify(JSON.parse(json)), 32, message => reports.push(message), () => storage);
  assert.equal(owner.load(), null);
  assert.equal(owner.save('{ "servers": [] }'), true);
  assert.equal(owner.load(), '{"servers":[]}');
  assert.equal(owner.save("x".repeat(33)), false);
  assert.equal(saved.length, 1);
  storage.setItem = () => { throw new Error("Quota exceeded"); };
  assert.equal(owner.save("{}"), false);
  memory.set(owner.key, "broken");
  assert.equal(owner.load(), null);
  storage.setItem = (key, value) => memory.set(key, value);
  assert.equal(owner.save("{}"), false);
  assert.equal(memory.get(owner.key), "broken");
  const blocked = new BrowserPreferences(json => json, 32, message => reports.push(message), () => { throw new Error("SecurityError"); });
  assert.equal(blocked.load(), null);
  assert.equal(blocked.save("{}"), false);
  assert.equal(reports.length, 6);
});

test("preferences bound UTF-8 bytes before schema validation", () => {
  let validations = 0;
  const owner = new BrowserPreferences(json => { validations++; return json; }, 4, () => {}, () => ({ setItem() {} }));
  assert.equal(owner.save("ééé"), false);
  assert.equal(validations, 0);
});

test("browser opener exceptions are reported by the gesture owner", () => {
  const target = canvas(), reports = [];
  const stop = signInGestures(target, () => "open_sign_in_link", () => null,
    () => "https://example.invalid/device", message => reports.push(message),
    { open() { throw new Error("Browser denied opening"); } });
  target.emit("pointerdown");
  assert.equal(reports.length, 1);
  assert.match(reports[0], /could not be opened/);
  stop();
});

test("touch and pen open links on release while mouse opens only on press", () => {
  const target = canvas(), opened = [];
  let hit = "open_sign_in_link";
  const stop = signInGestures(target, () => hit, () => null,
    () => "https://example.invalid/device", () => {}, { open: () => opened.push(true) });
  for (const pointerType of ["touch", "pen"]) {
    const before = opened.length;
    target.emit("pointerdown", { pointerType });
    assert.equal(opened.length, before);
    target.emit("pointerup", { pointerType });
    assert.equal(opened.length, before + 1);
  }
  const before = opened.length;
  target.emit("pointerdown");
  target.emit("pointerup");
  assert.equal(opened.length, before + 1);
  target.emit("pointerdown", { pointerType: "touch" });
  target.emit("pointercancel", { pointerType: "touch" });
  target.emit("pointerup", { pointerType: "touch" });
  assert.equal(opened.length, before + 1);
  hit = "cancel_sign_in";
  target.emit("pointerdown", { pointerType: "touch" });
  hit = "open_sign_in_link";
  target.emit("pointerup", { pointerType: "touch" });
  assert.equal(opened.length, before + 1);
  stop();
  assert.equal(target.listeners.size, 0);
});
