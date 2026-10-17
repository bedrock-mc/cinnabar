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

test("gesture bridge uses current intent, bounds concurrent pickers and owns teardown", async () => {
  const { input, doc, opened } = fixture();
  const target = new EventTarget();
  let requests = 0, completions = 0;
  let complete;
  const finished = new Promise((resolve) => { complete = resolve; });
  const stop = dressingPickerEdges(target, () => {
    requests++;
    return { cape: false, complete(files, error) {
      assert.deepEqual(files, []); assert.equal(error, null); completions++; complete();
    } };
  }, () => {}, 2, doc);
  target.dispatchEvent(new Event("click", { cancelable: true }));
  assert.ok(opened());
  target.dispatchEvent(new Event("click"));
  assert.equal(requests, 1);
  stop();
  await finished;
  target.dispatchEvent(new Event("click"));
  assert.equal(completions, 1);
  assert.equal(requests, 1);
});
