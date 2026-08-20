// Generates the Grimoire source icon as a PNG, with no image-library dependency.
//
// The mark is a closed codex seen face-on: a warm ink field, a cream board, and
// a darker spine band down the left edge. It reads at 32px and does not rely on
// gradients or texture.
//
// Usage: node scripts/generate-icon.mjs [size] [outfile]

import { deflateSync } from 'node:zlib';
import { writeFileSync, mkdirSync } from 'node:fs';
import { dirname } from 'node:path';

const SIZE = Number(process.argv[2] ?? 1024);
const OUT = process.argv[3] ?? 'src-tauri/icon-source.png';

const INK = [26, 23, 20, 255]; //  deep warm black — the binding
const BOARD = [232, 223, 206, 255]; // aged cream — the board
const SPINE = [138, 106, 74, 255]; //  tanned leather — the spine band
const RULE = [26, 23, 20, 255]; //  the ruled lines

const px = new Uint8Array(SIZE * SIZE * 4);

function blend(x, y, color, coverage) {
  if (coverage <= 0 || x < 0 || y < 0 || x >= SIZE || y >= SIZE) return;
  const a = Math.min(1, coverage);
  const i = (y * SIZE + x) * 4;
  for (let c = 0; c < 3; c++) px[i + c] = Math.round(px[i + c] * (1 - a) + color[c] * a);
  px[i + 3] = Math.round(px[i + 3] * (1 - a) + 255 * a);
}

// Anti-aliased rounded rectangle via signed-distance coverage.
function roundedRect(x0, y0, w, h, r, color) {
  const cx0 = x0 + r;
  const cy0 = y0 + r;
  const cx1 = x0 + w - r;
  const cy1 = y0 + h - r;
  const pad = 2;
  for (let y = Math.floor(y0 - pad); y <= Math.ceil(y0 + h + pad); y++) {
    for (let x = Math.floor(x0 - pad); x <= Math.ceil(x0 + w + pad); x++) {
      const sx = x + 0.5;
      const sy = y + 0.5;
      const dx = Math.max(cx0 - sx, 0, sx - cx1);
      const dy = Math.max(cy0 - sy, 0, sy - cy1);
      const dist = Math.hypot(dx, dy) - r;
      blend(x, y, color, 0.5 - dist);
    }
  }
}

const S = SIZE / 1024; // author at 1024, scale everything

// Field
roundedRect(0, 0, SIZE, SIZE, 224 * S, INK);

// Board
const bx = 236 * S;
const by = 168 * S;
const bw = 552 * S;
const bh = 688 * S;
roundedRect(bx, by, bw, bh, 26 * S, BOARD);

// Spine band down the left edge of the board
roundedRect(bx, by, 104 * S, bh, 26 * S, SPINE);
roundedRect(bx + 78 * S, by, 26 * S, bh, 0, SPINE);

// Ruled lines on the board — uneven lengths so it reads as writing, not a form
const rules = [1, 1, 0.82, 1, 0.9, 0.55];
const rx = bx + 168 * S;
const rw = bw - 224 * S;
rules.forEach((frac, i) => {
  roundedRect(rx, by + (146 + i * 82) * S, rw * frac, 26 * S, 13 * S, RULE);
});

// --- PNG encoding -----------------------------------------------------------

function crc32(buf) {
  let c;
  const table = crc32.table ?? (crc32.table = Array.from({ length: 256 }, (_, n) => {
    c = n;
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    return c >>> 0;
  }));
  let crc = 0xffffffff;
  for (const byte of buf) crc = table[(crc ^ byte) & 0xff] ^ (crc >>> 8);
  return (crc ^ 0xffffffff) >>> 0;
}

function chunk(type, data) {
  const len = Buffer.alloc(4);
  len.writeUInt32BE(data.length);
  const body = Buffer.concat([Buffer.from(type, 'ascii'), data]);
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(body));
  return Buffer.concat([len, body, crc]);
}

// Filter type 0 (None) on every scanline.
const raw = Buffer.alloc(SIZE * (SIZE * 4 + 1));
for (let y = 0; y < SIZE; y++) {
  raw[y * (SIZE * 4 + 1)] = 0;
  Buffer.from(px.buffer, y * SIZE * 4, SIZE * 4).copy(raw, y * (SIZE * 4 + 1) + 1);
}

const ihdr = Buffer.alloc(13);
ihdr.writeUInt32BE(SIZE, 0);
ihdr.writeUInt32BE(SIZE, 4);
ihdr[8] = 8; // bit depth
ihdr[9] = 6; // colour type: RGBA
ihdr[10] = 0;
ihdr[11] = 0;
ihdr[12] = 0;

const png = Buffer.concat([
  Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
  chunk('IHDR', ihdr),
  chunk('IDAT', deflateSync(raw, { level: 9 })),
  chunk('IEND', Buffer.alloc(0))
]);

mkdirSync(dirname(OUT), { recursive: true });
writeFileSync(OUT, png);
console.log(`wrote ${OUT} (${SIZE}x${SIZE}, ${png.length} bytes)`);
