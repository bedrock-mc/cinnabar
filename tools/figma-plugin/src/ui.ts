// Plugin UI: runs the JSON-UI editor core (WebAssembly) to convert the
// serialized frame, preview it with Cinnabar's renderer and export the pack.

import init, { Editor } from '../pkg/jsonui_editor.js';
import wasm from 'virtual:wasm';
import font from 'virtual:font';

type Warning = { node_id: string; layer: string; message: string };
type Diagnostic = { severity: string; stage: string; message: string };
type Conversion = {
  namespace: string;
  font_types: string[];
  screen: string;
  size: [number, number];
  warnings: Warning[];
  diagnostics: Diagnostic[];
  ui_path: string;
  files: string[];
};
type Document = { id: string; name: string; document: string; images: { [key: string]: Uint8Array }; pack: string | null };
type Incoming = ({ type: 'document' } & Document) | { type: 'empty'; message: string };

const element = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;
const status = element<HTMLParagraphElement>('status');
const canvas = element<HTMLCanvasElement>('preview');
const warnings = element<HTMLUListElement>('warnings');
const warningCount = element<HTMLSpanElement>('warning-count');
const source = element<HTMLPreElement>('source');
const scale = element<HTMLSelectElement>('scale');
const format = element<HTMLSelectElement>('format');
const exportButton = element<HTMLButtonElement>('export');

// The converted frame and the pack fields its last export used.
let current: { id: string; name: string; pack: unknown } | null = null;

const post = (message: unknown) => parent.postMessage({ pluginMessage: message }, '*');

async function main(): Promise<void> {
  await init({ module_or_path: wasm });
  const editor = new Editor();
  if (font) editor.load_font(font);
  window.onmessage = (event: MessageEvent) => {
    const message = event.data.pluginMessage as Incoming | undefined;
    if (!message) return;
    if (message.type === 'empty') {
      current = null;
      exportButton.disabled = true;
      status.textContent = message.message;
      return;
    }
    try {
      show(editor, message);
    } catch (error) {
      status.textContent = `Conversion failed: ${String(error)}`;
    }
  };
  scale.onchange = () => post({ type: 'convert' });
  element<HTMLButtonElement>('refresh').onclick = () => post({ type: 'convert' });
  element<HTMLButtonElement>('adopt').onclick = () => post({ type: 'convert-selection' });
  exportButton.onclick = () => exportPack(editor);
  post({ type: 'ready', directives: JSON.parse(editor.figma_directives()) });
}

function show(editor: Editor, message: Document): void {
  for (const [key, bytes] of Object.entries(message.images)) editor.stage_figma_image(key, bytes);
  const conversion = JSON.parse(editor.convert_figma(message.document, Number(scale.value))) as Conversion;
  current = { id: message.id, name: message.name, pack: message.pack ? JSON.parse(message.pack) : null };
  const frame = JSON.parse(editor.render()) as { width: number; height: number; gui_scale: number; diagnostics: Diagnostic[] };
  canvas.width = frame.width;
  canvas.height = frame.height;
  const pixels = new Uint8ClampedArray(editor.paint(0));
  canvas.getContext('2d')?.putImageData(new ImageData(pixels, frame.width, frame.height), 0, 0);
  const [width, height] = conversion.size;
  status.textContent = `${conversion.screen}: ${width}×${height} at GUI scale ${frame.gui_scale}`;
  // The game's named fonts are not shipped; the preview draws them with its own.
  const fonts = conversion.font_types.map((font) => ({
    severity: 'info',
    stage: 'preview',
    message: `previewed with the default font; ${font} draws in its own font in game`,
  }));
  listWarnings(conversion.warnings, [...fonts, ...conversion.diagnostics, ...frame.diagnostics]);
  source.textContent = editor.file_text(0, conversion.ui_path) ?? '';
  exportButton.disabled = false;
}

function listWarnings(list: Warning[], diagnostics: Diagnostic[]): void {
  warnings.replaceChildren();
  for (const warning of list) {
    const item = document.createElement('li');
    const layer = document.createElement('button');
    layer.className = 'layer';
    layer.textContent = warning.layer;
    layer.onclick = () => post({ type: 'select', id: warning.node_id });
    item.append(layer, document.createTextNode(warning.message));
    warnings.append(item);
  }
  for (const diagnostic of diagnostics) {
    const item = document.createElement('li');
    item.className = diagnostic.severity;
    item.textContent = `${diagnostic.stage}: ${diagnostic.message}`;
    warnings.append(item);
  }
  const total = list.length + diagnostics.length;
  warningCount.textContent = total === 0 ? 'none' : String(total);
}

function exportPack(editor: Editor): void {
  if (!current) return;
  const pack = current.pack
    ? JSON.parse(editor.bump_pack(JSON.stringify(current.pack)))
    : JSON.parse(editor.default_pack(current.name, uuid(), uuid()));
  const request = { mode: 'full', layer: 0, own_layer: true, format: format.value, pack };
  try {
    const bytes = editor.export_pack(JSON.stringify(request));
    const link = document.createElement('a');
    link.href = URL.createObjectURL(new Blob([new Uint8Array(bytes)], { type: 'application/zip' }));
    link.download = editor.pack_file_name(pack.name, format.value);
    link.click();
    setTimeout(() => URL.revokeObjectURL(link.href), 10_000);
    current.pack = pack;
    post({ type: 'save-pack', id: current.id, pack });
  } catch (error) {
    status.textContent = `Export failed: ${String(error)}`;
  }
}

// A random version 4 UUID; `crypto.randomUUID` needs a secure context the plugin iframe lacks.
function uuid(): string {
  const bytes = crypto.getRandomValues(new Uint8Array(16));
  bytes[6] = (bytes[6] & 0x0f) | 0x40;
  bytes[8] = (bytes[8] & 0x3f) | 0x80;
  const hex = Array.from(bytes, (byte) => byte.toString(16).padStart(2, '0')).join('');
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
}

main().catch((error) => {
  status.textContent = `Could not start the converter: ${String(error)}`;
});
