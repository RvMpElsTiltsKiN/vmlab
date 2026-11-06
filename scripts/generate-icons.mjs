#!/usr/bin/env node
/**
 * Pipeline de iconos de VMLab.
 *
 *   npm run icons                          # usa assets/icon-source.svg
 *   npm run icons -- ruta/logo.png         # PNG o SVG propio (ideal: cuadrado, ≥ 1024 px)
 *   npm run icons -- logo.svg --padding 8  # margen transparente del 8 % alrededor
 *   npm run icons -- logo.svg --out dist-icons
 *
 * Genera en `src-tauri/icons/` todo lo que referencia `tauri.conf.json`:
 *   32x32.png · 128x128.png · 128x128@2x.png · icon.png · icon.ico (Windows) · icon.icns (macOS)
 *
 * Solo depende de `sharp`: los contenedores .ico e .icns se escriben aquí mismo
 * (ambos admiten PNG incrustado), así no hay herramientas externas que instalar.
 */
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import path from 'node:path';
import process from 'node:process';
import { fileURLToPath } from 'node:url';
import sharp from 'sharp';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

// ───────────── Argumentos ─────────────
function parseArgs(argv) {
  const opts = { input: path.join(root, 'assets', 'icon-source.svg'), out: path.join(root, 'src-tauri', 'icons'), padding: 0 };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === '--out') opts.out = path.resolve(argv[++i] ?? '');
    else if (a === '--padding') opts.padding = Number(argv[++i]);
    else if (a === '-h' || a === '--help') { console.log('Uso: npm run icons -- [imagen.png|svg] [--padding 0-40] [--out carpeta]'); process.exit(0); }
    else if (a.startsWith('--')) throw new Error(`Opción desconocida: ${a}`);
    else opts.input = path.resolve(a);
  }
  if (!Number.isFinite(opts.padding) || opts.padding < 0 || opts.padding > 40) throw new Error('--padding debe estar entre 0 y 40 (%)');
  return opts;
}

// ───────────── Imagen maestra 1024×1024 ─────────────
async function renderMaster(file, paddingPct) {
  const buf = await readFile(file);
  const isSvg = /\.svg$/i.test(file);
  const meta = await sharp(buf).metadata();
  const longSide = Math.max(meta.width ?? 1, meta.height ?? 1);
  if (!isSvg && longSide < 512) console.warn(`⚠ La imagen mide ${longSide}px; se recomienda ≥ 1024px para que no se vea borrosa.`);
  if (meta.width && meta.height && meta.width !== meta.height) console.warn('⚠ La imagen no es cuadrada: se centrará sobre un lienzo cuadrado transparente.');

  const SIZE = 1024;
  const inner = Math.round(SIZE * (1 - (paddingPct * 2) / 100));
  // Para SVG, la densidad hace que se rasterice nítido al tamaño pedido.
  const density = isSvg ? Math.max(72, Math.ceil((72 * inner) / longSide)) : undefined;
  const glyph = await sharp(buf, density ? { density } : {})
    .resize(inner, inner, { fit: 'contain', background: { r: 0, g: 0, b: 0, alpha: 0 } })
    .png()
    .toBuffer();

  return sharp({ create: { width: SIZE, height: SIZE, channels: 4, background: { r: 0, g: 0, b: 0, alpha: 0 } } })
    .composite([{ input: glyph, gravity: 'centre' }])
    .png()
    .toBuffer();
}

// ───────────── Contenedores ─────────────
/** ICO con entradas PNG (Windows Vista+; es lo que usa también `tauri icon`). */
function buildIco(entries) {
  const header = Buffer.alloc(6);
  header.writeUInt16LE(1, 2); // tipo: icono
  header.writeUInt16LE(entries.length, 4);
  const dir = Buffer.alloc(16 * entries.length);
  let offset = header.length + dir.length;
  entries.forEach(({ size, png }, i) => {
    const o = i * 16;
    dir[o] = size >= 256 ? 0 : size; // 0 significa 256
    dir[o + 1] = size >= 256 ? 0 : size;
    dir.writeUInt16LE(1, o + 4); // planos
    dir.writeUInt16LE(32, o + 6); // bits por píxel
    dir.writeUInt32LE(png.length, o + 8);
    dir.writeUInt32LE(offset, o + 12);
    offset += png.length;
  });
  return Buffer.concat([header, dir, ...entries.map((e) => e.png)]);
}

/** ICNS con entradas PNG. Tipos: icp4-6 (16/32/64), ic07-ic10 (128…1024), ic11-ic14 (@2x). */
const ICNS_TYPES = [
  ['icp4', 16], ['icp5', 32], ['icp6', 64],
  ['ic07', 128], ['ic08', 256], ['ic09', 512], ['ic10', 1024],
  ['ic11', 32], ['ic12', 64], ['ic13', 256], ['ic14', 512],
];
function buildIcns(pngBySize) {
  const chunks = ICNS_TYPES.map(([type, size]) => {
    const png = pngBySize.get(size);
    const head = Buffer.alloc(8);
    head.write(type, 0, 'ascii');
    head.writeUInt32BE(8 + png.length, 4);
    return Buffer.concat([head, png]);
  });
  const total = 8 + chunks.reduce((n, c) => n + c.length, 0);
  const head = Buffer.alloc(8);
  head.write('icns', 0, 'ascii');
  head.writeUInt32BE(total, 4);
  return Buffer.concat([head, ...chunks]);
}

// ───────────── Main ─────────────
async function main() {
  const opts = parseArgs(process.argv.slice(2));
  console.log(`Origen : ${path.relative(root, opts.input) || opts.input}`);
  console.log(`Destino: ${path.relative(root, opts.out) || opts.out}`);

  const master = await renderMaster(opts.input, opts.padding);
  const sizes = [...new Set([...ICNS_TYPES.map(([, s]) => s), 24, 48, 128, 256, 512])].sort((a, b) => a - b);
  const png = new Map();
  for (const s of sizes) {
    png.set(s, await sharp(master).resize(s, s, { kernel: 'lanczos3' }).png({ compressionLevel: 9 }).toBuffer());
  }

  await mkdir(opts.out, { recursive: true });
  const files = {
    '32x32.png': png.get(32),
    '128x128.png': png.get(128),
    '128x128@2x.png': png.get(256),
    'icon.png': png.get(512),
    'icon.ico': buildIco([16, 24, 32, 48, 64, 128, 256].map((size) => ({ size, png: png.get(size) }))),
    'icon.icns': buildIcns(png),
  };
  for (const [name, data] of Object.entries(files)) {
    await writeFile(path.join(opts.out, name), data);
    console.log(`  ✔ ${name.padEnd(16)} ${(data.length / 1024).toFixed(1).padStart(7)} KB`);
  }
  console.log('Listo. Ahora puedes ejecutar: npm run tauri build');
}

main().catch((e) => {
  console.error(`✖ ${e.message}`);
  process.exit(1);
});
