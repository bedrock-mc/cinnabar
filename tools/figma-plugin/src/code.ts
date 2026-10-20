// Plugin main thread: serializes the selected layer for the UI's converter,
// which owns every mapping decision. Field names match `jsonui_editor::figma::Node`.

type Json = { [key: string]: unknown };
type Images = { [key: string]: Uint8Array };

type Message =
  | { type: 'ready'; raster: string }
  | { type: 'convert' }
  | { type: 'select'; id: string }
  | { type: 'save-pack'; id: string; pack: unknown };

// Plugin data key holding a frame's last exported pack manifest fields.
const PACK_KEY = 'jsonui-pack';

// The converter's `@image` token, sent by the UI once it has loaded.
let rasterDirective: string | null = null;
// Bumped per serialization so a stale result never replaces a newer one.
let generation = 0;

figma.showUI(__html__, { width: 980, height: 660, themeColors: true });

figma.ui.onmessage = async (message: Message) => {
  switch (message.type) {
    case 'ready':
      rasterDirective = message.raster;
      await send();
      break;
    case 'convert':
      await send();
      break;
    case 'select': {
      const node = await figma.getNodeByIdAsync(message.id);
      if (node && node.type !== 'DOCUMENT' && node.type !== 'PAGE') {
        figma.currentPage.selection = [node];
        figma.viewport.scrollAndZoomIntoView([node]);
      }
      break;
    }
    case 'save-pack': {
      const node = await figma.getNodeByIdAsync(message.id);
      node?.setPluginData(PACK_KEY, JSON.stringify(message.pack));
      break;
    }
  }
};

figma.on('selectionchange', () => void send());

async function send(): Promise<void> {
  if (rasterDirective === null) return;
  const run = ++generation;
  const selection = figma.currentPage.selection;
  if (selection.length !== 1) {
    figma.ui.postMessage({ type: 'empty', message: 'Select one frame to convert.' });
    return;
  }
  const root = selection[0];
  const images: Images = {};
  try {
    const document = await serialize(root, root.absoluteTransform, images);
    if (run !== generation) return;
    figma.ui.postMessage({
      type: 'document',
      id: root.id,
      name: root.name,
      document: JSON.stringify(document),
      images,
      pack: root.getPluginData(PACK_KEY) || null,
    });
  } catch (error) {
    figma.ui.postMessage({ type: 'empty', message: `Could not read the selection: ${String(error)}` });
  }
}

async function serialize(node: SceneNode, parent: Transform, images: Images): Promise<Json> {
  const own = node.absoluteTransform;
  const out: Json = {
    id: node.id,
    name: node.name,
    type: node.type,
    visible: node.visible,
    x: own[0][2] - parent[0][2],
    y: own[1][2] - parent[1][2],
    width: node.width,
    height: node.height,
  };
  if ('rotation' in node) out.rotation = node.rotation;
  if ('opacity' in node) out.opacity = node.opacity;
  if ('blendMode' in node) out.blend_mode = node.blendMode;
  if ('isMask' in node) out.is_mask = node.isMask;
  if ('constraints' in node) {
    out.constraints = { horizontal: node.constraints.horizontal, vertical: node.constraints.vertical };
  }
  if ('layoutPositioning' in node) out.positioning = node.layoutPositioning;
  const sizing = layoutSizing(node);
  if (sizing) out.sizing = sizing;
  if (rasterDirective !== null && node.name.split(/\s+/).includes(rasterDirective)) {
    const key = `node:${node.id}`;
    images[key] = await node.exportAsync({ format: 'PNG' });
    out.raster = key;
    return out;
  }
  if ('effects' in node) out.effects = node.effects.map((effect) => ({ type: effect.type, visible: effect.visible }));
  if ('strokes' in node) out.strokes = node.strokes.map((paint) => ({ type: paint.type, visible: paint.visible ?? true }));
  // A mixed radius still rounds some corner; the converter only needs to know it is non-zero.
  if ('cornerRadius' in node) out.corner_radius = node.cornerRadius === figma.mixed ? 1 : node.cornerRadius;
  if ('clipsContent' in node) out.clips_content = node.clipsContent;
  if ('layoutMode' in node && node.layoutMode !== 'NONE') {
    out.layout = {
      mode: node.layoutMode,
      item_spacing: node.itemSpacing,
      padding: [node.paddingLeft, node.paddingTop, node.paddingRight, node.paddingBottom],
      primary_align: node.primaryAxisAlignItems,
      counter_align: node.counterAxisAlignItems,
      wrap: node.layoutWrap === 'WRAP',
    };
  }
  if (node.type === 'TEXT') {
    const mixed: string[] = [];
    const fills = node.fills === figma.mixed ? (mixed.push('fill'), node.getRangeFills(0, 1)) : node.fills;
    out.fills = await paints(fills as ReadonlyArray<Paint>, images);
    out.text = text(node, mixed);
  } else if ('fills' in node && node.fills !== figma.mixed) {
    out.fills = await paints(node.fills, images);
  }
  if ('children' in node) {
    const children: Json[] = [];
    for (const child of node.children) children.push(await serialize(child, own, images));
    out.children = children;
  }
  return out;
}

// `[horizontal, vertical]` auto-layout sizing, where Figma defines it.
function layoutSizing(node: SceneNode): [string, string] | null {
  if (!('layoutSizingHorizontal' in node)) return null;
  try {
    return [node.layoutSizingHorizontal, node.layoutSizingVertical];
  } catch {
    return null;
  }
}

async function paints(list: ReadonlyArray<Paint>, images: Images): Promise<Json[]> {
  const out: Json[] = [];
  for (const paint of list) {
    const entry: Json = { type: paint.type, visible: paint.visible ?? true, opacity: paint.opacity ?? 1 };
    if (paint.type === 'SOLID') entry.color = [paint.color.r, paint.color.g, paint.color.b];
    if (paint.type === 'IMAGE' && paint.imageHash) {
      entry.image = paint.imageHash;
      entry.scale_mode = paint.scaleMode;
      entry.has_filters = Object.values(paint.filters ?? {}).some((value) => value !== 0);
      const image = paint.imageHash in images ? null : figma.getImageByHash(paint.imageHash);
      if (image) images[paint.imageHash] = await image.getBytesAsync();
    }
    out.push(entry);
  }
  return out;
}

// The text and its first run's style; properties that vary are listed in `mixed`.
function text(node: TextNode, mixed: string[]): Json {
  function first<T>(value: T | PluginAPI['mixed'], name: string, range: () => unknown): T {
    if (value !== figma.mixed) return value as T;
    mixed.push(name);
    return range() as T;
  }
  const font = first<FontName>(node.fontName, 'font', () => node.getRangeFontName(0, 1));
  const size = first<number>(node.fontSize, 'font size', () => node.getRangeFontSize(0, 1));
  const spacing = first<LetterSpacing>(node.letterSpacing, 'letter spacing', () => node.getRangeLetterSpacing(0, 1));
  const line = first<LineHeight>(node.lineHeight, 'line height', () => node.getRangeLineHeight(0, 1));
  const textCase = first<TextCase>(node.textCase, 'text case', () => node.getRangeTextCase(0, 1));
  const decoration = first<TextDecoration>(node.textDecoration, 'text decoration', () =>
    node.getRangeTextDecoration(0, 1),
  );
  const pixels = (value: number, unit: string) => (unit === 'PIXELS' ? value : (value / 100) * size);
  return {
    characters: node.characters,
    font_family: font.family,
    font_style: font.style,
    font_size: size,
    align_h: node.textAlignHorizontal,
    align_v: node.textAlignVertical,
    auto_resize: node.textAutoResize,
    letter_spacing: pixels(spacing.value, spacing.unit),
    line_height: line.unit === 'AUTO' ? null : pixels(line.value, line.unit),
    text_case: textCase,
    text_decoration: decoration,
    truncate: node.textTruncation === 'ENDING',
    mixed,
  };
}
