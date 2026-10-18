import { boundedBytes } from "./edges.mjs";

/** Reads exactly one identity-encoded range from a single immutable HTTPS representation. */
async function readRange(source, offset, length, signal, fetcher = fetch) {
  const url = new URL(source.url);
  if (url.protocol !== "https:" || url.username || url.password) throw new Error("Sound bank requires HTTPS");
  const end = offset + length - 1;
  if (![offset, length, end].every(Number.isSafeInteger) || offset < 0 || length <= 0 ||
      (source.total !== undefined && end >= source.total)) throw new Error("Invalid sound bank range");
  const headers = { Range: `bytes=${offset}-${end}` };
  if (source.etag) headers["If-Range"] = source.etag;
  const response = await fetcher(source.url, { credentials: "omit", redirect: "error", headers, signal });
  try {
    const range = /^bytes (\d+)-(\d+)\/(\d+)$/.exec(response.headers.get("Content-Range") || "");
    const etag = response.headers.get("ETag");
    const total = range ? Number(range[3]) : NaN;
    const encoding = response.headers.get("Content-Encoding");
    if (response.status !== 206 || !range || Number(range[1]) !== offset || Number(range[2]) !== end ||
        !Number.isSafeInteger(total) || total <= end || (source.total !== undefined && source.total !== total) ||
        !etag || etag.length > 512 || !/^"[\x21\x23-\x7e\x80-\xff]*"$/.test(etag) ||
        (source.etag && etag !== source.etag) || (encoding && encoding !== "identity")) {
      throw new Error("Sound bank server must return exact ranges with a stable strong ETag");
    }
    return { bytes: await boundedBytes(response, length), etag, total };
  } catch (error) {
    if (!response.body?.locked) await response.body?.cancel();
    throw error;
  }
}

/** Fetches the bounded header and hash-checked index without downloading sound data. */
export async function openSoundBank(url, headerBytes, prefixLength, fetcher = fetch) {
  url = new URL(url);
  if (url.protocol !== "https:" || url.username || url.password) throw new Error("Sound bank requires HTTPS");
  const controller = new AbortController();
  const deadline = setTimeout(() => controller.abort(), 30_000);
  try {
    const header = await readRange({ url: url.href }, 0, headerBytes, controller.signal, fetcher);
    const source = { url: url.href, etag: header.etag, total: header.total };
    const prefix = await readRange(source, 0, prefixLength(header.bytes), controller.signal, fetcher);
    return { source, prefix: prefix.bytes, total: source.total };
  } finally { clearTimeout(deadline); }
}

/** Starts one bounded read whose owner cancels it on pack replacement or page disposal. */
export function startRange(source, offset, length, fetcher = fetch) {
  const controller = new AbortController();
  const deadline = setTimeout(() => controller.abort(), 30_000);
  const promise = readRange(source, offset, length, controller.signal, fetcher)
    .then(result => result.bytes).finally(() => clearTimeout(deadline));
  return { controller, promise };
}

/** Returns the existing read promise without issuing another HTTP request. */
export const rangePromise = handle => handle.promise;
/** Aborts the owned read; settled reads ignore this harmless teardown. */
export const cancelRange = handle => handle.controller.abort();
