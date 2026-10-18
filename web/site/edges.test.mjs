import { test } from "node:test";
import assert from "node:assert/strict";
import { boundedBytes, downloadCarriers, canvasEdges, pageLifecycle } from "./edges.mjs";

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

for (const required of [false, true]) {
  for (const fault of ["network", "404", "truncated", "oversized", "read", "digest"]) {
    test(`${required ? "required" : "optional"} carrier download handles ${fault} failure`, async () => {
      const inserted = [], requested = [];
      const carriers = {
        entries: () => JSON.stringify([["first", "first.bin", required], ["next", "next.bin", true]]),
        /** Models Rust rejecting a carrier before publishing its bytes. */
        insert(name, bytes) {
          if (name === "first" && fault === "digest") throw new Error("Digest mismatch");
          inserted.push([name, Array.from(bytes)]);
        },
      };
      const manifest = { entries: [{ name: "first", bytes: 3 }, { name: "next", bytes: 3 }] };
      const fetcher = async (url, options) => {
        assert.equal(options.credentials, "omit");
        requested.push(url.pathname);
        if (url.pathname === "/first.bin") {
          if (fault === "network") throw new Error("Network failure");
          if (fault === "404") return new Response(null, { status: 404 });
          if (fault === "truncated") return new Response(Uint8Array.from([1]));
          if (fault === "oversized") return new Response(Uint8Array.from([1, 2, 3, 4]));
          if (fault === "read") return { ok: true, body: { getReader: () => ({
            read: async () => { throw new Error("Stream failure"); }, cancel: async () => {},
          }) } };
        }
        return new Response(Uint8Array.from([1, 2, 3]));
      };
      const work = downloadCarriers(carriers, manifest, new URL("https://assets.invalid/"), fetcher);
      if (required) {
        await assert.rejects(work);
        assert.deepEqual(inserted, []);
        assert.deepEqual(requested, ["/first.bin"]);
      } else {
        await work;
        assert.deepEqual(inserted, [["next", [1, 2, 3]]]);
        assert.deepEqual(requested, ["/first.bin", "/next.bin"]);
      }
    });
  }
}

test("valid required and optional carriers both pass verified bytes to startup", async () => {
  const inserted = [];
  const carriers = {
    entries: () => JSON.stringify([["required", "required.bin", true], ["optional", "optional.bin", false]]),
    insert: (name, bytes) => inserted.push([name, Array.from(bytes)]),
  };
  await downloadCarriers(carriers, { entries: [{ name: "required", bytes: 2 }, { name: "optional", bytes: 1 }] },
    new URL("https://assets.invalid/"), async url => new Response(Uint8Array.from(url.pathname === "/required.bin" ? [1, 2] : [3])));
  assert.deepEqual(inserted, [["required", [1, 2]], ["optional", [3]]]);
});

test("delivery failures stay fatal unless the owner explicitly marks the carrier optional", async () => {
  const carriers = { entries: () => JSON.stringify([["required", "required.bin"]]) };
  await assert.rejects(downloadCarriers(carriers, { entries: [{ name: "required", bytes: 1 }] },
    new URL("https://assets.invalid/"), async () => new Response(null, { status: 404 })));
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

test("owned retry timers register once and settle on cancellation or expiry", async () => {
  const { startTimer, timerPromise, cancelTimer } = await import("./edges.mjs");
  const previousSet = globalThis.setTimeout;
  const previousClear = globalThis.clearTimeout;
  const active = new Map();
  let sequence = 0;
  globalThis.setTimeout = (callback, ms) => { active.set(++sequence, { callback, ms }); return sequence; };
  globalThis.clearTimeout = (id) => active.delete(id);
  try {
    const cancelled = startTimer(1250);
    assert.equal(timerPromise(cancelled), timerPromise(cancelled));
    assert.equal(active.size, 1);
    let settled = false;
    timerPromise(cancelled).then(() => { settled = true; });
    cancelTimer(cancelled); cancelTimer(cancelled);
    assert.equal(active.size, 0);
    await Promise.resolve();
    assert.equal(settled, true);
    const completed = startTimer(40);
    const { callback, ms } = active.get(sequence);
    assert.equal(ms, 40);
    active.delete(sequence); callback();
    await timerPromise(completed);
    cancelTimer(completed);
    assert.equal(active.size, 0);
  } finally {
    globalThis.setTimeout = previousSet;
    globalThis.clearTimeout = previousClear;
  }
});
