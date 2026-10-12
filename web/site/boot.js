// Cinnabar's browser page: once the player agrees to the Minecraft EULA it downloads the
// WebAssembly client and the game files the relay serves, then starts the client, which signs in,
// lists servers and joins through the relay itself. The relay serves this page, so the client's
// WebSockets share the page's origin.

// Panics report their whole stack, not the browser's default ten frames.
Error.stackTraceLimit = 100;

const PHYSICS_REGISTRY = "block-physics-v2193.bin";
const DEV_ASSETS = "v1/dev-assets/";
const DEV_ASSETS_INDEX = `${DEV_ASSETS}index.json`;
// Sound bank indexes kept across visits, by the hash that ends each.
const SOUND_INDEX_CACHE = "cinnabar-sound-index";
// The client module and game files kept between visits. Each is revalidated with its ETag, so a
// repeat Play reuses unchanged files instead of downloading them again.
const ASSET_CACHE = "cinnabar-assets";

import { acceptDroppedPacks, installPackStore } from "./packs.js";

// The client downloads only once the player agrees.
let clientLoad = null;

/** Downloads and starts the client module once, reporting its bytes to `onProgress`. */
function loadClient(onProgress) {
  clientLoad ??= import("./pkg/web_client.js")
    .then(async (client) => {
      const exports = await client.default({ module_or_path: await clientModule(onProgress) });
      // The client's heap in bytes, for checking its memory from the console.
      globalThis.cinnabarHeapBytes = () => exports.memory.buffer.byteLength;
      // Global Resources keep imported packs in this page's storage.
      installPackStore(client.resource_pack_extensions());
      return client;
    })
    .catch((error) => {
      clientLoad = null;
      throw error;
    });
  return clientLoad;
}

const $ = (id) => document.getElementById(id);
const relayUrl = `${location.protocol === "https:" ? "wss" : "ws"}://${location.host}`;

// Byte sizes of the served carriers, from the relay's index.
let servedSizes = new Map();

// wgpu requests no inter-stage variable limit, so devices get WebGPU's default of 16, below what
// the terrain and model shaders pass. Ask for the adapter's own limit until wgpu maps it.
if (globalThis.GPUAdapter) {
  const requestDevice = GPUAdapter.prototype.requestDevice;
  GPUAdapter.prototype.requestDevice = function (descriptor = {}) {
    const requiredLimits = { ...(descriptor.requiredLimits || {}) };
    requiredLimits.maxInterStageShaderVariables ??= this.limits.maxInterStageShaderVariables;
    return requestDevice.call(this, { ...descriptor, requiredLimits });
  };
}

// Browsers expose WebGPU only to secure pages, so a plain-HTTP LAN address hides it.
if (!navigator.gpu) {
  $("support").hidden = false;
  $("support-reason").textContent = window.isSecureContext
    ? "This browser has no WebGPU. Use a current Chrome, Edge or Safari."
    : `WebGPU needs a secure page: open this over https:// or from localhost (this is ${location.origin}).`;
}
const REQUIRED_CARRIERS = ["vanilla-v2193.mcbea", "vanilla-v1.mcbeui", "vanilla-v1.mcbeent"];

/**
 * The game files the relay serves (`-dev-assets`), by their path below the compiled-assets
 * directory, each mapped to its URL; fails when the relay serves none or lacks a required one.
 */
async function servedCarriers() {
  let index;
  try {
    const response = await fetch(DEV_ASSETS_INDEX, { cache: "no-store" });
    index = response.ok ? await response.json() : null;
  } catch {
    index = null;
  }
  if (!index) {
    throw new Error("This relay serves no game files, so Cinnabar cannot start here.");
  }
  const carriers = new Map(index.files.map(({ path }) => [path, `${DEV_ASSETS}${path}`]));
  servedSizes = new Map(index.files.map(({ path, size }) => [path, size]));
  const missing = REQUIRED_CARRIERS.filter((name) => !carriers.has(name));
  if (missing.length) {
    throw new Error(`The relay's game files are missing ${missing.join(", ")}.`);
  }
  return carriers;
}

/** Bytes `first..=last` of `url`, which must answer the range. */
async function fetchRange(url, first, last) {
  const response = await fetch(url, { headers: { Range: `bytes=${first}-${last}` } });
  if (response.status !== 206) {
    throw new Error(`${url} does not serve byte ranges (${response.status})`);
  }
  return new Uint8Array(await response.arrayBuffer());
}

/** Registers the relay's sound bank by its index alone; the client downloads each sound later. */
async function addRangedSoundBank(client, carriers, relative, source) {
  const url = new URL(source, location.href).href;
  carriers.add_ranged_compiled(relative, await soundBankPrefix(client, url), url);
}

/**
 * The index prefix of the sound bank at `url`. Its last bytes hash the rest, so a copy kept under
 * that hash stands in for the download until the bank changes.
 */
async function soundBankPrefix(client, url) {
  const header = await fetchRange(url, 0, client.sound_bank_header_bytes() - 1);
  const length = client.sound_bank_prefix_length(header);
  const hash = await fetchRange(url, length - client.sound_bank_hash_bytes(), length - 1);
  const key = `${url}?index=${Array.from(hash, (byte) => byte.toString(16).padStart(2, "0")).join("")}`;
  // Cache Storage is missing outside secure pages and may refuse writes when full.
  const cache = await globalThis.caches?.open(SOUND_INDEX_CACHE).catch(() => null);
  const kept = await cache?.match(key).catch(() => null);
  if (kept) {
    const prefix = new Uint8Array(await kept.arrayBuffer());
    if (prefix.length === length && client.sound_bank_prefix_valid(prefix)) {
      return prefix;
    }
  }
  const prefix = await fetchRange(url, 0, length - 1);
  if (cache) {
    // A recompiled bank replaces the index kept for the one before.
    for (const request of await cache.keys().catch(() => [])) {
      if (request.url.startsWith(`${url}?index=`) && request.url !== key) {
        await cache.delete(request).catch(() => {});
      }
    }
    await cache.put(key, new Response(prefix)).catch(() => {});
  }
  return prefix;
}

/**
 * The bytes of the carrier the relay serves at `url`, reporting the bytes read so far to
 * `onProgress` as they arrive; a download streams into one buffer of the `expected` size when
 * it is known.
 */
async function carrierBytes(url, expected, onProgress) {
  const { cache, kept, response } = await revalidated(url);
  if (kept) {
    const bytes = new Uint8Array(await kept.arrayBuffer());
    onProgress(bytes.length);
    return bytes;
  }
  if (!response.ok) {
    throw new Error(`could not download ${url} (${response.status})`);
  }
  if (!response.body) {
    const bytes = new Uint8Array(await response.arrayBuffer());
    onProgress(bytes.length);
    await keep(cache, url, response, bytes);
    return bytes;
  }
  const bytes = await streamedBytes(response, expected, onProgress);
  await keep(cache, url, response, bytes);
  return bytes;
}

/**
 * The body of `response`, reporting the bytes read so far to `onProgress` as they arrive, read
 * into one buffer of the `expected` size when it is known.
 */
async function streamedBytes(response, expected, onProgress) {
  const reader = response.body.getReader();
  let buffer = new Uint8Array(expected || decodedLength(response));
  let received = 0;
  let shown = 0;
  for (;;) {
    const { done, value } = await reader.read();
    if (done) {
      break;
    }
    if (received + value.length > buffer.length) {
      const grown = new Uint8Array(Math.max(buffer.length * 2, received + value.length));
      grown.set(buffer.subarray(0, received));
      buffer = grown;
    }
    buffer.set(value, received);
    received += value.length;
    if (performance.now() - shown > 50) {
      shown = performance.now();
      onProgress(received);
    }
  }
  onProgress(received);
  return received === buffer.length ? buffer : buffer.slice(0, received);
}

/**
 * The client module as a Response the browser compiles while it streams: this browser's kept
 * copy when the relay confirms it is current, else a download that is kept for the next visit,
 * its bytes received and total (0 when unknown) reported to `onProgress`.
 */
async function clientModule(onProgress) {
  const url = "pkg/web_client_bg.wasm";
  const { cache, kept, response } = await revalidated(url);
  if (kept) {
    return kept;
  }
  if (!response.ok || !response.body) {
    return response;
  }
  const total = decodedLength(response);
  // A copy is kept only where it can be stored; an unread branch would buffer the whole module.
  const keeping = cache && response.headers.get("etag");
  const [compiled, stored] = keeping ? response.body.tee() : [response.body, null];
  if (stored) {
    keep(cache, url, response, stored);
  }
  let received = 0;
  let shown = 0;
  const counted = compiled.pipeThrough(
    new TransformStream({
      transform(chunk, controller) {
        received += chunk.length;
        if (performance.now() - shown > 50) {
          shown = performance.now();
          onProgress(received, Math.max(total, received));
        }
        controller.enqueue(chunk);
      },
      flush() {
        onProgress(received, Math.max(total, received));
      },
    }),
  );
  return new Response(counted, { headers: { "Content-Type": "application/wasm" } });
}

/**
 * Fetches `url` past the HTTP cache, asking the relay whether this browser's kept copy is still
 * current: `kept` is that copy on a 304, else `response` is the download.
 */
async function revalidated(url) {
  const cache = await globalThis.caches?.open(ASSET_CACHE).catch(() => null);
  const kept = await cache?.match(url).catch(() => null);
  const etag = kept?.headers.get("etag");
  const response = await fetch(url, { cache: "no-store", headers: etag ? { "If-None-Match": etag } : {} });
  return response.status === 304 && kept ? { cache, kept } : { cache, response };
}

/**
 * Keeps `body` downloaded with `response` under `url` with its ETag, for revalidation on the
 * next visit; a missing ETag, an unavailable cache or a full quota only costs a later download.
 */
async function keep(cache, url, response, body) {
  const etag = response.headers.get("etag");
  if (!cache || !etag) {
    return;
  }
  const headers = { etag, "Content-Type": response.headers.get("content-type") ?? "application/octet-stream" };
  await cache.put(url, new Response(body, { headers })).catch(() => {});
}

/**
 * The decoded size of `response`'s body, or 0 when unknown. A compressed response's
 * Content-Length is its compressed size, so the relay names the decoded size separately.
 */
function decodedLength(response) {
  const decoded = Number(response.headers.get("x-decoded-length"));
  if (decoded) {
    return decoded;
  }
  return response.headers.get("content-encoding") ? 0 : Number(response.headers.get("content-length")) || 0;
}

/** `bytes` in megabytes with one decimal, as progress labels show them. */
function megabytes(bytes) {
  return (bytes / 1e6).toFixed(1);
}

/** Shows startup `label` over the page, with `fraction` of the bar filled when known. */
function showStartup(label, fraction) {
  $("startup").hidden = false;
  $("startup-label").textContent = label;
  if (fraction !== null && fraction !== undefined) {
    $("startup-bar").style.width = `${Math.round(fraction * 100)}%`;
  }
}

/** Resolves once the page has painted, so a label set before a long task is on screen. */
function painted() {
  return new Promise((resolve) => requestAnimationFrame(() => setTimeout(resolve)));
}

// What the player agreed to, as the desktop's first run keeps it: new consent text or a new EULA
// link asks again.
const EULA_KEY = "cinnabar.eulaAccepted";
const eulaConsent = () => `${$("eula-text").textContent.replace(/\s+/g, " ").trim()}\n${$("eula-link").href}`;

/** Whether this browser agreed to the current consent text and EULA link. */
function rememberedConsent() {
  try {
    return localStorage.getItem(EULA_KEY) === eulaConsent();
  } catch {
    // Without storage the player agrees again on each visit.
    return false;
  }
}

function rememberConsent(agreed) {
  try {
    if (agreed) {
      localStorage.setItem(EULA_KEY, eulaConsent());
    } else {
      localStorage.removeItem(EULA_KEY);
    }
  } catch {
    // Nothing to keep without storage.
  }
}

/**
 * Shows whether this browser already agreed. The player still clicks to start, since browsers
 * allow sound and mouse capture only after one.
 */
function showConsent() {
  const agreed = rememberedConsent();
  $("agree").textContent = agreed ? "Play" : "Agree";
  $("consent-state").textContent = agreed ? "You agreed to the Minecraft EULA in this browser." : "";
  $("agree").disabled = !navigator.gpu;
  $("decline").disabled = false;
}

$("decline").onclick = () => {
  rememberConsent(false);
  showConsent();
  $("play-state").textContent =
    "Cinnabar downloads no game files without your agreement. You can agree at any time.";
};

$("agree").onclick = async () => {
  rememberConsent(true);
  $("agree").disabled = true;
  $("decline").disabled = true;
  $("consent-state").textContent = "";
  $("play-state").textContent = "";
  try {
    // No Mojang-derived file downloads before the player agrees.
    const served = await servedCarriers();
    showStartup("Downloading the game…", 0);
    const client = await loadClient((received, total) =>
      showStartup(`Downloading the game ${megabytes(received)} / ${megabytes(total)} MB…`, total ? received / total : null),
    );
    // Compile reports and the prepare stamp are never read, so they are not downloaded.
    const wanted = [...served].filter(([relative]) => !client.carrier_unused(relative));
    const soundBank = client.sound_bank_file();
    const languages = `${client.language_carrier_dir()}/`;
    const carriers = new client.Carriers();
    // Whole carriers download now; the sound bank and languages are read later by URL.
    const later = (relative) => relative === soundBank || relative.startsWith(languages);
    const sizeOf = (relative) => servedSizes.get(relative) ?? 0;
    const total = wanted
      .filter(([relative]) => !later(relative))
      .reduce((sum, [relative]) => sum + sizeOf(relative), 0);
    let done = 0;
    const report = (bytes) =>
      showStartup(
        `Downloading game files ${megabytes(done + bytes)} / ${megabytes(total)} MB…`,
        total ? Math.min((done + bytes) / total, 1) : null,
      );
    for (const [relative, source] of wanted) {
      if (relative === soundBank) {
        await addRangedSoundBank(client, carriers, relative, source);
      } else if (relative.startsWith(languages)) {
        // A language downloads only once a player picks it.
        carriers.add_remote_compiled(relative, new URL(source, location.href).href);
      } else {
        const bytes = await carrierBytes(source, sizeOf(relative), report);
        done += bytes.length;
        carriers.add_compiled(relative, bytes);
      }
    }
    const physics = await fetch(PHYSICS_REGISTRY);
    if (!physics.ok) {
      throw new Error(`could not load ${PHYSICS_REGISTRY}`);
    }
    carriers.set_physics_registry(new Uint8Array(await physics.arrayBuffer()));
    $("launcher").hidden = true;
    showStartup("Preparing the game…", 0);
    await painted();
    const progress = (label, fraction, ready) => {
      if (ready) {
        $("startup").hidden = true;
      } else {
        showStartup(`${label}…`, fraction);
      }
    };
    // The client starts at its menu, where the player signs in and picks a server.
    client.start(carriers, relayUrl, progress);
    // A pack dropped on the game imports into Global Resources.
    acceptDroppedPacks(client);
  } catch (error) {
    $("startup").hidden = true;
    $("launcher").hidden = false;
    showConsent();
    $("play-state").textContent = String(error);
  }
};

showConsent();
