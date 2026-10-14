import { test } from "node:test";
import assert from "node:assert/strict";
import { boundedBytes, canvasEdges, pageLifecycle } from "./edges.mjs";

/** Supplies arbitrary network chunks and records cancellation. */
function response(chunks) {
  let cancelled = false;
  return { ok: true, body: { getReader: () => ({
    read: async () => chunks.length ? { value: Uint8Array.from(chunks.shift()), done: false } : { done: true },
    cancel: async () => { cancelled = true; },
  }) }, cancelled: () => cancelled };
}
test("bounded download accepts fragmented exact data", async () => {
  const reply = response([[1], [2, 3]]);
  assert.deepEqual(await boundedBytes(reply, 3), Uint8Array.from([1, 2, 3]));
  assert.equal(reply.cancelled(), true);
});
test("oversized and truncated downloads cancel the reader", async () => {
  for (const chunks of [[[1, 2, 3, 4]], [[1]]]) {
    const reply = response(chunks);
    await assert.rejects(boundedBytes(reply, 3));
    assert.equal(reply.cancelled(), true);
  }
});

test("cached navigation suspends and restores without freeing page resources", () => {
  const view = new EventTarget();
  const calls = [];
  const page = { suspend: () => calls.push("suspend"), resume: () => calls.push("resume") };
  pageLifecycle({ release: () => calls.push("release") },
    { suspend: async () => calls.push("audio suspended") }, () => page,
    () => calls.push("dispose"), view);
  /** Sends the browser's cache eligibility observation without a real navigation. */
  const hide = (persisted) => view.dispatchEvent(Object.assign(new Event("pagehide"), { persisted }));
  hide(true);
  view.dispatchEvent(new Event("pageshow"));
  view.dispatchEvent(new Event("pageshow"));
  hide(true);
  view.dispatchEvent(new Event("pageshow"));
  assert.deepEqual(calls, ["release", "suspend", "audio suspended", "resume",
    "release", "suspend", "audio suspended", "resume"]);
  hide(false);
  hide(false);
  view.dispatchEvent(new Event("pageshow"));
  assert.deepEqual(calls.slice(-2), ["release", "dispose"]);
});

test("canvas preserves pixels on unchanged size and releases focus", () => {
  const view = Object.assign(new EventTarget(), { devicePixelRatio: 2 });
  const doc = new EventTarget();
  const canvas = Object.assign(new EventTarget(), { clientWidth: 100, clientHeight: 50 });
  let writes = 0, pixels = 0, releases = 0, queued;
  Object.defineProperty(canvas, "width", { get: () => pixels, set: value => { pixels = value; writes++; } });
  const saved = [globalThis.ResizeObserver, globalThis.requestAnimationFrame];
  globalThis.ResizeObserver = class { observe() {} disconnect() {} };
  globalThis.requestAnimationFrame = callback => { queued = callback; };
  try {
    const input = { release: () => releases++ };
    const stop = canvasEdges(canvas, input, {}, view, doc);
    assert.equal(canvas.width, 200); assert.equal(canvas.height, 100);
    queued(); assert.equal(writes, 1);
    view.devicePixelRatio = 1; queued(); assert.equal(canvas.width, 100);
    view.dispatchEvent(new Event("blur")); assert.equal(releases, 1);
    doc.dispatchEvent(new Event("pointerlockchange")); assert.equal(releases, 2);
    stop(); view.dispatchEvent(new Event("blur")); assert.equal(releases, 3);
  } finally { [globalThis.ResizeObserver, globalThis.requestAnimationFrame] = saved; }
});
