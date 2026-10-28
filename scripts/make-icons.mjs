// Draws the tray and app icons as PNGs with no image dependencies.
// The mark is a round eye, a sphere with a vertical slit pupil.
import { writeFileSync, mkdirSync } from 'node:fs';
import { deflateSync } from 'node:zlib';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const outDir = join(dirname(fileURLToPath(import.meta.url)), '..', 'assets', 'icons');
mkdirSync(outDir, { recursive: true });

const crcTable = Array.from({ length: 256 }, (_, n) => {
  let c = n;
  for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
  return c >>> 0;
});
function crc32(buf) {
  let c = 0xffffffff;
  for (const b of buf) c = crcTable[(c ^ b) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
}
function chunk(type, data) {
  const len = Buffer.alloc(4);
  len.writeUInt32BE(data.length);
  const td = Buffer.concat([Buffer.from(type), data]);
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(td));
  return Buffer.concat([len, td, crc]);
}
function png(size, pixel) {
  const raw = Buffer.alloc(size * (size * 4 + 1));
  for (let y = 0; y < size; y++) {
    raw[y * (size * 4 + 1)] = 0;
    for (let x = 0; x < size; x++) {
      const [r, g, b, a] = pixel(x, y);
      const i = y * (size * 4 + 1) + 1 + x * 4;
      raw[i] = r; raw[i + 1] = g; raw[i + 2] = b; raw[i + 3] = a;
    }
  }
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(size, 0);
  ihdr.writeUInt32BE(size, 4);
  ihdr[8] = 8; ihdr[9] = 6; ihdr[10] = 0; ihdr[11] = 0; ihdr[12] = 0;
  return Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    chunk('IHDR', ihdr),
    chunk('IDAT', deflateSync(raw)),
    chunk('IEND', Buffer.alloc(0)),
  ]);
}

// ---- The mark ----
// A round eye: a sphere with a vertical slit pupil, matching the shader.
// Geometry is in units where the canvas spans [-1, 1]. At 16 px one pixel is
// 0.125 units, so the small marks below are chosen to land on the pixel grid.

// The slit: a lens, widest at the middle, pointed at the ends.
function slitDist(u, v, halfW, halfH, taper = 0.7) {
  const t = Math.min(1, Math.abs(v) / halfH);
  const w = halfW * Math.pow(Math.max(0, 1 - t * t), taper);
  return Math.abs(v) >= halfH ? 1 : Math.abs(u) - w; // < 0 inside
}

// Small value noise for the app icon's fire.
function hash(x, y) {
  const s = Math.sin(x * 127.1 + y * 311.7) * 43758.5453;
  return s - Math.floor(s);
}
function noise(x, y) {
  const ix = Math.floor(x), iy = Math.floor(y);
  const fx = x - ix, fy = y - iy;
  const ux = fx * fx * (3 - 2 * fx), uy = fy * fy * (3 - 2 * fy);
  const a = hash(ix, iy), b = hash(ix + 1, iy), c = hash(ix, iy + 1), d = hash(ix + 1, iy + 1);
  return a + (b - a) * ux + (c - a) * uy + (a - b - c + d) * ux * uy;
}
function fbm(x, y) {
  let v = 0, a = 0.5;
  for (let i = 0; i < 5; i++) {
    v += a * noise(x, y);
    x = x * 2.03 + 1.7; y = y * 2.03 + 9.2;
    a *= 0.5;
  }
  return v;
}
// Black body ramp, the same one the shader uses.
function fire(t) {
  t = Math.max(0, t);
  return [Math.min(1, t * 1.7), Math.min(1, t * t * 1.25), Math.min(1, Math.pow(t, 4) * 0.9)];
}

// Supersampled renderer. shade(u, v) returns straight RGBA in 0..1.
// Colors are accumulated premultiplied so edges do not fringe.
function draw(size, shade, ss = 6) {
  return png(size, (x, y) => {
    let r = 0, g = 0, b = 0, a = 0;
    for (let sy = 0; sy < ss; sy++) {
      for (let sx = 0; sx < ss; sx++) {
        const u = ((x + (sx + 0.5) / ss) / size) * 2 - 1;
        const v = ((y + (sy + 0.5) / ss) / size) * 2 - 1;
        const c = shade(u, v);
        r += c[0] * c[3]; g += c[1] * c[3]; b += c[2] * c[3]; a += c[3];
      }
    }
    if (a === 0) return [0, 0, 0, 0];
    const to8 = (k) => Math.max(0, Math.min(255, Math.round(k * 255)));
    return [to8(r / a), to8(g / a), to8(b / a), to8(a / (ss * ss))];
  });
}

// macOS template image: pure black, the shape lives in alpha only; the
// menu bar recolors it. A solid disc with the slit cut through it.
const template = (u, v) => {
  const r = Math.hypot(u, v);
  const disc = r <= 0.875;                 // 14 px across at 16 px
  // 2 px wide at 16 px, straight-sided so it stays on the pixel grid,
  // pointed only at the very ends.
  const slit = slitDist(u, v, 0.125, 0.75, 0.2) < 0;
  return [0, 0, 0, disc && !slit ? 1 : 0];
};

// Windows tray (32 px, shown at 16): a fire sphere with a dark outline so it
// holds up on light and dark taskbars, and a bold slit.
const tray = (u, v) => {
  const r = Math.hypot(u, v);
  if (r > 0.94) return [0, 0, 0, 0];
  if (r > 0.81) return [0.16, 0.02, 0.0, 1];                       // outline
  if (slitDist(u, v, 0.16, 0.7, 0.35) < 0) return [0.04, 0.0, 0.0, 1];   // pupil
  const rn = r / 0.81;
  const z = Math.sqrt(Math.max(0, 1 - rn * rn));
  const t = 0.42 + 0.55 * z - 0.25 * Math.abs(u) + 0.15 * Math.max(0, -v);
  return [...fire(t), 1];
};

// App icon: the sphere of fire, its glow, and a few rising embers.
const R = 0.66;
const app = (u, v) => {
  const x = u, y = -v; // +y up
  const rr = Math.hypot(x, y);
  const rn = rr / R;
  if (rn <= 1) {
    const z = Math.sqrt(Math.max(0, 1 - rn * rn));
    // Gaze straight out, a touch up and to the left for life.
    const nx = x / R, ny = y / R;
    const ax = -0.12, ay = 0.08;
    const lx = nx - ax * z, ly = ny - ay * z, lz = z + ax * nx + ay * ny;
    const s = Math.hypot(lx, ly);
    const ang = Math.atan2(s, lz) / Math.PI;
    const rad = s > 1e-4 ? [lx / s, ly / s] : [0, 1];
    const swirl = fbm(lx * 3.2 + 4.0, ly * 3.2 + 1.0);
    const fibers = noise(rad[0] * 6 + 3.3, rad[1] * 6 + 3.3) * 0.75 + noise(rad[0] * 13 + 1.1, rad[1] * 13 + 1.1) * 0.25;
    let it = 0.8 - ang * 1.05 + (swirl - 0.5) * 1.5 + (fibers - 0.5) * 0.4 + 0.08;
    it = 1.06 * (1 - Math.exp(-Math.max(0, it) * 1.9));
    it *= 0.55 + 0.45 * Math.min(1, Math.max(0, (swirl - 0.28) / 0.34));
    it += Math.exp(-ang * 7) * 0.26;
    let c = fire(it);
    // Slit, with a hot rim.
    const pd = slitDist(lx, ly, 0.13, 0.78);
    const ring = Math.exp(-Math.max(pd, 0) * 22) * Math.max(0, Math.min(1, (0.8 - Math.abs(ly)) / 0.2));
    c = c.map((k, i) => k + [1, 0.8, 0.45][i] * ring * 0.55);
    if (pd < 0) c = [0.03, 0.0, 0.0];
    // Limb darkening and fresnel rim.
    const limb = 0.38 + 0.62 * Math.sqrt(z);
    const fres = Math.pow(1 - z, 4);
    c = c.map((k, i) => k * limb + [1, 0.45, 0.1][i] * fres * 0.6);
    return [...c.map((k) => Math.min(1, k)), 1];
  }
  // Outside: glow, plus embers above.
  const d = rr - R;
  const glow = Math.exp(-d * 9) * 0.75 + Math.exp(-d * 3.5) * 0.25;
  let col = [1, 0.32 + 0.25 * Math.exp(-d * 12), 0.06];
  let a = Math.min(1, glow) * Math.max(0, 1 - Math.pow(rr / 1.0, 6));
  for (const [ex, ey, er] of EMBERS) {
    const k = Math.exp(-(((x - ex) ** 2) + ((y - ey) ** 2) * 0.5) / (er * er));
    if (k > 0.01) {
      col = col.map((cc, i) => cc * (1 - k) + [1, 0.78, 0.35][i] * k);
      a = Math.max(a, Math.min(1, k * 1.2));
    }
  }
  return [...col, a];
};
const EMBERS = [
  [-0.32, 0.76, 0.022], [0.12, 0.82, 0.018], [0.4, 0.72, 0.024],
  [-0.08, 0.9, 0.014], [0.26, 0.9, 0.012], [-0.5, 0.62, 0.016],
];

const files = {
  'trayTemplate.png': draw(16, template, 8),
  'trayTemplate@2x.png': draw(32, template, 8),
  'tray.png': draw(32, tray, 8),
  'icon.png': draw(512, app, 3),
};
for (const [name, buf] of Object.entries(files)) writeFileSync(join(outDir, name), buf);
console.log('icons written:', Object.keys(files).join(', '));
