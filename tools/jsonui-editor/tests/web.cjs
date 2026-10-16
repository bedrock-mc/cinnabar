// Run with: node --experimental-vm-modules --test tools/jsonui-editor/tests/web.cjs
const assert = require("node:assert/strict");
const fs = require("node:fs/promises");
const test = require("node:test");
const vm = require("node:vm");
const { fileURLToPath, pathToFileURL } = require("node:url");
const path = require("node:path");
const { webcrypto } = require("node:crypto");

/** Load the browser host with an idle WASM initializer and minimal DOM controls. */
async function host() {
  const nodes = new Map();
  const element = () => {
    const classes = new Set();
    return {
      value: "", checked: false, files: [], children: [],
      classList: { add: (name) => classes.add(name), contains: (name) => classes.has(name) },
      querySelectorAll: () => [{ value: "1" }, { value: "0" }, { value: "0" }],
      replaceChildren(...children) { this.children = children; },
      append(...children) { this.children.push(...children); },
      click() { return this.onclick?.({ target: this, preventDefault() {} }); },
      remove() {},
    };
  };
  const node = (id) => {
    if (!nodes.has(id)) nodes.set(id, element());
    return nodes.get(id);
  };
  const context = vm.createContext({
    console, TextEncoder, TextDecoder, Uint8Array, Blob, crypto: webcrypto,
    URL: { createObjectURL: () => "blob:pack", revokeObjectURL() {} },
    setTimeout: () => 0, clearTimeout() {},
    document: { getElementById: node, createElement: element, body: node("body"), querySelector: () => ({ hidden: true }) },
    localStorage: { getItem: () => null, setItem() {} },
    Option: class { constructor(text, value) { this.text = text; this.value = value; } },
  });
  const wasm = new vm.SyntheticModule(["default", "Editor"], function () {
    this.setExport("default", () => new Promise(() => {}));
    this.setExport("Editor", class {});
  }, { context });
  const controls = codeMirror();
  const ui = new vm.SyntheticModule(Object.keys(controls), function () {
    for (const [name, value] of Object.entries(controls)) this.setExport(name, value);
  }, { context });
  const url = pathToFileURL(path.resolve(__dirname, "../web/app.js"));
  const source = await fs.readFile(url, "utf8");
  const app = new vm.SourceTextModule(`${source}\nexport { state, addLayer, supplyWanted, setupExport, refreshExport, renderLayers, renderFiles };\nexport function attachCode(view) { code = view; }`, {
    context, identifier: url.href,
  });
  await app.link(async (specifier, parent) => {
    if (specifier.endsWith("/pkg/jsonui_editor.js")) return wasm;
    if (specifier.startsWith("https:")) return ui;
    const child = new URL(specifier, parent.identifier);
    return new vm.SourceTextModule(await fs.readFile(fileURLToPath(child), "utf8"), {
      context, identifier: child.href,
    });
  });
  await app.evaluate();
  const code = new controls.EditorView({ state: controls.EditorState.create({ doc: "", extensions: [] }) });
  app.namespace.attachCode(code);
  return { ...app.namespace, node, code };
}

/** Model document replacement and typing through the host's actual update listener. */
function codeMirror() {
  const EditorState = {
    create({ doc, extensions }) { return { doc: { toString: () => doc }, extensions }; },
  };
  class EditorView {
    static updateListener = { of: (listener) => ({ listener }) };
    constructor({ state }) { this.state = state; }
    setState(state) { this.state = state; }
    dispatch({ changes }) {
      const before = this.state.doc.toString();
      const text = before.slice(0, changes.from) + changes.insert + before.slice(changes.to);
      this.state = EditorState.create({ doc: text, extensions: this.state.extensions });
      for (const extension of this.state.extensions) extension?.listener?.({ docChanged: true });
    }
  }
  return {
    EditorView, EditorState, basicSetup: {}, placeholder: () => ({}), javascript: () => ({}),
    forceLinting() {}, linter: () => ({}), lintGutter: () => ({}), oneDark: {},
  };
}

/** Make a browser File stand-in that records each lazy read. */
function file(text) {
  return { reads: 0, async arrayBuffer() { this.reads++; return new TextEncoder().encode(text).buffer; } };
}

/** Model the workspace bridge's pack-relative paths and supplied bytes. */
function editor() {
  const staged = [];
  const pending = [];
  const loaded = new Map();
  return {
    loaded,
    add_layer: () => 0,
    stage_file: (path, bytes) => staged.push([path, bytes]),
    stage_pending: (path) => pending.push(path),
    commit_files() {
      for (const [path, bytes] of staged) loaded.set(path.slice("Pack/".length), bytes);
      return JSON.stringify(pending.map((path) => [path.slice("Pack/".length), path]));
    },
    supply: (_layer, path, bytes) => loaded.set(path, bytes),
    default_pack: () => JSON.stringify({ name: "Test", description: "", version: [1, 0, 0], min_engine_version: [1, 0, 0] }),
    export_plan: () => JSON.stringify({ plan: { files: {}, notes: [], skipped: [] }, manifest: {}, warnings: [] }),
    export_pack() { this.exported = new Map(loaded); return new Uint8Array(); },
    pack_file_name: () => "test.mcpack",
    bump_pack: (pack) => pack,
  };
}

/** Model independent workspace layers so a host edit/export reveals its actual destination. */
function layeredEditor(names) {
  const bridge = editor();
  bridge.layers = names.map((name) => ({
    name, files: new Map([["ui/screen.json", JSON.stringify({ namespace: name.toLowerCase(), screen: { type: "screen" } })]]),
    edited: new Set(),
  }));
  bridge.files = () => JSON.stringify(bridge.layers.map((layer, index) => ({
    layer: index, name: layer.name,
    files: [...layer.files.keys()].map((path) => ({ path, edited: layer.edited.has(path) })),
  })));
  bridge.screens = () => "[]";
  bridge.file_text = (index, path) => bridge.layers[index]?.files.get(path);
  bridge.remove_layer = (index) => bridge.layers.splice(index, 1);
  bridge.edit = (index, path, text) => {
    const layer = bridge.layers[index];
    if (!layer) return;
    layer.files.set(path, text);
    layer.edited.add(path);
  };
  bridge.export_plan = (request) => {
    const { layer } = JSON.parse(request);
    return JSON.stringify({
      plan: { files: Object.fromEntries(bridge.layers[layer]?.files ?? []), notes: [], skipped: [] },
      manifest: {}, warnings: [],
    });
  };
  bridge.export_pack = (request) => {
    const { layer } = JSON.parse(request);
    const selected = bridge.layers[layer];
    bridge.exported = { name: selected.name, files: new Map(selected.files) };
    return new Uint8Array();
  };
  return bridge;
}

/** Populate the actual layer/file controls from independent original fixture documents. */
function showLayers(app, names) {
  app.state.editor = layeredEditor(names);
  app.state.layers = names.map((name) => ({ name, files: new Map() }));
  app.renderLayers();
  app.renderFiles();
}

/** Submit a full owned-layer export through the browser's real export handler. */
async function exportLayer(app, index) {
  app.node("export-mode").value = "full";
  app.node("export-format").value = "mcpack";
  app.node("export-layer").value = String(index);
  app.node("export-own").checked = true;
  await app.node("export-form").onsubmit({ preventDefault() {} });
  return app.state.editor.exported;
}

test("removing another layer keeps edits and exports in the open document's layer", async () => {
  for (const { names, removed, expected } of [
    { names: ["A", "B", "C"], removed: 0, expected: 0 },
    { names: ["A", "B"], removed: 0, expected: 0 },
    { names: ["A", "B", "C"], removed: 2, expected: 1 },
  ]) {
    const app = await host();
    showLayers(app, names);
    const bridge = app.state.editor;
    const originalB = bridge.layers[1];
    const originalC = bridge.layers[2];
    const originalCText = originalC?.files.get("ui/screen.json");
    app.node("files").children.find((row) => row.title === "B: ui/screen.json").click();
    const before = app.code.state.doc.toString();
    app.node("layers").children[removed].children[1].click();
    assert.equal(app.state.open.layer, expected);
    assert.equal(app.code.state.doc.toString(), before);
    assert.equal(app.node("files").children.find((row) => row.title === "B: ui/screen.json").classList.contains("open"), true);
    const edited = JSON.stringify({ namespace: "b", screen: { type: "screen", visible: false } });
    app.code.dispatch({ changes: { from: 0, to: before.length, insert: edited } });
    assert.equal(originalB.files.get("ui/screen.json"), edited);
    assert.equal(originalB.edited.has("ui/screen.json"), true);
    if (originalC) {
      assert.equal(originalC.files.get("ui/screen.json"), originalCText);
      assert.equal(originalC.edited.size, 0);
    }
    app.setupExport();
    const exported = await exportLayer(app, expected);
    assert.equal(exported.name, "B");
    assert.equal(exported.files.get("ui/screen.json"), edited);
    if (removed === 0 && originalC) {
      const untouched = await exportLayer(app, 1);
      assert.equal(untouched.name, "C");
      assert.equal(untouched.files.get("ui/screen.json"), originalCText);
    }
  }
});

test("removing the open layer closes its document instead of selecting its replacement", async () => {
  const app = await host();
  showLayers(app, ["A", "B", "C"]);
  app.node("files").children.find((row) => row.title === "B: ui/screen.json").click();
  const buffer = app.code.state.doc.toString();
  const replacement = app.state.editor.layers[2];
  app.node("layers").children[1].children[1].click();
  assert.equal(app.state.open, null);
  assert.equal(app.code.state.doc.toString(), buffer);
  assert.equal(app.node("files").children.some((row) => row.classList.contains("open")), false);
  assert.equal(replacement.edited.size, 0);
  app.setupExport();
  const exported = await exportLayer(app, 1);
  assert.equal(exported.name, "C");
  assert.equal(exported.files.get("ui/screen.json"), replacement.files.get("ui/screen.json"));
});

test("folder requests use exact pack-relative paths when names share a suffix", async () => {
  const app = await host();
  app.state.editor = editor();
  const nested = file('{"namespace":"unrelated"}');
  const indexed = file('{"namespace":"indexed"}');
  await app.addLayer("Pack", [
    ["Pack/manifest.json", file("{}")],
    ["Pack/custom/ui/screen.json", nested],
    ["Pack/ui/screen.json", indexed],
  ]);
  await app.supplyWanted([[0, "ui/screen.json"]]);
  assert.equal(new TextDecoder().decode(app.state.editor.loaded.get("ui/screen.json")), '{"namespace":"indexed"}');
  assert.equal(nested.reads, 0);
  assert.equal(indexed.reads, 1);
});

test("full folder exports load every remaining file in the selected owned layer", async () => {
  const app = await host();
  app.state.editor = editor();
  const unused = file('{"namespace":"unused"}');
  const binary = file("binary bytes");
  const indexed = file("original");
  await app.addLayer("Pack", [
    ["Pack/manifest.json", file("{}")],
    ["Pack/ui/screen.json", indexed],
    ["Pack/ui/unused.json", unused],
    ["Pack/other/data.bin", binary],
  ]);
  await app.supplyWanted([[0, "ui/screen.json"]]);
  app.state.editor.loaded.set("ui/screen.json", new TextEncoder().encode("edited"));
  const other = file("other layer");
  app.state.layers.push({ name: "Other", files: new Map([["ui/other.json", other]]) });
  app.setupExport();
  app.node("export-mode").value = "full";
  app.node("export-format").value = "mcpack";
  app.node("export-layer").value = "0";
  app.node("export-own").checked = true;
  await app.node("export-form").onsubmit({ preventDefault() {} });
  const exported = app.state.editor.exported;
  assert.equal(new TextDecoder().decode(exported.get("ui/unused.json")), '{"namespace":"unused"}');
  assert.equal(new TextDecoder().decode(exported.get("other/data.bin")), "binary bytes");
  assert.equal(new TextDecoder().decode(exported.get("ui/screen.json")), "edited");
  assert.equal(indexed.reads, 1);
  assert.equal(other.reads, 0);
});

test("the full export preview includes pending folder files after ownership is confirmed", async () => {
  const app = await host();
  app.state.editor = editor();
  await app.addLayer("Pack", [["Pack/manifest.json", file("{}")], ["Pack/ui/unused.json", file("unused")]]);
  app.setupExport();
  app.node("export-mode").value = "full";
  app.node("export-layer").value = "0";
  app.node("export-own").checked = true;
  app.state.editor.export_plan = () => JSON.stringify({
    plan: { files: { "ui/unused.json": new TextDecoder().decode(app.state.editor.loaded.get("ui/unused.json")) }, notes: [], skipped: [] },
    manifest: {}, warnings: [],
  });
  await app.refreshExport();
  assert.match(app.node("export-preview").textContent, /ui\/unused.json\nunused/);
  assert.equal(app.node("export-download").disabled, false);
});

test("other export modes and unconfirmed layers keep unrelated folder files lazy", async () => {
  for (const [mode, own] of [["full", false], ["changed", true], ["overlay", true]]) {
    const app = await host();
    app.state.editor = editor();
    const unused = file("unused");
    await app.addLayer("Pack", [["Pack/manifest.json", file("{}")], ["Pack/ui/unused.json", unused]]);
    app.setupExport();
    app.node("export-mode").value = mode;
    app.node("export-layer").value = "0";
    app.node("export-own").checked = own;
    await app.node("export-form").onsubmit({ preventDefault() {} });
    assert.equal(unused.reads, 0);
  }
});

test("a failed folder read prevents an incomplete full export", async () => {
  const app = await host();
  app.state.editor = editor();
  await app.addLayer("Pack", [
    ["Pack/manifest.json", file("{}")],
    ["Pack/ui/unused.json", { async arrayBuffer() { throw new Error("file is unavailable"); } }],
  ]);
  app.setupExport();
  app.node("export-mode").value = "full";
  app.node("export-layer").value = "0";
  app.node("export-own").checked = true;
  await app.node("export-form").onsubmit({ preventDefault() {} });
  assert.equal(app.state.editor.exported, undefined);
  assert.match(app.node("export-status").textContent, /file is unavailable/);
});

test("a concurrent lazy read cannot overwrite bytes edited after the first supply", async () => {
  const app = await host();
  app.state.editor = editor();
  const reads = [];
  await app.addLayer("Pack", [
    ["Pack/manifest.json", file("{}")],
    ["Pack/ui/screen.json", { arrayBuffer: () => new Promise((resolve) => reads.push(resolve)) }],
  ]);
  const first = app.supplyWanted([[0, "ui/screen.json"]]);
  const second = app.supplyWanted([[0, "ui/screen.json"]]);
  reads[0](new TextEncoder().encode("original").buffer);
  await first;
  app.state.editor.loaded.set("ui/screen.json", new TextEncoder().encode("edited"));
  reads[1](new TextEncoder().encode("original").buffer);
  await second;
  assert.equal(new TextDecoder().decode(app.state.editor.loaded.get("ui/screen.json")), "edited");
});

test("removing the selected layer during a read prevents supplying or exporting another layer", async () => {
  const app = await host();
  app.state.editor = editor();
  let finish;
  await app.addLayer("Pack", [
    ["Pack/manifest.json", file("{}")],
    ["Pack/ui/unused.json", { arrayBuffer: () => new Promise((resolve) => { finish = resolve; }) }],
  ]);
  app.setupExport();
  app.node("export-mode").value = "full";
  app.node("export-layer").value = "0";
  app.node("export-own").checked = true;
  const submitting = app.node("export-form").onsubmit({ preventDefault() {} });
  app.state.layers.splice(0, 1, { name: "Other", files: new Map() });
  finish(new TextEncoder().encode("unused").buffer);
  await submitting;
  assert.equal(app.state.editor.loaded.has("ui/unused.json"), false);
  assert.equal(app.state.editor.exported, undefined);
  assert.match(app.node("export-status").textContent, /selected layer changed/);
});
