import test from "node:test";
import assert from "node:assert/strict";
import { pickSkinFiles, dressingPickerEdges } from "./dressing.mjs";

/** Provides a deterministic picker surface without reading the user's filesystem. */
function fixture() {
  const input = new EventTarget();
  input.remove = () => {};
  let opened = false;
  input.showPicker = () => { opened = true; };
  const doc = { createElement: () => input, body: { append() {} } };
  return { input, doc, opened: () => opened };
}

/** Creates metadata whose read can be observed or deliberately delayed. */
function file(name, size, read = async () => new ArrayBuffer(size)) {
  return { name, size, arrayBuffer: read };
}

test("picker opens synchronously and cancelled selection completes once", async () => {
  const { input, doc, opened } = fixture();
  const picked = pickSkinFiles(false, () => assert.fail("cancel must not admit"), 2, undefined, doc);
  assert.ok(opened());
  input.dispatchEvent(new Event("cancel"));
  assert.deepEqual(await picked, []);
});

test("file count and metadata fail before any binary read", async () => {
  for (const excessive of [true, false]) {
    const { input, doc } = fixture();
    let reads = 0;
    const bad = file("skin.png", 1, async () => { reads++; return new ArrayBuffer(1); });
    const picked = pickSkinFiles(false, () => { throw new Error("oversized"); }, 2, undefined, doc);
    input.files = excessive ? [bad, bad, bad] : [bad];
    input.dispatchEvent(new Event("change"));
    await assert.rejects(picked);
    assert.equal(reads, 0);
  }
});

test("companion bytes retain names and a read failure is reported", async () => {
  const { input, doc } = fixture();
  let admitted = false;
  const picked = pickSkinFiles(false, (metadata, cape) => {
    assert.equal(cape, false);
    assert.deepEqual(JSON.parse(metadata), [["Skin.PNG", 2], ["Skin.geo.json", 1]]);
    admitted = true;
  }, 2, undefined, doc);
  input.files = [file("Skin.PNG", 2, async () => { assert.ok(admitted); return new ArrayBuffer(2); }), file("Skin.geo.json", 1)];
  input.dispatchEvent(new Event("change"));
  assert.deepEqual((await picked).map(([name, bytes]) => [name, bytes.length]), [["Skin.PNG", 2], ["Skin.geo.json", 1]]);
  const next = fixture();
  const failed = pickSkinFiles(true, () => {}, 2, undefined, next.doc);
  next.input.files = [file("cape.png", 1, async () => { throw new Error("read denied"); })];
  next.input.dispatchEvent(new Event("change"));
  await assert.rejects(failed, /read denied/);
});

test("teardown during a file read cancels publication", async () => {
  const { input, doc } = fixture();
  const controller = new AbortController();
  let finish;
  const picked = pickSkinFiles(false, () => {}, 2, controller.signal, doc);
  input.files = [file("skin.png", 1, () => new Promise((resolve) => { finish = resolve; }))];
  input.dispatchEvent(new Event("change"));
  controller.abort();
  assert.deepEqual(await picked, []);
  finish(new ArrayBuffer(1));
});

test("gesture bridge keeps the ticket receiver during metadata admission", async () => {
  const { input, doc } = fixture();
  const target = new EventTarget();
  let finish;
  const completed = new Promise((resolve) => { finish = resolve; });
  const stop = dressingPickerEdges(target, () => ({
    cape: false, maximumCount: 2,
    admit() { assert.equal(this.maximumCount, 2); },
    complete(files, error) { finish({ files, error }); },
  }), doc);
  target.dispatchEvent(new Event("click"));
  input.files = [file("skin.png", 1)];
  input.dispatchEvent(new Event("change"));
  const result = await completed;
  stop();
  assert.equal(result.error, null);
  assert.equal(result.files.length, 1);
});

test("gesture bridge uses current intent, bounds concurrent pickers and owns teardown", async () => {
  const { input, doc, opened } = fixture();
  const target = new EventTarget();
  let requests = 0, completions = 0;
  let complete;
  const finished = new Promise((resolve) => { complete = resolve; });
  const stop = dressingPickerEdges(target, () => {
    requests++;
    return { cape: false, admit() {}, maximumCount: 2, complete(files, error) {
      assert.deepEqual(files, []); assert.equal(error, null); completions++; complete();
    } };
  }, doc);
  let propagated = 0;
  target.addEventListener("click", () => { propagated++; });
  target.dispatchEvent(new Event("click", { cancelable: true }));
  assert.ok(opened());
  assert.equal(propagated, 0, "a reserved import consumes the game gesture");
  target.dispatchEvent(new Event("click"));
  assert.equal(requests, 1);
  stop();
  await finished;
  target.dispatchEvent(new Event("click"));
  assert.equal(completions, 1);
  assert.equal(requests, 1);
});

for (const compatibilityClick of [false, true]) {
  for (const result of ["cancel", "change", "error"]) {
    test(`touch ${compatibilityClick ? "with" : "without"} a compatibility click permits keyboard imports after ${result}`, async () => {
      const { input, doc } = fixture();
      const target = new EventTarget();
      let requests = 0;
      const completions = [];
      let finish;
      const completed = new Promise((resolve) => { finish = resolve; });
      const stop = dressingPickerEdges(target, () => {
        requests++;
        return {
          cape: false, maximumCount: 2, admit() {},
          complete(files, error) { completions.push({ files, error }); finish(); },
        };
      }, doc);
      try {
        target.dispatchEvent(new Event("touchstart", { cancelable: true }));
        target.dispatchEvent(new Event("touchend", { cancelable: true }));
        if (compatibilityClick) target.dispatchEvent(new Event("click", { cancelable: true }));
        assert.equal(requests, 1);
        input.files = [file("skin.png", 1, async () => {
          if (result === "error") throw new Error("read denied");
          return new ArrayBuffer(1);
        })];
        input.dispatchEvent(new Event(result === "cancel" ? "cancel" : "change"));
        await completed;
        await Promise.resolve();
        assert.equal(completions.length, 1);
        assert.equal(completions[0].files.length, result === "change" ? 1 : 0);
        assert.equal(completions[0].error, result === "error" ? "read denied" : null);
        for (const key of ["Enter", " "]) {
          const keyboardCompleted = new Promise((resolve) => { finish = resolve; });
          const event = Object.assign(new Event("keydown", { cancelable: true }), { key });
          target.dispatchEvent(event);
          assert.ok(event.defaultPrevented, `${JSON.stringify(key)} starts the next import`);
          assert.equal(requests, completions.length + 1);
          input.dispatchEvent(new Event("cancel"));
          await keyboardCompleted;
          await Promise.resolve();
        }
        assert.equal(completions.length, 3);
      } finally { stop(); }
    });
  }
}

test("all activation edges are consumed while a selected file is still being read", async () => {
  const { input, doc } = fixture();
  const target = new EventTarget();
  let requests = 0, propagated = 0, finishRead, finish;
  const completed = new Promise((resolve) => { finish = resolve; });
  const stop = dressingPickerEdges(target, () => {
    requests++;
    return { cape: false, maximumCount: 2, admit() {}, complete: finish };
  }, doc);
  try {
    target.dispatchEvent(new Event("click", { cancelable: true }));
    input.files = [file("skin.png", 1, () => new Promise((resolve) => { finishRead = resolve; }))];
    input.dispatchEvent(new Event("change"));
    for (const type of ["pointerdown", "mousedown", "touchstart", "pointerup", "mouseup", "touchend", "click", "keydown"]) {
      target.addEventListener(type, () => { propagated++; });
      const event = Object.assign(new Event(type, { cancelable: true }), { key: "Enter", button: 0 });
      target.dispatchEvent(event);
      assert.ok(event.defaultPrevented, `${type} belongs to the in-flight import`);
      assert.equal(requests, 1);
    }
    assert.equal(propagated, 0);
    finishRead(new ArrayBuffer(1));
    assert.equal((await completed).length, 1);
  } finally { stop(); }
});

test("click activation does not require a primary pointer flag", async () => {
  const { input, doc } = fixture();
  const target = new EventTarget();
  let requests = 0, finish;
  const completed = new Promise((resolve) => { finish = resolve; });
  const stop = dressingPickerEdges(target, () => {
    requests++;
    return { cape: false, maximumCount: 2, admit() {}, complete: finish };
  }, doc);
  try {
    for (let repeat = 0; repeat < 2; repeat++) {
      const event = Object.assign(new Event("click", { cancelable: true }), { button: 0, isPrimary: false });
      target.dispatchEvent(event);
      assert.ok(event.defaultPrevented);
      assert.equal(requests, 1);
    }
    input.dispatchEvent(new Event("cancel"));
    assert.deepEqual(await completed, []);
  } finally { stop(); }
});

for (const [name, pointerType, events] of [
  ["mouse pointer", "mouse", ["pointerdown", "mousedown", "pointerup", "mouseup", "click"]],
  ["legacy mouse", undefined, ["mousedown", "mouseup", "click"]],
  ["touch pointer", "touch", ["pointerdown", "touchstart", "pointerup", "touchend", "click"]],
  ["legacy touch", undefined, ["touchstart", "touchend", "click"]],
  ["pen pointer", "pen", ["pointerdown", "pointerup", "click"]],
]) {
  test(`${name} reserves import before ordinary input can activate it`, async () => {
    const { input, doc, opened } = fixture();
    const target = new EventTarget();
    let ordinaryPress = false, requests = 0, propagated = 0, finish;
    const completed = new Promise((resolve) => { finish = resolve; });
    const stop = dressingPickerEdges(target, () => {
      if (ordinaryPress) return null;
      requests++;
      return { cape: false, maximumCount: 2, admit() {}, complete: finish };
    }, doc);
    for (const type of events) target.addEventListener(type, () => {
      propagated++;
      if (type.endsWith("down") || type === "touchstart") ordinaryPress = true;
    });
    try {
      for (const type of events) {
        const event = Object.assign(new Event(type, { cancelable: true }), { pointerType, button: 0 });
        target.dispatchEvent(event);
        assert.ok(event.defaultPrevented, `${type} belongs to the reserved import`);
        // A frame can drain ordinary input between the press and its click.
        await Promise.resolve();
        assert.equal(ordinaryPress, false);
      }
      assert.ok(opened());
      assert.equal(requests, 1);
      assert.equal(propagated, 0);
      input.dispatchEvent(new Event("cancel"));
      assert.deepEqual(await completed, []);
    } finally { stop(); }
  });
}

test("touch cancellation and teardown release reserved tickets without opening a picker", () => {
  for (const end of ["pointercancel", "touchcancel", "teardown"]) {
    const { doc, opened } = fixture();
    const target = new EventTarget();
    let completions = 0;
    const stop = dressingPickerEdges(target, () => ({
      cape: false, maximumCount: 2, admit() {},
      complete(files, error) { assert.deepEqual(files, []); assert.equal(error, null); completions++; },
    }), doc);
    target.dispatchEvent(Object.assign(new Event("pointerdown", { cancelable: true }), { pointerType: "touch", button: 0 }));
    assert.equal(opened(), false, "touch pickers wait for the release activation");
    if (end !== "teardown") target.dispatchEvent(new Event(end, { cancelable: true }));
    stop();
    assert.equal(completions, 1);
    target.dispatchEvent(new Event("touchend"));
    assert.equal(opened(), false);
  }
});

test("non-import presses and secondary pointers keep ordinary input", () => {
  const target = new EventTarget();
  let requests = 0, propagated = 0;
  const stop = dressingPickerEdges(target, () => { requests++; return null; }, fixture().doc);
  const events = [
    Object.assign(new Event("pointerdown", { cancelable: true }), { button: 0, isPrimary: true }),
    Object.assign(new Event("pointerdown", { cancelable: true }), { button: 2, isPrimary: true }),
    Object.assign(new Event("pointerdown", { cancelable: true }), { button: 0, isPrimary: false }),
  ];
  target.addEventListener("pointerdown", () => { propagated++; });
  for (const event of events) {
    target.dispatchEvent(event);
    assert.equal(event.defaultPrevented, false);
  }
  stop();
  assert.equal(requests, 1);
  assert.equal(propagated, events.length);
});
