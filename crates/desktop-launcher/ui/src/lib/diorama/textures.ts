// Procedural pixel textures for the diorama. Every pixel is generated here; no game art is used.

export const TILE = 16;

/** Block materials in atlas order. */
export const BLOCKS = ['slate', 'dolomite', 'mossTop', 'mossSide', 'ore', 'deepslate'] as const;
export type Block = (typeof BLOCKS)[number];

type Rgb = [number, number, number];

function rng(seed: number) {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

const hex = (h: number): Rgb => [(h >> 16) & 255, (h >> 8) & 255, h & 255];
const shade = ([r, g, b]: Rgb, k: number): Rgb => [r * k, g * k, b * k];

function put(img: ImageData, x: number, y: number, c: Rgb, a = 255) {
  const i = (y * img.width + x) * 4;
  img.data[i] = Math.max(0, Math.min(255, c[0]));
  img.data[i + 1] = Math.max(0, Math.min(255, c[1]));
  img.data[i + 2] = Math.max(0, Math.min(255, c[2]));
  img.data[i + 3] = a;
}

/** Stone with soft strata: two-tone noise biased into horizontal bands. */
function stone(img: ImageData, ox: number, base: Rgb, seed: number, band = 0.06) {
  const r = rng(seed);
  for (let y = 0; y < TILE; y++) {
    const strata = 1 + Math.sin(y * 1.3 + seed) * band;
    for (let x = 0; x < TILE; x++) put(img, ox + x, y, shade(base, strata * (0.93 + r() * 0.1)));
  }
  // A few darker pits and pale flecks so faces read as rock at close range.
  for (let i = 0; i < 7; i++) put(img, ox + Math.floor(r() * TILE), Math.floor(r() * TILE), shade(base, 0.72));
  for (let i = 0; i < 4; i++) put(img, ox + Math.floor(r() * TILE), Math.floor(r() * TILE), shade(base, 1.18));
}

function moss(img: ImageData, ox: number, seed: number, rows: number) {
  const r = rng(seed);
  const greens = [hex(0x6f8f6a), hex(0x5f7f5d), hex(0x7c9b72), hex(0x587559)];
  for (let x = 0; x < TILE; x++) {
    const depth = rows === TILE ? TILE : rows + Math.floor(r() * 2.2);
    for (let y = 0; y < Math.min(TILE, depth); y++) put(img, ox + x, y, shade(greens[Math.floor(r() * greens.length)]!, 0.95 + r() * 0.1));
  }
}

/** Cinnabar: a dark red ore mass with bright crystal facets; `glow` paints only the facets. */
function ore(img: ImageData, glow: ImageData, ox: number, seed: number) {
  stone(img, ox, hex(0x5a2420), seed, 0.04);
  const r = rng(seed + 7);
  for (let i = 0; i < 9; i++) {
    const cx = 1 + Math.floor(r() * 14);
    const cy = 1 + Math.floor(r() * 14);
    const size = r() < 0.4 ? 2 : 1;
    for (let dy = -size; dy <= size; dy++) {
      for (let dx = -size; dx <= size; dx++) {
        if (Math.abs(dx) + Math.abs(dy) > size) continue;
        const x = cx + dx;
        const y = cy + dy;
        if (x < 0 || y < 0 || x >= TILE || y >= TILE) continue;
        // Lit upper facet, mid body, shadowed lower-right facet: the brand mark's three faces.
        const c = dy < 0 || (dy === 0 && dx < 0) ? hex(0xf27a63) : dx > 0 || dy > 0 ? hex(0xb8281e) : hex(0xe2483a);
        put(img, ox + x, y, c);
        put(glow, ox + x, y, shade(c, 0.9));
      }
    }
  }
}

export interface Atlas {
  color: HTMLCanvasElement;
  glow: HTMLCanvasElement;
  /** UV rectangle [u0, v0, u1, v1] of a block tile. */
  uv(block: Block): [number, number, number, number];
}

export function buildAtlas(): Atlas {
  const w = TILE * BLOCKS.length;
  const make = () => {
    const c = document.createElement('canvas');
    c.width = w;
    c.height = TILE;
    return c;
  };
  const color = make();
  const glow = make();
  const cctx = color.getContext('2d')!;
  const gctx = glow.getContext('2d')!;
  const img = cctx.createImageData(w, TILE);
  const gimg = gctx.createImageData(w, TILE);
  for (let i = 0; i < gimg.data.length; i += 4) gimg.data[i + 3] = 255;

  BLOCKS.forEach((block, i) => {
    const ox = i * TILE;
    switch (block) {
      case 'slate':
        stone(img, ox, hex(0x5d6472), 11);
        break;
      case 'deepslate':
        stone(img, ox, hex(0x3d424d), 23, 0.1);
        break;
      case 'dolomite':
        stone(img, ox, hex(0xc4bdb2), 31);
        break;
      case 'mossTop':
        moss(img, ox, 41, TILE);
        break;
      case 'mossSide':
        stone(img, ox, hex(0x5d6472), 53);
        moss(img, ox, 59, 1);
        break;
      case 'ore':
        ore(img, gimg, ox, 67);
        break;
    }
  });
  cctx.putImageData(img, 0, 0);
  gctx.putImageData(gimg, 0, 0);

  return {
    color,
    glow,
    uv(block) {
      const i = BLOCKS.indexOf(block);
      // Inset half a texel so nearest sampling never bleeds into the neighbor tile.
      const e = 0.5 / w;
      return [i / BLOCKS.length + e, 0, (i + 1) / BLOCKS.length - e, 1];
    },
  };
}

/**
 * Paints a synthetic 64x64 skin in the standard Bedrock layout: a miner in a cinnabar jacket.
 * Stands in for the player's own skin, which the launcher does not have yet.
 */
export function mockSkin(): HTMLCanvasElement {
  const c = document.createElement('canvas');
  c.width = 64;
  c.height = 64;
  const ctx = c.getContext('2d')!;
  const img = ctx.createImageData(64, 64);
  const r = rng(404);
  const fill = (x: number, y: number, w: number, h: number, base: Rgb, jitter = 0.08) => {
    for (let j = y; j < y + h; j++) for (let i = x; i < x + w; i++) put(img, i, j, shade(base, 1 - jitter / 2 + r() * jitter));
  };
  const box = (u: number, v: number, w: number, h: number, d: number, base: Rgb, jitter?: number) => {
    fill(u + d, v, w, d, shade(base, 1.08), jitter);
    fill(u + d + w, v, w, d, shade(base, 0.8), jitter);
    fill(u, v + d, d + w + d + w, h, base, jitter);
  };

  const skin = hex(0xc98f6b);
  const hair = hex(0x2b1d18);
  const jacket = hex(0xb8322a);
  const trim = hex(0xb9c0c8);
  const trousers = hex(0x2e3440);
  const boots = hex(0x4a3328);

  // Head with hair cap and face.
  box(0, 0, 8, 8, 8, skin, 0.05);
  fill(8, 0, 8, 8, hair);
  fill(0, 8, 32, 3, hair);
  fill(24, 8, 8, 8, hair);
  fill(0, 11, 2, 4, hair);
  fill(22, 11, 2, 4, hair);
  fill(9, 12, 2, 1, hex(0xf2f2f2), 0);
  fill(13, 12, 2, 1, hex(0xf2f2f2), 0);
  fill(10, 12, 1, 1, hex(0x253b5e), 0);
  fill(13, 12, 1, 1, hex(0x253b5e), 0);
  fill(11, 14, 2, 1, hex(0x9a6150), 0);
  fill(10, 15, 4, 1, hex(0x3a2620), 0.1);

  // Hat layer: goggles strapped around the head.
  for (const [x, w] of [[32, 8], [40, 8], [48, 8], [56, 8]] as const) fill(x, 11, w, 2, hex(0x30343a), 0.1);
  fill(41, 10, 2, 3, hex(0x9fd3e0), 0.05);
  fill(45, 10, 2, 3, hex(0x9fd3e0), 0.05);

  // Torso: cinnabar jacket with mercury-silver zip and hem.
  box(16, 16, 8, 12, 4, jacket);
  fill(23, 20, 2, 12, trim, 0.05);
  fill(16, 30, 24, 2, trim, 0.05);
  fill(20, 20, 8, 2, shade(jacket, 0.8));

  // Arms: jacket sleeves ending in gloves.
  box(40, 16, 4, 12, 4, jacket);
  fill(40, 29, 16, 3, hex(0x3b2c26));
  box(32, 48, 4, 12, 4, jacket);
  fill(32, 61, 16, 3, hex(0x3b2c26));

  // Legs: trousers over boots.
  box(0, 16, 4, 12, 4, trousers);
  fill(0, 28, 16, 4, boots);
  box(16, 48, 4, 12, 4, trousers);
  fill(16, 60, 16, 4, boots);

  ctx.putImageData(img, 0, 0);
  return c;
}
