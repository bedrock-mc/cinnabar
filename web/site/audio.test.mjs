import { test } from "node:test";
import assert from "node:assert/strict";
import { openSoundBank, startRange, rangePromise, cancelRange } from "./audio.mjs";

/** Serves a synthetic bank and records ranges without external network access. */
function fixture(change = response => response) {
  const requests = [];
  const bytes = Uint8Array.from({ length: 64 }, (_, index) => index);
  const fetcher = async (url, options) => {
    requests.push({ url, ...options });
    const [, first, last] = /^bytes=(\d+)-(\d+)$/.exec(options.headers.Range);
    return change(new Response(bytes.slice(Number(first), Number(last) + 1), {
      status: 206,
      headers: { "Content-Range": `bytes ${first}-${last}/${bytes.length}`, ETag: '"fixture-v1"' },
    }), requests.length);
  };
  return { requests, fetcher };
}

test("bank startup reads only the bounded prefix and pins later ranges", async () => {
  const { fetcher, requests } = fixture();
  const bank = await openSoundBank("https://assets.invalid/bank", 8, () => 16, fetcher);
  assert.equal(bank.prefix.length, 16);
  assert.deepEqual(requests.map(request => request.headers.Range), ["bytes=0-7", "bytes=0-15"]);
  assert.equal(requests[1].headers["If-Range"], '"fixture-v1"');
  const request = startRange(bank.source, 40, 4, fetcher);
  assert.deepEqual(await rangePromise(request), Uint8Array.from([40, 41, 42, 43]));
  cancelRange(request);
  assert.equal(requests[2].headers["If-Range"], '"fixture-v1"');
});

test("range reads reject whole bodies, mismatched extents, weak tags and compressed bodies", async () => {
  for (const headers of [
    { "Content-Range": "bytes 1-8/64" }, { "Content-Range": "bytes 0-7/*" },
    { ETag: 'W/"weak"' }, { "Content-Encoding": "gzip" }, { ETag: "" },
  ]) {
    const { fetcher } = fixture(response => {
      for (const [name, value] of Object.entries(headers)) response.headers.set(name, value);
      return response;
    });
    await assert.rejects(openSoundBank("https://assets.invalid/bank", 8, () => 16, fetcher));
  }
  await assert.rejects(openSoundBank("https://assets.invalid/bank", 8, () => 16, async () => new Response(new Uint8Array(64))));
});

test("changed representations and truncated or oversized range bodies fail closed", async () => {
  for (const fault of ["etag", "total", "short", "long"]) {
    const { fetcher } = fixture((response, index) => {
      if (index !== 2) return response;
      if (fault === "etag") response.headers.set("ETag", '"fixture-v2"');
      if (fault === "total") response.headers.set("Content-Range", "bytes 0-15/65");
      if (fault === "short" || fault === "long") return new Response(new Uint8Array(fault === "short" ? 15 : 17), { status: 206, headers: response.headers });
      return response;
    });
    await assert.rejects(openSoundBank("https://assets.invalid/bank", 8, () => 16, fetcher));
  }
});

test("cancelling an admitted range aborts its fetch", async () => {
  const source = { url: "https://assets.invalid/bank", etag: '"fixture"', total: 64 };
  const request = startRange(source, 40, 4, async (_, { signal }) => new Promise((_, reject) => {
    signal.addEventListener("abort", () => reject(signal.reason), { once: true });
  }));
  const rejected = assert.rejects(rangePromise(request));
  cancelRange(request);
  await rejected;
});

test("invalid range extents do not issue requests", async () => {
  const { fetcher, requests } = fixture();
  for (const [offset, length] of [[-1, 4], [63, 2], [0, 0], [0.5, 2], [Number.MAX_SAFE_INTEGER, 2]]) {
    const request = startRange({ url: "https://assets.invalid/bank", total: 64 }, offset, length, fetcher);
    await assert.rejects(rangePromise(request));
  }
  assert.equal(requests.length, 0);
});
