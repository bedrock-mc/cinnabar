import { BufferAttribute, BufferGeometry } from 'three';
import type { Atlas, Block } from './textures';

/** A floating outcrop of slate and dolomite crossed by a cinnabar vein. */
export interface World {
  size: number;
  get(x: number, y: number, z: number): Block | undefined;
  /** Top surface height per column, for placing the player and props. */
  top(x: number, z: number): number;
  /** Ore voxels in the order the vein refines, back-left to front-right. */
  ore: [number, number, number][];
}

/** Shape of a generated island. */
export interface IslandShape {
  seed: number;
  size: number;
  radius: number;
  /** Height of the terraced hill above the base. */
  hill: number;
  /** Depth of the hanging underside at the centre. */
  keel: number;
  vein: boolean;
}

export const MAIN_ISLAND: IslandShape = { seed: 2026, size: 16, radius: 6.6, hill: 5.2, keel: 5.5, vein: true };

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

/** Smooth 2D value noise in 0..1 so materials form patches instead of confetti. */
function valueNoise(seed: number) {
  const hash = (x: number, y: number) => {
    let h = Math.imul(x, 374761393) ^ Math.imul(y, 668265263) ^ seed;
    h = Math.imul(h ^ (h >>> 13), 1274126177);
    return ((h ^ (h >>> 16)) >>> 0) / 4294967296;
  };
  const smooth = (t: number) => t * t * (3 - 2 * t);
  return (x: number, y: number) => {
    const xi = Math.floor(x);
    const yi = Math.floor(y);
    const fx = smooth(x - xi);
    const fy = smooth(y - yi);
    const a = hash(xi, yi) + (hash(xi + 1, yi) - hash(xi, yi)) * fx;
    const b = hash(xi, yi + 1) + (hash(xi + 1, yi + 1) - hash(xi, yi + 1)) * fx;
    return a + (b - a) * fy;
  };
}

export function buildWorld(shape: IslandShape = MAIN_ISLAND): World {
  const { seed, size } = shape;
  const r = rng(seed);
  const c = (size - 1) / 2;
  const cells = new Map<string, Block>();
  const key = (x: number, y: number, z: number) => `${x},${y},${z}`;
  const tops: number[][] = [];
  const noise = valueNoise(seed);

  // Outline: a lumpy disc so the island never reads as a square tile.
  const lobes = Array.from({ length: 5 }, () => ({ a: r() * Math.PI * 2, k: 0.7 + r() * 0.6 }));
  const radius = (x: number, z: number) => {
    const ang = Math.atan2(z - c, x - c);
    let rad = shape.radius;
    for (const l of lobes) rad += Math.cos(ang - l.a) * 0.25 * l.k;
    return rad;
  };

  for (let x = 0; x < size; x++) {
    tops[x] = [];
    for (let z = 0; z < size; z++) {
      const d = Math.hypot(x - c, z - c);
      const rad = radius(x, z);
      if (d > rad) {
        tops[x]![z] = -1;
        continue;
      }
      // Terraced hill rising toward the back-left; stepped so each terrace reads as a ledge.
      const hill = Math.max(0, 1 - Math.hypot(x - c + shape.radius * 0.4, z - c + shape.radius * 0.42) / (shape.radius * 1.12));
      const h = Math.floor(1 + hill * shape.hill);
      // Underside tapers into a hanging point.
      const depth = Math.floor(1 + Math.pow(1 - d / rad, 1.2) * shape.keel + r() * 1.2);
      tops[x]![z] = h;
      // Moss gathers in patches on the lower terraces; the summit stays bare rock.
      const mossy = noise(x * 0.35, z * 0.35) > 0.42 && h < 6;
      for (let y = -depth; y <= h; y++) {
        // Pale dolomite strata run through the slate, warped gently so the bands read as geology.
        const band = (y + noise(x * 0.2 + 9, z * 0.2) * 1.6) % 3.4;
        let block: Block = y < -3 ? 'deepslate' : band > 0 && band < 1.1 ? 'dolomite' : 'slate';
        if (y === h && mossy) block = 'mossTop';
        cells.set(key(x, y, z), block);
      }
    }
  }

  // The vein: a slanted sheet through the hill, exposed on its terraces and front cliff.
  const ore: [number, number, number][] = [];
  for (const [k, block] of cells) {
    if (!shape.vein || block === 'deepslate') continue;
    const [x, y, z] = k.split(',').map(Number) as [number, number, number];
    const sheet = x * 0.62 - z * 0.48 + y * 0.55 - 1.0 + Math.sin(z * 0.8) * 0.7;
    if (Math.abs(sheet) < 0.62 && y > -4) {
      cells.set(k, 'ore');
      ore.push([x, y, z]);
    }
  }
  ore.sort((a, b) => a[0] + a[2] - (b[0] + b[2]) || b[1] - a[1]);

  return {
    size,
    get: (x, y, z) => cells.get(key(x, y, z)),
    top: (x, z) => tops[x]?.[z] ?? -1,
    ore,
  };
}

/** A small floating chunk of slate with a dolomite cap, for depth around the island. */
export function buildRock(seed: number): World {
  const r = rng(seed);
  const size = 4;
  const cells = new Map<string, Block>();
  const key = (x: number, y: number, z: number) => `${x},${y},${z}`;
  for (let x = 0; x < size; x++) {
    for (let z = 0; z < size; z++) {
      const d = Math.hypot(x - 1.5, z - 1.5);
      if (d > 1.9 + r() * 0.4) continue;
      const depth = Math.floor(1 + (2 - d) * 1.4 + r());
      for (let y = -depth; y <= 0; y++) cells.set(key(x, y, z), y === 0 ? (r() < 0.5 ? 'mossTop' : 'dolomite') : 'slate');
    }
  }
  return { size, get: (x, y, z) => cells.get(key(x, y, z)), top: () => 0, ore: [] };
}

// Face definitions: normal, the four corners (counter-clockwise from outside), and the two
// in-plane axes used to look up ambient-occlusion neighbors.
const FACES = [
  { n: [1, 0, 0], corners: [[1, 0, 1], [1, 0, 0], [1, 1, 0], [1, 1, 1]], light: 0.86 },
  { n: [-1, 0, 0], corners: [[0, 0, 0], [0, 0, 1], [0, 1, 1], [0, 1, 0]], light: 0.86 },
  { n: [0, 1, 0], corners: [[0, 1, 1], [1, 1, 1], [1, 1, 0], [0, 1, 0]], light: 1 },
  { n: [0, -1, 0], corners: [[0, 0, 0], [1, 0, 0], [1, 0, 1], [0, 0, 1]], light: 0.6 },
  { n: [0, 0, 1], corners: [[0, 0, 1], [1, 0, 1], [1, 1, 1], [0, 1, 1]], light: 0.93 },
  { n: [0, 0, -1], corners: [[1, 0, 0], [0, 0, 0], [0, 1, 0], [1, 1, 0]], light: 0.93 },
] as const;

/**
 * Merges every exposed face into one geometry with baked ambient occlusion in vertex colors and
 * an `orePos` attribute (the voxel's index in the vein, or -1) that the shader turns into glow.
 */
export function buildMesh(world: World, atlas: Atlas): BufferGeometry {
  const pos: number[] = [];
  const nor: number[] = [];
  const uv: number[] = [];
  const col: number[] = [];
  const orePos: number[] = [];
  const index: number[] = [];
  const solid = (x: number, y: number, z: number) => world.get(x, y, z) !== undefined;
  const oreIndex = new Map(world.ore.map(([x, y, z], i) => [`${x},${y},${z}`, i]));
  const half = world.size / 2;

  for (let x = 0; x < world.size; x++) {
    for (let z = 0; z < world.size; z++) {
      for (let y = -12; y <= 12; y++) {
        const block = world.get(x, y, z);
        if (!block) continue;
        for (const face of FACES) {
          const [nx, ny, nz] = face.n;
          if (solid(x + nx, y + ny, z + nz)) continue;
          // Grass-like tops only on the upward face; their sides use the fringe tile.
          let tile: Block = block;
          if (block === 'mossTop' && ny !== 1) tile = ny === -1 ? 'slate' : 'mossSide';
          const [u0, v0, u1, v1] = atlas.uv(tile);
          const faceUv = [[u0, v0], [u1, v0], [u1, v1], [u0, v1]];
          const base = pos.length / 3;
          const ao: number[] = [];
          face.corners.forEach((corner, i) => {
            const [cx, cy, cz] = corner;
            pos.push(x + cx - half, y + cy, z + cz - half);
            nor.push(nx, ny, nz);
            uv.push(faceUv[i]![0]!, faceUv[i]![1]!);
            // Classic voxel AO: the two edge neighbors and the diagonal in the layer this face looks into.
            const p = [x + nx, y + ny, z + nz];
            const step = [cx === 0 ? -1 : 1, cy === 0 ? -1 : 1, cz === 0 ? -1 : 1];
            const [t1, t2] = [0, 1, 2].filter((k) => face.n[k] === 0) as [number, number];
            const at = (k1: boolean, k2: boolean) => {
              const q = [...p];
              if (k1) q[t1]! += step[t1]!;
              if (k2) q[t2]! += step[t2]!;
              return solid(q[0]!, q[1]!, q[2]!);
            };
            const s1 = at(true, false);
            const s2 = at(false, true);
            const occ = s1 && s2 ? 3 : Number(s1) + Number(s2) + Number(at(true, true));
            const v = face.light * (1 - occ * 0.17);
            ao.push(v);
            col.push(v, v, v);
            orePos.push(block === 'ore' ? oreIndex.get(`${x},${y},${z}`)! : -1);
          });
          // Flip the quad's diagonal when it would otherwise crease the AO gradient.
          if (ao[0]! + ao[2]! > ao[1]! + ao[3]!) index.push(base, base + 1, base + 2, base, base + 2, base + 3);
          else index.push(base + 1, base + 2, base + 3, base + 1, base + 3, base);
        }
      }
    }
  }

  const g = new BufferGeometry();
  g.setAttribute('position', new BufferAttribute(new Float32Array(pos), 3));
  g.setAttribute('normal', new BufferAttribute(new Float32Array(nor), 3));
  g.setAttribute('uv', new BufferAttribute(new Float32Array(uv), 2));
  g.setAttribute('color', new BufferAttribute(new Float32Array(col), 3));
  g.setAttribute('orePos', new BufferAttribute(new Float32Array(orePos), 1));
  g.setIndex(index);
  g.computeBoundingSphere();
  return g;
}
