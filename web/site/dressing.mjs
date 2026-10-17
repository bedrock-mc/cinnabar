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
  let active = false;
  /** Reserves a catalog ticket and opens its picker before returning from the user gesture. */
  const gesture = (event) => {
    if (active || (event.type === "keydown" && !["Enter", " "].includes(event.key))) return;
    const request = intent(event);
    if (!request) return;
    event.preventDefault();
    active = true;
    pickSkinFiles(request.cape, request.admit, request.maximumCount, controller.signal, doc)
      .then((files) => request.complete(files, null), (error) => request.complete([], String(error.message || error)))
      .finally(() => { active = false; });
  };
  target.addEventListener("click", gesture);
  target.addEventListener("keydown", gesture);
  return () => {
    target.removeEventListener("click", gesture);
    target.removeEventListener("keydown", gesture);
    controller.abort();
  };
}
