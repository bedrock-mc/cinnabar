/** Defers catalog and preview slices beyond the frame that requested them. */
export const nextDressingFrame = () => new Promise((resolve) => requestAnimationFrame(resolve));

/** Opens the page picker immediately; metadata admission precedes every file read. */
export function pickSkinFiles(cape, admit, maximumCount, signal, doc = document) {
  return new Promise((resolve, reject) => {
    if (!Number.isSafeInteger(maximumCount) || maximumCount < 1) {
      reject(new Error("The import file limit is unavailable.")); return;
    }
    const input = doc.createElement("input");
    input.type = "file";
    input.accept = cape ? ".png" : ".png,.json,.mcpack";
    input.multiple = !cape;
    input.hidden = true;
    let finished = false;
    /** Removes every listener and detached input after any terminal result. */
    const cleanup = () => {
      input.removeEventListener("change", change);
      input.removeEventListener("cancel", cancel);
      signal?.removeEventListener("abort", cancel);
      input.remove();
    };
    /** Resolves cancellation exactly once, including page teardown during an async read. */
    const cancel = () => {
      if (finished) return;
      finished = true; cleanup(); resolve([]);
    };
    /** Checks bounded metadata before making or reading any binary buffer. */
    const change = async () => {
      if (finished) return;
      try {
        const list = input.files;
        if (!list?.length) { cancel(); return; }
        if (list.length > maximumCount) throw new Error("Too many selected skin files.");
        const files = Array.from(list);
        const metadata = files.map((file) => [file.name, file.size]);
        admit(JSON.stringify(metadata), cape);
        input.removeEventListener("change", change);
        input.removeEventListener("cancel", cancel);
        input.remove();
        const selected = [];
        for (const file of files) {
          const bytes = new Uint8Array(await file.arrayBuffer());
          if (finished) return;
          if (bytes.byteLength !== file.size) throw new Error("The selected file is truncated.");
          selected.push([file.name, bytes]);
        }
        finished = true; cleanup(); resolve(selected);
      } catch (error) {
        if (!finished) { finished = true; cleanup(); reject(error); }
      }
    };
    if (signal?.aborted) { cancel(); return; }
    input.addEventListener("change", change);
    input.addEventListener("cancel", cancel);
    signal?.addEventListener("abort", cancel, { once: true });
    doc.body.append(input);
    try {
      if (input.showPicker) input.showPicker();
      else input.click();
    } catch (error) { finished = true; cleanup(); reject(error); }
  });
}

/** Owns the gesture bridge; the host hit-tests this event against its current menu state. */
export function dressingPickerEdges(target, intent, doc = document) {
  const controller = new AbortController();
  const listenerOptions = { capture: true, passive: false };
  let inFlight = null;
  /** Keeps every edge of a reserved activation out of ordinary menu input. */
  const consume = (event) => {
    event.preventDefault();
    event.stopImmediatePropagation();
  };
  /** Retires the request before publishing its result, so later gestures can import again. */
  const complete = (files, error) => {
    const { request } = inFlight;
    inFlight = null;
    request.complete(files, error);
  };
  /** Opens the current request once while this event still has browser user activation. */
  const open = () => {
    if (!inFlight || inFlight.opened) return;
    inFlight.opened = true;
    const { request } = inFlight;
    pickSkinFiles(request.cape, (metadata) => request.admit(metadata), request.maximumCount, controller.signal, doc)
      .then((files) => complete(files, null), (error) => complete([], String(error.message || error)));
  };
  /** Reserves before ordinary input, dropping every further activation until completion. */
  const gesture = (event) => {
    if (event.type === "keydown" && !["Enter", " "].includes(event.key)) return;
    if ((event.type === "pointerdown" && event.isPrimary === false)
      || (event.button != null && event.button !== 0)) return;
    if (inFlight) { consume(event); return; }
    const request = intent(event);
    if (!request) return;
    inFlight = { request, opened: false };
    consume(event);
    const onRelease = event.type === "touchstart"
      || (event.type === "pointerdown" && ["touch", "pen"].includes(event.pointerType));
    if (!onRelease) open();
  };
  /** Opens touch and pen requests on release and consumes releases while importing. */
  const release = (event) => {
    if (!inFlight || event.isPrimary === false || (event.button != null && event.button !== 0)) return;
    consume(event);
    open();
  };
  /** Cancels a touch reservation that never reached its release activation. */
  const cancel = (event) => {
    if (event?.isPrimary === false) return;
    if (!inFlight) return;
    if (event) consume(event);
    if (!inFlight.opened) complete([], null);
  };
  const listeners = [
    ["pointerdown", gesture], ["mousedown", gesture], ["touchstart", gesture],
    ["pointerup", release], ["mouseup", release], ["touchend", release],
    ["pointercancel", cancel], ["touchcancel", cancel],
    ["click", gesture], ["keydown", gesture],
  ];
  for (const [name, listener] of listeners) target.addEventListener(name, listener, listenerOptions);
  return () => {
    for (const [name, listener] of listeners) target.removeEventListener(name, listener, listenerOptions);
    cancel();
    controller.abort();
  };
}
