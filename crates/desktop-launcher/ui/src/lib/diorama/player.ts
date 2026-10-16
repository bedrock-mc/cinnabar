import { BoxGeometry, CanvasTexture, Group, Mesh, MeshStandardMaterial, NearestFilter, SRGBColorSpace, type Material } from 'three';

const PX = 1 / 16;

/** Sets a box's six faces to skin rectangles laid out from (u, v), as the skin format unfolds them. */
function skinBox(w: number, h: number, d: number, u: number, v: number, material: Material, inflate = 0): Mesh {
  const g = new BoxGeometry((w + inflate) * PX, (h + inflate) * PX, (d + inflate) * PX);
  const rect = (x: number, y: number, rw: number, rh: number) => [x / 64, 1 - (y + rh) / 64, (x + rw) / 64, 1 - y / 64] as const;
  // BoxGeometry face order: +x, -x, +y, -y, +z, -z. The model faces +z, so its right side is -x.
  const faces = [
    rect(u + d + w, v + d, d, h), // left side
    rect(u, v + d, d, h), // right side
    rect(u + d, v, w, d), // top
    rect(u + d + w, v, w, d), // bottom
    rect(u + d, v + d, w, h), // front
    rect(u + d + w + d, v + d, w, h), // back
  ];
  const uv = g.getAttribute('uv');
  faces.forEach(([u0, v0, u1, v1], f) => {
    const bottom = f === 3;
    // Each face's vertices run top-left, top-right, bottom-left, bottom-right.
    uv.setXY(f * 4, u0, bottom ? v0 : v1);
    uv.setXY(f * 4 + 1, u1, bottom ? v0 : v1);
    uv.setXY(f * 4 + 2, u0, bottom ? v1 : v0);
    uv.setXY(f * 4 + 3, u1, bottom ? v1 : v0);
  });
  uv.needsUpdate = true;
  const mesh = new Mesh(g, material);
  mesh.castShadow = true;
  mesh.receiveShadow = true;
  return mesh;
}

export interface Player {
  root: Group;
  /** Advances the idle pose; `look` is -1..1 toward the pointer. */
  update(t: number, look: { x: number; y: number }): void;
}

/** Builds a classic-proportion player from a 64x64 skin canvas. Feet sit at y = 0. */
export function buildPlayer(skin: HTMLCanvasElement): Player {
  const tex = new CanvasTexture(skin);
  tex.magFilter = NearestFilter;
  tex.minFilter = NearestFilter;
  tex.colorSpace = SRGBColorSpace;
  tex.generateMipmaps = false;
  const body = new MeshStandardMaterial({ map: tex, roughness: 0.85 });
  const layer = new MeshStandardMaterial({ map: tex, roughness: 0.6, transparent: true, alphaTest: 0.5 });

  const root = new Group();
  const torso = skinBox(8, 12, 4, 16, 16, body);
  torso.position.y = 18 * PX;
  root.add(torso);

  const head = new Group();
  head.position.y = 24 * PX;
  const skull = skinBox(8, 8, 8, 0, 0, body);
  skull.position.y = 4 * PX;
  const hat = skinBox(8, 8, 8, 32, 0, layer, 1);
  hat.position.y = 4 * PX;
  head.add(skull, hat);
  root.add(head);

  const limb = (u: number, v: number, x: number, y: number) => {
    const pivot = new Group();
    pivot.position.set(x * PX, y * PX, 0);
    const m = skinBox(4, 12, 4, u, v, body);
    m.position.y = -6 * PX;
    pivot.add(m);
    root.add(pivot);
    return pivot;
  };
  const rightArm = limb(40, 16, -6, 24);
  const leftArm = limb(32, 48, 6, 24);
  limb(0, 16, -2, 12);
  limb(16, 48, 2, 12);

  return {
    root,
    update(t, look) {
      const breathe = Math.sin(t * 1.6);
      rightArm.rotation.z = 0.04 + breathe * 0.025;
      leftArm.rotation.z = -0.04 - breathe * 0.025;
      rightArm.rotation.x = Math.sin(t * 0.8) * 0.04;
      leftArm.rotation.x = -Math.sin(t * 0.8) * 0.04;
      // The head turns toward the pointer, damped so it eases rather than snaps.
      head.rotation.y += (look.x * 0.55 - head.rotation.y) * 0.08;
      head.rotation.x += (-look.y * 0.25 - head.rotation.x) * 0.08;
      torso.position.y = 18 * PX + breathe * 0.004;
    },
  };
}
