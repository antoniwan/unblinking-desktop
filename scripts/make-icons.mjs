// Draws the tray and app icons as PNGs with no image dependencies.
// The mark is an almond eye with a vertical slit pupil.
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

// Coverage of the eye mark at (u, v) in [-1, 1]: 1 = almond, 0 = outside or pupil.
function eyeMask(u, v) {
  const edge = 0.52 * Math.pow(Math.max(0, 1 - u * u), 0.75);
  const inAlmond = Math.abs(v) < edge && Math.abs(u) < 0.95;
  const slitW = 0.16 * Math.sqrt(Math.max(0, 1 - (v / 0.62) ** 2));
  const inPupil = Math.abs(u) < slitW;
  return { inAlmond, inPupil };
}

// Supersampled renderer so small icons stay smooth.
function draw(size, shade) {
  const ss = 4;
  return png(size, (x, y) => {
    let acc = [0, 0, 0, 0];
    for (let sy = 0; sy < ss; sy++) {
      for (let sx = 0; sx < ss; sx++) {
        const u = ((x + (sx + 0.5) / ss) / size) * 2 - 1;
        const v = ((y + (sy + 0.5) / ss) / size) * 2 - 1;
        const c = shade(u, v);
        for (let k = 0; k < 4; k++) acc[k] += c[k];
      }
    }
    return acc.map((c) => Math.round(c / (ss * ss)));
  });
}

// macOS template image: black with alpha, the menu bar recolors it.
const template = (u, v) => {
  const { inAlmond, inPupil } = eyeMask(u, v);
  return inAlmond && !inPupil ? [0, 0, 0, 255] : [0, 0, 0, 0];
};

// Windows tray and app icon: fire eye on a dark disc.
const color = (u, v) => {
  const r = Math.hypot(u, v);
  if (r > 0.98) return [0, 0, 0, 0];
  const { inAlmond, inPupil } = eyeMask(u * 1.08, v * 1.08);
  if (inPupil && inAlmond) return [8, 0, 0, 255];
  if (inAlmond) {
    const t = Math.max(0, 1 - Math.hypot(u * 0.9, v * 1.6));
    return [255, Math.round(90 + 150 * t), Math.round(20 + 120 * t * t), 255];
  }
  const glow = Math.max(0, 1 - r) * 0.9;
  return [Math.round(40 + 160 * glow), Math.round(8 + 30 * glow), 4, 255];
};

const files = {
  'trayTemplate.png': draw(16, template),
  'trayTemplate@2x.png': draw(32, template),
  'tray.png': draw(32, color),
  'icon.png': draw(256, color),
};
for (const [name, buf] of Object.entries(files)) writeFileSync(join(outDir, name), buf);
console.log('icons written:', Object.keys(files).join(', '));
