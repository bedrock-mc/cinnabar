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
  const node = (id) => {
    if (!nodes.has(id)) nodes.set(id, {
      value: "", checked: false, files: [],
      querySelectorAll: () => [{ value: "1" }, { value: "0" }, { value: "0" }],
      replaceChildren() {}, append() {}, click() {}, remove() {},
    });
    return nodes.get(id);
  };
  const context = vm.createContext({
    console, TextEncoder, TextDecoder, Uint8Array, Blob, crypto: webcrypto,
    URL: { createObjectURL: () => "blob:pack", revokeObjectURL() {} },
    setTimeout: () => 0, clearTimeout() {},
    document: { getElementById: node, createElement: () => node("anchor"), body: node("body") },
    localStorage: { getItem: () => null, setItem() {} },
    Option: class { constructor(text, value) { this.text = text; this.value = value; } },
  });
  const wasm = new vm.SyntheticModule(["default", "Editor"], function () {
    this.setExport("default", () => new Promise(() => {}));
    this.setExport("Editor", class {});
  }, { context });
  const ui = new vm.SyntheticModule([
    "EditorView", "basicSetup", "EditorState", "placeholder", "javascript",
    "forceLinting", "linter", "lintGutter", "oneDark",
  ], function () {
    for (const name of this.namespace ? Object.keys(this.namespace) : []) this.setExport(name, {});
  }, { context });
  const url = pathToFileURL(path.resolve(__dirname, "../web/app.js"));
  const source = await fs.readFile(url, "utf8");
  const app = new vm.SourceTextModule(`${source}\nexport { state, addLayer, supplyWanted, setupExport, refreshExport };`, {
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
  return { ...app.namespace, node };
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
