/** Times of day the diorama can be lit for. */
export type Mood = 'dawn' | 'day' | 'dusk' | 'night';

export interface MoodLight {
  skyTop: number;
  horizon: number;
  sunGlow: number;
  /** Direction toward the sun or moon. */
  sun: [number, number, number];
  /** Where its glow sits in the backdrop, in 0..1 screen coordinates. */
  sunScreen: [number, number];
  hemiSky: number;
  hemiGround: number;
  hemi: number;
  key: number;
  keyIntensity: number;
  oreGlow: number;
  oreLight: number;
  exposure: number;
  bloom: number;
  stars: number;
  /** Cloud tint and opacity. */
  cloud: number;
  cloudOpacity: number;
}

export const MOODS: Record<Mood, MoodLight> = {
  dawn: {
    skyTop: 0x2c3a6e, horizon: 0xe9a184, sunGlow: 0xffb48a, sun: [-0.55, 0.4, 0.75], sunScreen: [0.2, 0.42],
    hemiSky: 0xb7c3ff, hemiGround: 0x4a3530, hemi: 0.9, key: 0xffc49a, keyIntensity: 2.4,
    oreGlow: 1.3, oreLight: 3, exposure: 1.05, bloom: 0.45, stars: 0.15, cloud: 0xffc9b4, cloudOpacity: 0.75,
  },
  day: {
    skyTop: 0x3f78c4, horizon: 0xbcd4ea, sunGlow: 0xfff1d6, sun: [0.4, 0.85, 0.5], sunScreen: [0.8, 0.95],
    hemiSky: 0xd8e6ff, hemiGround: 0x51473f, hemi: 1.1, key: 0xfff2e0, keyIntensity: 3,
    oreGlow: 0.9, oreLight: 1.5, exposure: 1, bloom: 0.3, stars: 0, cloud: 0xffffff, cloudOpacity: 0.85,
  },
  dusk: {
    skyTop: 0x1a1d45, horizon: 0xd8705a, sunGlow: 0xff8a5c, sun: [0.75, 0.38, 0.55], sunScreen: [0.86, 0.4],
    hemiSky: 0x8d8fd6, hemiGround: 0x3a2526, hemi: 0.75, key: 0xff9a6c, keyIntensity: 2.2,
    oreGlow: 1.6, oreLight: 5, exposure: 1.05, bloom: 0.6, stars: 0.45, cloud: 0xf2a48c, cloudOpacity: 0.7,
  },
  night: {
    skyTop: 0x060a1a, horizon: 0x1c2747, sunGlow: 0x6f86c9, sun: [-0.5, 0.7, 0.5], sunScreen: [0.25, 0.85],
    hemiSky: 0x5a6aa8, hemiGround: 0x15121a, hemi: 0.55, key: 0xa9bcff, keyIntensity: 0.8,
    oreGlow: 2.1, oreLight: 7, exposure: 1.15, bloom: 0.8, stars: 0.9, cloud: 0x52618f, cloudOpacity: 0.55,
  },
};

/** Picks the mood for a local hour; `?mood=` overrides it in the browser preview. */
export function moodForHour(hour: number): Mood {
  if (hour >= 5 && hour < 8) return 'dawn';
  if (hour >= 8 && hour < 17) return 'day';
  if (hour >= 17 && hour < 20) return 'dusk';
  return 'night';
}
