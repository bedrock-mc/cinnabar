/** Samples the browser clock for relay capability expiry. */
export const milliseconds = () => performance.now();
/** Gives the shared selector a browser-owned deadline. */
export const delay = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

/** Reads an exact, bounded response; cancels surplus bytes before retaining them. */
export async function boundedBytes(response, maximum, exact = true) {
  if (!response.ok || !response.body) throw new Error("Asset request failed");
  const reader = response.body.getReader();
  const bytes = new Uint8Array(maximum);
  let used = 0;
  try {
    for (;;) {
      const { value, done } = await reader.read();
      if (done) break;
      if (value.length > maximum - used) throw new Error("Asset exceeds delivery size");
      bytes.set(value, used); used += value.length;
    }
    if (exact && used !== maximum) throw new Error("Asset is truncated");
    return bytes.subarray(0, used);
  } finally { await reader.cancel(); }
}

/** Installs canvas gesture, focus and size edges, returning an explicit teardown. */
export function canvasEdges(canvas, input, audio, view = window, doc = document) {
  const listeners = [];
  /** Registers a listener owned by this canvas session. */
  const listen = (target, name, callback) => {
    target.addEventListener(name, callback);
    listeners.push(() => target.removeEventListener(name, callback));
  };
  /** Reports whether this canvas owns pointer lock. */
  const locked = () => doc.pointerLockElement === canvas;
  /** Updates backing pixels only when CSS size or DPR changes. */
  const resize = () => {
    const dpr = view.devicePixelRatio || 1;
    const width = Math.max(1, Math.round(canvas.clientWidth * dpr));
    const height = Math.max(1, Math.round(canvas.clientHeight * dpr));
    if (canvas.width !== width) canvas.width = width;
    if (canvas.height !== height) canvas.height = height;
  };
  listen(canvas, "click", () => {
    audio.resume().catch(() => {});
    canvas.requestPointerLock()?.catch(() => {});
  });
  listen(doc, "pointerlockchange", () => { if (!locked()) input.release(); });
  listen(view, "blur", () => input.release());
  for (const [name, down] of [["keydown", true], ["keyup", false]]) {
    listen(doc, name, (event) => { if (locked()) { event.preventDefault(); input.key(event.code, down); } });
  }
  for (const [name, down] of [["mousedown", true], ["mouseup", false]]) {
    listen(canvas, name, (event) => { if (locked()) input.mouse(event.button, down); });
  }
  listen(canvas, "mousemove", (event) => { if (locked()) input.motion(event.movementX, event.movementY); });
  listen(canvas, "contextmenu", (event) => event.preventDefault());
  listen(view, "resize", resize);
  const observer = new ResizeObserver(resize);
  observer.observe(canvas); resize();
  let stopped = false;
  /** Tracks DPR changes without resetting an unchanged drawing surface. */
  const frame = () => { if (!stopped) { resize(); requestAnimationFrame(frame); } };
  requestAnimationFrame(frame);
  return () => { stopped = true; listeners.forEach((remove) => remove()); observer.disconnect(); input.release(); };
}
