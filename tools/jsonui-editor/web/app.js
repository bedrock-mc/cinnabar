// JSON-UI editor front end: file handling, CodeMirror, preview painting and
// panels. Everything engine-side lives in the wasm `Editor`.
import init, { Editor } from "./pkg/jsonui_editor.js";
import { EditorView, basicSetup } from "https://esm.sh/codemirror@6.0.1";
import { EditorState } from "https://esm.sh/@codemirror/state@^6";
import { placeholder } from "https://esm.sh/@codemirror/view@^6";
import { javascript } from "https://esm.sh/@codemirror/lang-javascript@^6";
import { forceLinting, linter, lintGutter } from "https://esm.sh/@codemirror/lint@^6";
import { oneDark } from "https://esm.sh/@codemirror/theme-one-dark@^6";

const $ = (id) => document.getElementById(id);
const EAGER = /(^|\/)(ui\/.*\.json|texts\/en_US\.lang|textures\/.*\.json)$/i;
const IMAGE = /(^|\/)textures\/.*\.(png|tga|jpe?g)$/i;
const RENDER_DELAY = 120;

const MOCK_PRESETS = {
  // Screen controllers answer bindings they do not provide as false.
  "Screen defaults (strict)": { strict: true },
  "None (lenient)": {},
  "Example rows": { globals: { "#title_text": "Example Server" }, collections: { rows: [
    { role: "row", "#row_text": "Survival" }, { role: "row", "#row_text": "Creative" }, { role: "row", "#row_text": "Minigames" }] } },
  "Server form (action)": { strict: true, form: { type: "action", title: "My Server", body: "Pick a game mode.",
    buttons: ["Survival", "Creative", { text: "Shop", image: "textures/ui/icon_recipe_item" }] } },
  "Server form (modal)": { strict: true, form: { type: "modal", title: "Teleport?", body: "Go to spawn now?", button1: "Yes", button2: "No" } },
  "Server form (custom)": { strict: true, form: { type: "custom", title: "Settings", submit: "Save", elements: [
    { type: "label", text: "Adjust your preferences." }, { type: "toggle", text: "Show particles", on: true },
    { type: "slider", text: "Volume", fraction: 0.6 }, { type: "dropdown", text: "Team", options: ["Red", "Blue"], index: 1 },
    { type: "input", text: "Nickname", placeholder: "Steve" }] } },
  "HUD (title + scoreboard)": { strict: true, hud: { title: "Welcome", subtitle: "to the server", actionbar: "Coins: 120",
    sidebar: { title: "My Server", rows: [["Coins", "120"], ["Kills", "7"], ["Online", "42"]] },
    boss_bars: [{ name: "Event ends soon", progress: 0.6, color: "#ff55ff" }], chat: ["<Steve> hello"] } },
};
const QUICK_FLAGS = ["desktop_screen", "pocket_screen", "touch", "pocket_edition", "win10_edition", "console_edition"];

const state = {
  editor: null,
  layers: [],          // [{name, files: Map(fullPath -> File)}]
  open: null,          // {layer, path}
  frame: null,
  selected: -1,
  hover: -1,
  playing: false,
  start: 0,
  renderTimer: 0,
  screens: [],
  presets: {},
};

let code;              // CodeMirror view
let loadingDoc = false;

function setStatus(text) { $("status").textContent = text; }

function savePrefs() {
  try {
    localStorage.setItem("jsonui-editor", JSON.stringify({
      size: $("size").value, scale: $("scale").value, zoom: $("zoom").value,
      background: $("background").value, preset: $("context-preset").value,
    }));
  } catch { /* storage unavailable */ }
}

function loadPrefs() {
  try {
    const prefs = JSON.parse(localStorage.getItem("jsonui-editor") || "{}");
    for (const key of ["size", "scale", "zoom", "background"]) {
      if (prefs[key] && [...$(key).options].some((o) => o.value === prefs[key])) $(key).value = prefs[key];
    }
    return prefs;
  } catch { return {}; }
}

// ---------- CodeMirror ----------

const PLACEHOLDER = "Open a ui/*.json file from the list, or type or paste a JSON-UI file here to start a scratch file.";

function makeState(text) {
  return EditorState.create({
    doc: text,
    extensions: [
      basicSetup, javascript(), oneDark, lintGutter(), placeholder(PLACEHOLDER),
      linter(lintBuffer, { delay: 300 }),
      EditorView.updateListener.of((update) => {
        if (update.docChanged && !loadingDoc) onEdit();
      }),
    ],
  });
}

function lintBuffer(view) {
  if (!state.editor || !state.open) return [];
  const text = view.state.doc.toString();
  const out = [];
  const syntax = JSON.parse(state.editor.lint(text));
  if (syntax) {
    const from = Math.min(utf16Offset(text, syntax.offset), text.length);
    out.push({ from, to: Math.min(from + 1, text.length), severity: "error", message: syntax.message });
  }
  for (const d of state.frame?.diagnostics ?? []) {
    const loc = d.location;
    if (!loc || loc.layer !== state.open.layer || loc.path !== state.open.path || d.stage === "syntax") continue;
    const from = Math.min(utf16Offset(text, loc.start), text.length);
    const to = Math.max(from, Math.min(utf16Offset(text, loc.end), text.length));
    out.push({ from, to, severity: d.severity === "info" ? "info" : d.severity, message: `${d.stage}: ${d.message}` });
  }
  return out;
}

// Engine offsets are UTF-8 bytes; CodeMirror counts UTF-16 units.
function utf16Offset(text, byteOffset) {
  const bytes = new TextEncoder().encode(text);
  return new TextDecoder().decode(bytes.subarray(0, byteOffset)).length;
}

function openFile(layer, path, select) {
  const text = state.editor.file_text(layer, path);
  if (text === undefined || text === null) return;
  if (!state.open || state.open.layer !== layer || state.open.path !== path) {
    loadingDoc = true;
    code.setState(makeState(text));
    loadingDoc = false;
    state.open = { layer, path };
    $("file-name").textContent = `${state.layers[layer]?.name ?? layer} / ${path}`;
    renderFiles();
  }
  if (select) {
    const doc = code.state.doc.toString();
    const from = Math.min(utf16Offset(doc, select.start), doc.length);
    const to = Math.min(utf16Offset(doc, select.end), doc.length);
    code.dispatch({ selection: { anchor: from, head: to }, effects: EditorView.scrollIntoView(from, { y: "center" }) });
    code.focus();
  }
}

function onEdit() {
  if (!state.open) { startScratch(); return; }
  state.editor.edit(state.open.layer, state.open.path, code.state.doc.toString());
  $("file-state").textContent = "edited";
  $("export").disabled = false;
  scheduleRender();
  if (!document.querySelector('[data-panel="export"]').hidden) refreshExportSoon();
}

// Text typed or pasted with no file open becomes a scratch-layer file, on top
// of whatever is loaded (the example when nothing is).
let scratchPending = null;
function startScratch() {
  if (scratchPending) return;
  scratchPending = (async () => {
    if (!state.layers.length) await loadExample({ keepBuffer: true });
    const text = code.state.doc.toString();
    await createScratch(text);
  })().catch((error) => setStatus(String(error))).finally(() => { scratchPending = null; });
}

async function createScratch(text) {
  const created = JSON.parse(state.editor.new_scratch_file(text));
  if (!state.layers[created.layer]?.scratch) {
    state.layers.splice(created.layer, 0, { name: "scratch", scratch: true, files: new Map() });
  }
  state.open = null;
  openFile(created.layer, created.path);
  $("export").disabled = false;
  $("file-state").textContent = "scratch";
  afterLoad();
  const picked = state.editor.pick_screen(created.layer, created.path);
  if (picked) { $("screen").value = picked; state.selected = -1; }
  scheduleRender(0);
}

async function newFile() {
  if (!state.layers.length) await loadExample({ keepBuffer: true });
  await createScratch("");
}

// A new layer goes below the scratch layer; keep our mirror and open file in step.
function insertLayer(index, entry) {
  state.layers.splice(index, 0, entry);
  if (state.open && state.open.layer >= index) state.open.layer++;
}

// ---------- loading packs ----------

async function addLayer(name, entries) {
  const index = state.editor.add_layer(name);
  const files = new Map();
  for (const [path, file] of entries) {
    if (EAGER.test(path)) {
      state.editor.stage_file(path, new Uint8Array(await file.arrayBuffer()));
    } else if (IMAGE.test(path)) {
      state.editor.stage_pending(path);
      files.set(path, file);
    }
  }
  state.editor.commit_files(index);
  insertLayer(index, { name, files });
}

async function addZip(name, file) {
  const index = state.editor.add_layer(name);
  try {
    state.editor.add_zip(index, new Uint8Array(await file.arrayBuffer()));
  } catch (error) {
    state.editor.remove_layer(index);
    throw new Error(`${name}: ${error}`);
  }
  insertLayer(index, { name, files: new Map() });
}

async function loadEntries(groups) {
  setStatus("Loading…");
  try {
    for (const group of groups) {
      if (group.zip) await addZip(group.name, group.zip);
      else await addLayer(group.name, group.entries);
    }
  } catch (error) {
    setStatus(String(error.message ?? error));
  }
  afterLoad();
}

function afterLoad() {
  $("hint").hidden = state.layers.length > 0;
  setStatus(`${state.layers.length} layer${state.layers.length === 1 ? "" : "s"} loaded`);
  renderLayers();
  renderFiles();
  refreshScreens();
  scheduleRender(0);
}

// When the screen box is empty or names nothing, preview the open file's
// screen, else a loaded screen. Returns whether it changed the box.
function autoPickScreen() {
  const current = $("screen").value.trim();
  if (current && state.editor.has_control(current)) return false;
  let picked = state.open ? state.editor.pick_screen(state.open.layer, state.open.path) : undefined;
  if (!picked && !current) {
    picked = (state.screens.find((s) => s.reference === "start.start_screen") ?? state.screens.find((s) => s.screen))?.reference;
  }
  if (!picked || picked === current) return false;
  $("screen").value = picked;
  state.selected = -1;
  setStatus(`Previewing ${picked} (picked automatically)`);
  return true;
}

function showEmpty(message) {
  $("empty").textContent = message;
  $("empty").hidden = !message;
  $("canvas").hidden = $("overlay").hidden = Boolean(message);
}

function folderGroups(fileList) {
  const byRoot = new Map();
  for (const file of fileList) {
    const path = file.webkitRelativePath || file.name;
    const root = path.split("/")[0];
    if (!byRoot.has(root)) byRoot.set(root, []);
    byRoot.get(root).push([path, file]);
  }
  return [...byRoot].map(([name, entries]) => ({ name, entries }));
}

async function walkEntry(entry, prefix, out) {
  if (entry.isFile) {
    const file = await new Promise((resolve, reject) => entry.file(resolve, reject));
    out.push([prefix + entry.name, file]);
  } else if (entry.isDirectory) {
    const reader = entry.createReader();
    for (;;) {
      const batch = await new Promise((resolve, reject) => reader.readEntries(resolve, reject));
      if (!batch.length) break;
      for (const child of batch) {
        // Only ui/, texts/ and textures/ matter; skip the rest of big packs.
        const path = `${prefix}${entry.name}/${child.name}`;
        if (child.isDirectory && /(^|\/)(models|entity|sounds|particles|animations|animation_controllers|render_controllers|attachables|biomes|fogs)$/.test(path)) continue;
        await walkEntry(child, `${prefix}${entry.name}/`, out);
      }
    }
  }
}

async function onDrop(event) {
  event.preventDefault();
  $("drop").hidden = true;
  const groups = [];
  for (const item of [...event.dataTransfer.items]) {
    const entry = item.webkitGetAsEntry?.();
    if (entry?.isDirectory) {
      const entries = [];
      await walkEntry(entry, "", entries);
      groups.push({ name: entry.name, entries });
    } else {
      const file = item.getAsFile();
      if (file && /\.(zip|mcpack)$/i.test(file.name)) groups.push({ name: file.name, zip: file });
    }
  }
  if (groups.length) await loadEntries(groups);
}

async function loadExample(options = {}) {
  const listing = await (await fetch("examples/files.json")).json();
  const groups = [];
  for (const [name, paths] of Object.entries(listing.layers)) {
    const entries = [];
    for (const path of paths) {
      const blob = await (await fetch(`examples/${name}/${path}`)).blob();
      entries.push([`${name}/${path}`, blob]);
    }
    groups.push({ name: `example ${name}`, entries });
  }
  const buffer = options.keepBuffer ? code.state.doc.toString() : null;
  await loadEntries(groups);
  $("screen").value = listing.screen;
  $("mock-preset").value = "Example rows";
  applyMockPreset();
  if (buffer !== null && !state.open) {
    loadingDoc = true;
    code.setState(makeState(buffer));
    loadingDoc = false;
  }
}

// Supply texture images the last render asked for, then render again.
async function supplyWanted(wanted) {
  let supplied = 0;
  for (const [layer, path] of wanted) {
    const files = state.layers[layer]?.files;
    if (!files) continue;
    let file = files.get(path);
    if (!file) {
      for (const [full, candidate] of files) {
        if (full.endsWith(`/${path}`)) { file = candidate; break; }
      }
    }
    state.editor.supply(layer, path, file ? new Uint8Array(await file.arrayBuffer()) : new Uint8Array());
    supplied++;
  }
  return supplied;
}

// ---------- rendering ----------

function currentView() {
  const [w, h] = sizeValue();
  let context = {};
  let mock = {};
  try { context = JSON.parse($("context").value || "{}"); $("context-error").textContent = ""; }
  catch (error) { $("context-error").textContent = error.message; }
  try { mock = JSON.parse($("mock").value || "{}"); $("mock-error").textContent = ""; }
  catch (error) { $("mock-error").textContent = error.message; }
  return { reference: $("screen").value.trim(), size: [w, h], gui_scale: Number($("scale").value) || null, context, mock };
}

function sizeValue() {
  const value = $("size").value;
  if (value === "custom") {
    const answer = ($("size").dataset.custom || "1600x900");
    return answer.split("x").map(Number);
  }
  return value.split("x").map(Number);
}

function scheduleRender(delay = RENDER_DELAY) {
  clearTimeout(state.renderTimer);
  state.renderTimer = setTimeout(render, delay);
}

async function render() {
  if (!state.editor) return;
  if (!state.layers.length) {
    showEmpty("Nothing loaded: open a resource pack folder or zip, load the example, or paste a JSON-UI file into the editor.");
    return;
  }
  autoPickScreen();
  const view = currentView();
  if (!view.reference) {
    state.frame = null;
    renderTree();
    showEmpty("No screen selected: type namespace.control above, or click a file with a screen in it.");
    return;
  }
  try {
    state.editor.set_view(JSON.stringify(view));
  } catch (error) {
    $("mock-error").textContent = String(error);
    return;
  }
  const started = performance.now();
  let frame = JSON.parse(state.editor.render());
  for (let round = 0; round < 4 && frame.wanted.length; round++) {
    if (!(await supplyWanted(frame.wanted))) break;
    frame = JSON.parse(state.editor.render());
  }
  const laid = performance.now() - started;
  state.frame = frame;
  if (!frame.boxes.length) {
    renderTree();
    renderDiagnostics();
    showEmpty(`${view.reference} did not resolve. Check the name, or see Diagnostics.`);
    return;
  }
  showEmpty("");
  if (state.selected >= frame.boxes.length) state.selected = -1;
  paint();
  const total = performance.now() - started;
  $("frame-info").textContent = `${frame.width}×${frame.height} · GUI ${frame.gui_scale} · ${frame.boxes.length} controls · layout ${laid.toFixed(0)} ms, total ${total.toFixed(0)} ms`;
  renderTree();
  renderDiagnostics();
  if (state.selected >= 0) renderProps(state.selected);
  if (code) forceLinting(code);
  if (frame.animated && !state.playing) $("play").title = "This screen has animations";
}

function paint() {
  const frame = state.frame;
  if (!frame) return;
  const canvas = $("canvas");
  canvas.width = frame.width;
  canvas.height = frame.height;
  const pixels = state.editor.paint(Number($("time").value));
  const image = new ImageData(new Uint8ClampedArray(pixels.buffer, pixels.byteOffset, pixels.byteLength), frame.width, frame.height);
  canvas.getContext("2d").putImageData(image, 0, 0);
  layoutStage();
}

function layoutStage() {
  const frame = state.frame;
  if (!frame) return;
  const stage = $("stage");
  let zoom = $("zoom").value;
  if (zoom === "fit") {
    zoom = Math.min((stage.clientWidth - 16) / frame.width, (stage.clientHeight - 16) / frame.height);
  }
  zoom = Math.max(0.05, Number(zoom));
  const cssW = frame.width * zoom;
  const cssH = frame.height * zoom;
  for (const canvas of [$("canvas"), $("overlay")]) {
    canvas.style.width = `${cssW}px`;
    canvas.style.height = `${cssH}px`;
  }
  const overlay = $("overlay");
  const dpr = window.devicePixelRatio || 1;
  overlay.width = Math.round(cssW * dpr);
  overlay.height = Math.round(cssH * dpr);
  overlay.dataset.scale = String(zoom * dpr);
  drawOverlay();
}

function drawOverlay() {
  const frame = state.frame;
  const overlay = $("overlay");
  const ctx = overlay.getContext("2d");
  ctx.clearRect(0, 0, overlay.width, overlay.height);
  if (!frame) return;
  const k = Number(overlay.dataset.scale) * frame.gui_scale;
  const box = (index, color, dash = []) => {
    const [x, y, w, h] = frame.boxes[index].rect;
    ctx.setLineDash(dash);
    ctx.strokeStyle = color;
    ctx.strokeRect(Math.round(x * k) + 0.5, Math.round(y * k) + 0.5, Math.max(1, Math.round(w * k) - 1), Math.max(1, Math.round(h * k) - 1));
  };
  ctx.lineWidth = 1;
  if ($("show-bounds").checked) {
    frame.boxes.forEach((b, i) => { if (frame.visible[i]) box(i, "rgba(255,255,255,0.18)"); });
  }
  frame.boxes.forEach((b, i) => { if (b.type === "custom" && frame.visible[i]) box(i, "rgba(190,120,255,0.8)", [4, 3]); });
  if (state.hover >= 0 && state.hover !== state.selected) box(state.hover, "rgba(61,214,255,0.6)");
  if (state.selected >= 0) {
    ctx.lineWidth = 2;
    box(state.selected, "#3dd6ff");
    const b = frame.boxes[state.selected];
    const label = `${b.name}${b.type ? ` (${b.type})` : ""} ${b.rect[2].toFixed(1)}×${b.rect[3].toFixed(1)}`;
    ctx.font = `${12 * (window.devicePixelRatio || 1)}px ui-monospace, monospace`;
    const x = Math.max(0, b.rect[0] * k);
    const y = Math.max(14 * (window.devicePixelRatio || 1), b.rect[1] * k - 3);
    const width = ctx.measureText(label).width + 8;
    ctx.fillStyle = "rgba(10,40,50,0.9)";
    ctx.fillRect(x, y - 13 * (window.devicePixelRatio || 1), width, 15 * (window.devicePixelRatio || 1));
    ctx.fillStyle = "#bff3ff";
    ctx.fillText(label, x + 4, y - 2);
  }
}

function eventPoint(event) {
  const overlay = $("overlay");
  const rect = overlay.getBoundingClientRect();
  const frame = state.frame;
  const scale = frame.width / rect.width;
  return [((event.clientX - rect.left) * scale) / frame.gui_scale, ((event.clientY - rect.top) * scale) / frame.gui_scale];
}

function select(index, jump) {
  state.selected = index;
  drawOverlay();
  renderTree();
  renderProps(index, jump);
}

// ---------- panels ----------

function renderLayers() {
  const list = $("layers");
  list.replaceChildren();
  state.layers.forEach((layer, index) => {
    const li = document.createElement("li");
    const name = document.createElement("span");
    name.textContent = `${index}. ${layer.name}`;
    name.title = layer.name;
    const remove = document.createElement("button");
    remove.textContent = "×";
    remove.title = "Remove layer";
    remove.onclick = () => {
      state.editor.remove_layer(index);
      state.layers.splice(index, 1);
      if (state.open?.layer === index) state.open = null;
      afterLoad();
    };
    li.append(name, remove);
    list.append(li);
  });
}

function renderFiles() {
  const filter = $("file-filter").value.toLowerCase();
  const list = $("files");
  list.replaceChildren();
  const layers = JSON.parse(state.editor.files());
  for (const layer of [...layers].reverse()) {
    for (const file of layer.files) {
      if (filter && !file.path.toLowerCase().includes(filter)) continue;
      const li = document.createElement("li");
      li.textContent = file.path.replace(/^ui\//, "");
      const tag = document.createElement("span");
      tag.className = "layer-tag";
      tag.textContent = ` · ${layer.name}`;
      li.append(tag);
      li.title = `${layer.name}: ${file.path}`;
      if (file.edited) li.classList.add("edited");
      if (state.open && state.open.layer === layer.layer && state.open.path === file.path) li.classList.add("open");
      li.onclick = () => openFile(layer.layer, file.path);
      list.append(li);
    }
  }
}

function refreshScreens() {
  state.screens = JSON.parse(state.editor.screens());
  const all = $("all-controls").checked;
  const list = $("screen-list");
  list.replaceChildren();
  for (const entry of state.screens) {
    if (!all && !entry.screen) continue;
    const option = document.createElement("option");
    option.value = entry.reference;
    if (entry.engine) option.label = "rendered by the client";
    list.append(option);
  }
}

function renderTree() {
  const frame = state.frame;
  const root = $("tree");
  root.replaceChildren();
  if (!frame || !frame.boxes.length) return;
  const children = frame.boxes.map(() => []);
  frame.boxes.forEach((b, i) => { if (b.parent !== null && b.parent !== undefined) children[b.parent].push(i); });
  const selectedPath = new Set();
  for (let i = state.selected; i >= 0 && i !== null && i !== undefined; i = frame.boxes[i].parent) selectedPath.add(i);
  const build = (index, depth) => {
    const b = frame.boxes[index];
    const li = document.createElement("li");
    const kids = children[index];
    const open = depth < 2 || selectedPath.has(index);
    const toggle = document.createElement("span");
    toggle.className = "toggle";
    toggle.textContent = kids.length ? (open ? "▾" : "▸") : "";
    const node = document.createElement("span");
    node.className = "node";
    if (!frame.visible[index]) node.classList.add("hidden-node");
    if (index === state.selected) node.classList.add("selected");
    node.innerHTML = "";
    node.append(document.createTextNode(b.name || "(root)"));
    if (b.type) {
      const type = document.createElement("span");
      type.className = "type";
      type.textContent = ` ${b.type}`;
      node.append(type);
    }
    node.onclick = () => select(index, true);
    node.onmouseenter = () => { state.hover = index; drawOverlay(); };
    li.append(toggle, node);
    if (kids.length) {
      const ul = document.createElement("ul");
      const fill = () => { for (const kid of kids) ul.append(build(kid, depth + 1)); };
      if (open) fill();
      toggle.onclick = () => {
        if (ul.childElementCount) { ul.replaceChildren(); toggle.textContent = "▸"; }
        else { fill(); toggle.textContent = "▾"; }
      };
      li.append(ul);
    }
    if (index === state.selected) requestAnimationFrame(() => node.scrollIntoView({ block: "nearest", inline: "nearest" }));
    return li;
  };
  root.append(build(0, 0));
}

function locationText(loc) {
  const layer = state.layers[loc.layer]?.name ?? loc.layer;
  return `${layer}: ${loc.path}:${loc.line + 1}`;
}

function jumpTo(loc) {
  if (loc) openFile(loc.layer, loc.path, { start: loc.start, end: loc.end });
}

function renderProps(index, jump) {
  const panel = $("props");
  const info = JSON.parse(state.editor.inspect(index));
  if (!info) { panel.textContent = "No details for this control."; return; }
  panel.classList.remove("muted");
  panel.replaceChildren();
  const title = document.createElement("h3");
  title.textContent = `${info.name}${info.type ? ` · ${info.type}` : ""}${info.base ? ` @${info.base}` : ""}`;
  panel.append(title);
  if (info.definition) {
    const def = document.createElement("div");
    def.className = "link";
    def.textContent = `defined at ${locationText(info.definition)}`;
    def.onclick = () => jumpTo(info.definition);
    panel.append(def);
    if (jump) jumpTo(info.definition);
  }
  const sites = document.createElement("div");
  sites.className = "muted";
  sites.textContent = info.sites.length ? `sites: ${info.sites.join(" → ")}` : "sites: (created at runtime)";
  panel.append(sites);
  const table = document.createElement("table");
  for (const prop of info.properties) {
    const tr = document.createElement("tr");
    const key = document.createElement("td");
    key.textContent = prop.key;
    const value = document.createElement("td");
    value.className = "value";
    const text = JSON.stringify(prop.value);
    value.textContent = text.length > 160 ? `${text.slice(0, 160)}…` : text;
    value.title = text.slice(0, 4000);
    const source = document.createElement("td");
    source.className = "source";
    const s = prop.source;
    if (s.kind === "file") {
      const link = document.createElement("span");
      link.className = "link";
      link.textContent = locationText(s.location);
      link.title = s.site;
      link.onclick = () => jumpTo(s.location);
      source.append(link);
      if (s.via) {
        const via = document.createElement("div");
        via.textContent = `via ${s.via.variable}`;
        if (s.via.declared) {
          via.className = "link";
          via.title = `declared at ${locationText(s.via.declared)} (${s.via.declared_site})`;
          via.onclick = () => jumpTo(s.via.declared);
        }
        source.append(via);
      }
    } else {
      source.textContent = { binding: "binding (mock data)", engine: "engine", unknown: "context / unknown" }[s.kind] ?? s.kind;
    }
    tr.append(key, value, source);
    table.append(tr);
  }
  panel.append(table);
}

function renderDiagnostics() {
  const list = $("diagnostics");
  list.replaceChildren();
  const diagnostics = state.frame?.diagnostics ?? [];
  const errors = diagnostics.filter((d) => d.severity === "error").length;
  $("diag-count").textContent = String(diagnostics.length);
  $("diag-count").classList.toggle("has-errors", errors > 0);
  for (const d of diagnostics) {
    const li = document.createElement("li");
    const sev = document.createElement("span");
    sev.className = `sev ${d.severity}`;
    sev.textContent = d.severity;
    const stage = document.createElement("span");
    stage.className = "stage-name";
    stage.textContent = d.stage;
    const message = document.createElement("span");
    message.textContent = d.message;
    li.append(sev, stage, message);
    if (d.location) {
      const where = document.createElement("span");
      where.className = "where";
      where.textContent = locationText(d.location);
      where.onclick = () => jumpTo(d.location);
      li.append(where);
    }
    list.append(li);
  }
}

function applyMockPreset() {
  $("mock").value = JSON.stringify(MOCK_PRESETS[$("mock-preset").value] ?? {}, null, 2);
  scheduleRender(0);
}

function applyContextPreset() {
  $("context").value = JSON.stringify(state.presets[$("context-preset").value] ?? {}, null, 2);
  renderFlags();
  savePrefs();
  scheduleRender(0);
}

function renderFlags() {
  const holder = $("flags");
  holder.replaceChildren();
  let context = {};
  try { context = JSON.parse($("context").value || "{}"); } catch { return; }
  for (const flag of QUICK_FLAGS) {
    const label = document.createElement("label");
    const box = document.createElement("input");
    box.type = "checkbox";
    box.checked = context[flag] === true;
    box.onchange = () => {
      let current = {};
      try { current = JSON.parse($("context").value || "{}"); } catch { return; }
      current[flag] = box.checked;
      $("context").value = JSON.stringify(current, null, 2);
      scheduleRender(0);
    };
    label.append(box, ` $${flag}`);
    holder.append(label);
  }
}

function showTab(name) {
  for (const tab of document.querySelectorAll("[role=tab]")) tab.setAttribute("aria-selected", String(tab.dataset.tab === name));
  for (const panel of document.querySelectorAll(".panel")) panel.hidden = panel.dataset.panel !== name;
  if (name === "export") refreshExport();
}

// ---------- export ----------

let exportTimer = 0;
function refreshExportSoon() {
  clearTimeout(exportTimer);
  exportTimer = setTimeout(refreshExport, 400);
}

const PACK_KEY = "jsonui-editor-pack";
let packIcon = null;

function uuid() {
  if (crypto.randomUUID) return crypto.randomUUID();
  const b = crypto.getRandomValues(new Uint8Array(16));
  b[6] = (b[6] & 0x0f) | 0x40;
  b[8] = (b[8] & 0x3f) | 0x80;
  const h = [...b].map((x) => x.toString(16).padStart(2, "0")).join("");
  return `${h.slice(0, 8)}-${h.slice(8, 12)}-${h.slice(12, 16)}-${h.slice(16, 20)}-${h.slice(20)}`;
}

// The pack's settings persist per browser so re-exports update the same pack.
function loadPack() {
  try {
    const saved = JSON.parse(localStorage.getItem(PACK_KEY) || "null");
    if (saved && saved.header_uuid && saved.module_uuid) return saved;
  } catch { /* storage unavailable */ }
  return JSON.parse(state.editor.default_pack("My JSON-UI pack", uuid(), uuid()));
}

function savePack(pack) {
  try { localStorage.setItem(PACK_KEY, JSON.stringify(pack)); } catch { /* storage unavailable */ }
}

function triple(id) { return [...$(id).querySelectorAll("input")].map((i) => Math.max(0, Number(i.value) | 0)); }
function setTriple(id, values) { [...$(id).querySelectorAll("input")].forEach((input, i) => { input.value = values[i] ?? 0; }); }

function showPack(pack) {
  $("pack-name").value = pack.name;
  $("pack-description").value = pack.description;
  setTriple("pack-version", pack.version);
  setTriple("pack-engine", pack.min_engine_version);
  $("pack-header").value = pack.header_uuid;
  $("pack-module").value = pack.module_uuid;
}

function formPack() {
  return {
    name: $("pack-name").value.trim(), description: $("pack-description").value.trim(),
    version: triple("pack-version"), min_engine_version: triple("pack-engine"),
    header_uuid: $("pack-header").value, module_uuid: $("pack-module").value,
  };
}

function exportRequest() {
  const mode = $("export-mode").value;
  return {
    mode, format: $("export-format").value, pack: formPack(),
    layer: mode === "full" ? Number($("export-layer").value) : null,
    own_layer: mode === "full" && $("export-own").checked,
  };
}

function refreshExport() {
  if ($("export-mode").value === "full") {
    const select = $("export-layer");
    const current = select.value;
    select.replaceChildren(...state.layers.map((layer, i) => new Option(`${i}. ${layer.name}`, String(i))));
    if (current && Number(current) < state.layers.length) select.value = current;
    else if (state.layers.length) select.value = String(state.layers.length - 1);
  }
  $("full-options").hidden = $("export-mode").value !== "full";
  const warnings = $("export-warnings");
  warnings.replaceChildren();
  let result;
  try {
    result = JSON.parse(state.editor.export_plan(JSON.stringify(exportRequest())));
  } catch (error) {
    $("export-preview").textContent = String(error);
    $("export-download").disabled = true;
    return;
  }
  const { plan, manifest } = result;
  const lines = [];
  const notes = [...plan.notes, ...(plan.skipped.length ? [`${plan.skipped.length} files left out (not yours to ship)`] : [])];
  for (const d of result.warnings) notes.push(`error in ${d.location ? locationText(d.location) : d.stage}: ${d.message}`);
  for (const note of notes) {
    const li = document.createElement("li");
    li.textContent = note;
    warnings.append(li);
  }
  lines.push(`manifest.json\n${JSON.stringify(manifest, null, 2)}`);
  if (packIcon) lines.push(`pack_icon.png (${packIcon.length} bytes)`);
  const paths = Object.keys(plan.files);
  for (const path of paths) lines.push(`${path}\n${plan.files[path]}`);
  $("export-preview").textContent = paths.length ? lines.join("\n\n") : "Nothing to export yet: edit a file or paste into the editor.";
  $("export-download").disabled = !paths.length;
}

function setupExport() {
  const pack = loadPack();
  showPack(pack);
  for (const id of ["pack-name", "pack-description", "export-own"]) $(id).oninput = () => { savePack(formPack()); refreshExport(); };
  for (const id of ["pack-version", "pack-engine"]) $(id).oninput = () => { savePack(formPack()); refreshExport(); };
  $("export-mode").onchange = refreshExport;
  $("export-layer").onchange = refreshExport;
  $("export-format").onchange = refreshExport;
  $("pack-regenerate").onclick = () => {
    $("pack-header").value = uuid();
    $("pack-module").value = uuid();
    savePack(formPack());
    refreshExport();
  };
  $("pack-icon").onchange = async (event) => {
    const file = event.target.files[0];
    packIcon = null;
    $("pack-icon-note").textContent = "";
    if (!file) return refreshExport();
    const bytes = new Uint8Array(await file.arrayBuffer());
    try {
      const warnings = JSON.parse(state.editor.check_icon(bytes));
      packIcon = bytes;
      $("pack-icon-note").textContent = warnings.join(" ") || "Icon ready.";
    } catch (error) {
      $("pack-icon-note").textContent = String(error);
    }
    refreshExport();
  };
  $("export-form").onsubmit = (event) => {
    event.preventDefault();
    const request = exportRequest();
    let bytes;
    try {
      bytes = state.editor.export_pack(JSON.stringify(request), packIcon ?? undefined);
    } catch (error) {
      $("export-status").textContent = String(error);
      return;
    }
    const name = state.editor.pack_file_name(request.pack.name, request.format);
    const url = URL.createObjectURL(new Blob([bytes], { type: "application/zip" }));
    const a = Object.assign(document.createElement("a"), { href: url, download: name });
    document.body.append(a);
    a.click();
    a.remove();
    setTimeout(() => URL.revokeObjectURL(url), 1000);
    const next = JSON.parse(state.editor.bump_pack(JSON.stringify(request.pack)));
    savePack(next);
    showPack(next);
    $("export-status").textContent = `Exported ${name} (v${request.pack.version.join(".")}); the next export will be v${next.version.join(".")}.`;
    refreshExport();
  };
}

function tick() {
  if (!state.playing) return;
  let t = (performance.now() - state.start) / 1000;
  if (t > Number($("time").max)) { state.start = performance.now(); t = 0; }
  $("time").value = String(t);
  $("time-label").textContent = `${t.toFixed(2)} s`;
  paint();
  requestAnimationFrame(tick);
}

// ---------- wiring ----------

async function main() {
  await init();
  state.editor = new Editor();
  code = new EditorView({ state: makeState(""), parent: $("editor") });
  try {
    const response = await fetch("cinnangles-sans.mcbefont");
    if (response.ok) state.editor.load_font(new Uint8Array(await response.arrayBuffer()));
  } catch (error) {
    console.warn("font unavailable", error);
  }
  if (!state.editor.has_font()) setStatus("No font carrier: text measures with a fixed-advance fallback.");

  const prefs = loadPrefs();
  state.presets = JSON.parse(state.editor.context_presets());
  for (const name of Object.keys(state.presets)) $("context-preset").append(new Option(name, name));
  if (prefs.preset && state.presets[prefs.preset]) $("context-preset").value = prefs.preset;
  $("context").value = JSON.stringify(state.presets[$("context-preset").value], null, 2);
  renderFlags();
  for (const name of Object.keys(MOCK_PRESETS)) $("mock-preset").append(new Option(name, name));
  $("mock").value = JSON.stringify(MOCK_PRESETS["Screen defaults (strict)"], null, 2);
  $("stage").className = `stage ${$("background").value}`;

  $("open-folder").onchange = async (e) => { await loadEntries(folderGroups(e.target.files)); e.target.value = ""; };
  $("open-zip").onchange = async (e) => {
    await loadEntries([...e.target.files].map((file) => ({ name: file.name, zip: file })));
    e.target.value = "";
  };
  $("load-example").onclick = () => loadExample().catch((error) => setStatus(String(error)));
  $("new-file").onclick = () => newFile().catch((error) => setStatus(String(error)));
  $("export").onclick = () => showTab("export");
  setupExport();
  $("file-filter").oninput = renderFiles;
  $("screen").onchange = () => { state.selected = -1; scheduleRender(0); };
  $("all-controls").onchange = refreshScreens;
  $("size").onchange = () => {
    if ($("size").value === "custom") {
      const answer = prompt("Window size in physical pixels (WxH)", $("size").dataset.custom || "1600x900");
      if (answer && /^\d+x\d+$/.test(answer)) $("size").dataset.custom = answer;
    }
    savePrefs();
    scheduleRender(0);
  };
  $("scale").onchange = () => { savePrefs(); scheduleRender(0); };
  $("zoom").onchange = () => { savePrefs(); layoutStage(); };
  $("background").onchange = () => { $("stage").className = `stage ${$("background").value}`; savePrefs(); };
  $("show-bounds").onchange = drawOverlay;
  $("mock").oninput = () => scheduleRender();
  $("mock-preset").onchange = applyMockPreset;
  $("context").oninput = () => { renderFlags(); scheduleRender(); };
  $("context-preset").onchange = applyContextPreset;
  $("time").oninput = () => { $("time-label").textContent = `${Number($("time").value).toFixed(2)} s`; paint(); };
  $("play").onclick = () => {
    state.playing = !state.playing;
    $("play").textContent = state.playing ? "❚❚" : "▶";
    state.start = performance.now() - Number($("time").value) * 1000;
    if (state.playing) requestAnimationFrame(tick);
  };
  const overlay = $("overlay");
  overlay.onmousemove = (e) => {
    if (!state.frame) return;
    const [x, y] = eventPoint(e);
    const index = state.editor.pick(x, y);
    if (index !== state.hover) { state.hover = index; drawOverlay(); }
  };
  overlay.onmouseleave = () => { state.hover = -1; drawOverlay(); };
  overlay.onclick = (e) => {
    if (!state.frame) return;
    const [x, y] = eventPoint(e);
    const index = state.editor.pick(x, y);
    if (index >= 0) select(index, true);
  };
  for (const tab of document.querySelectorAll("[role=tab]")) tab.onclick = () => showTab(tab.dataset.tab);
  window.addEventListener("resize", layoutStage);
  let dragDepth = 0;
  window.addEventListener("dragenter", (e) => { e.preventDefault(); dragDepth++; $("drop").hidden = false; });
  window.addEventListener("dragleave", () => { if (--dragDepth <= 0) { dragDepth = 0; $("drop").hidden = true; } });
  window.addEventListener("dragover", (e) => e.preventDefault());
  window.addEventListener("drop", (e) => { dragDepth = 0; onDrop(e); });
  setStatus(state.editor.has_font() ? "Ready" : $("status").textContent);
  render();
}

main().catch((error) => setStatus(`Failed to start: ${error}`));
