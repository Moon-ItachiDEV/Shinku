// Fabrique l'icône de Shinku pour Windows : src-tauri/icons/icon.ico.
//
// L'icône est le signe de l'interface (le trou noir et son disque, voir
// src/views/art.ts) posé sur une tuile aux couleurs du mode Crépuscule.
// Chaque taille est dessinée pour elle-même, pas réduite depuis une grande
// image : à 16 pixels, les traits sont épaissis et le disque simplifié.
//
// Le script se lance rarement, et son résultat est versionné. Il a besoin de
// Playwright, qui ne fait pas partie des dépendances du projet :
//
//   npm install --no-save playwright && npx playwright install chromium
//   node scripts/icone.mjs
//
// Avec `--apercu <dossier>`, il écrit aussi un PNG par taille pour contrôle.

import { mkdir, writeFile } from 'node:fs/promises';
import { createRequire } from 'node:module';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const output = path.join(here, '..', 'src-tauri', 'icons', 'icon.ico');

/** Les tailles qu'utilise Windows 11, de la barre des tâches aux grandes icônes. */
const SIZES = [16, 20, 24, 32, 40, 48, 64, 128, 256];

/** Le dessin, pour une taille donnée en pixels. */
function drawing(size) {
  // Sous 32 pixels, un trait fin disparaît : on épaissit et on simplifie.
  const small = size < 32;
  const tiny = size < 24;
  const ring = tiny ? 2.3 : small ? 2 : 1.6;
  const disk = tiny ? 1.9 : small ? 1.6 : 1.3;
  const hole = tiny ? 5.6 : 5.3;
  const scale = tiny ? 2.3 : 2.2;
  const radius = tiny ? 15 : 14;
  return `<svg xmlns="http://www.w3.org/2000/svg" width="${size}" height="${size}" viewBox="0 0 64 64">
  <defs>
    <linearGradient id="tile" x1="0" y1="0" x2="1" y2="1">
      <stop offset="0" stop-color="#54487A"/>
      <stop offset="0.5" stop-color="#3A3156"/>
      <stop offset="1" stop-color="#272138"/>
    </linearGradient>
    <radialGradient id="glow" cx="0.62" cy="0.36" r="0.6">
      <stop offset="0" stop-color="#B096E6" stop-opacity="0.34"/>
      <stop offset="1" stop-color="#B096E6" stop-opacity="0"/>
    </radialGradient>
    <linearGradient id="mark" x1="0" y1="0" x2="1" y2="1">
      <stop offset="0" stop-color="#FBE3C8"/>
      <stop offset="0.55" stop-color="#F0A77E"/>
      <stop offset="1" stop-color="#C98BC0"/>
    </linearGradient>
  </defs>
  <rect x="2" y="2" width="60" height="60" rx="${radius}" fill="url(#tile)"/>
  <rect x="2" y="2" width="60" height="60" rx="${radius}" fill="url(#glow)"/>
  ${small ? '' : `<rect x="2.5" y="2.5" width="59" height="59" rx="${radius - 0.5}" fill="none" stroke="#FFFFFF" stroke-opacity="0.12"/>`}
  <g transform="translate(32 32) scale(${scale}) translate(-12 -12.5)">
    <g transform="rotate(-9 12 12.5)">
      <ellipse cx="12" cy="12.5" rx="10.6" ry="${tiny ? 3.1 : 2.7}" fill="none" stroke="url(#mark)" stroke-width="${disk}" stroke-opacity="0.7"/>
    </g>
    <circle cx="12" cy="12.5" r="${hole}" fill="#050407" stroke="url(#mark)" stroke-width="${ring}"/>
    <g transform="rotate(-9 12 12.5)">
      <path d="M1.4 12.5A10.6 ${tiny ? 3.1 : 2.7} 0 0 0 22.6 12.5" fill="none" stroke="url(#mark)" stroke-width="${disk}" stroke-linecap="round"/>
    </g>
  </g>
</svg>`;
}

/** Une image d'icône au format historique : en-tête, pixels BGRA de bas en haut, masque. */
function bitmapEntry(size, rgba) {
  const header = Buffer.alloc(40);
  header.writeUInt32LE(40, 0);
  header.writeInt32LE(size, 4);
  // La hauteur compte double : l'image, puis son masque.
  header.writeInt32LE(size * 2, 8);
  header.writeUInt16LE(1, 12);
  header.writeUInt16LE(32, 14);
  const pixels = Buffer.alloc(size * size * 4);
  for (let y = 0; y < size; y += 1) {
    for (let x = 0; x < size; x += 1) {
      const from = (y * size + x) * 4;
      const to = ((size - 1 - y) * size + x) * 4;
      pixels[to] = rgba[from + 2];
      pixels[to + 1] = rgba[from + 1];
      pixels[to + 2] = rgba[from];
      pixels[to + 3] = rgba[from + 3];
    }
  }
  // Masque à un bit par pixel, lignes alignées sur 32 bits. Tout à zéro : la
  // transparence vient de la couche alpha.
  const mask = Buffer.alloc((Math.ceil(size / 32) * 4) * size);
  return Buffer.concat([header, pixels, mask]);
}

/** Assemble le fichier .ico : un répertoire, puis les images. */
function ico(images) {
  const header = Buffer.alloc(6);
  header.writeUInt16LE(1, 2);
  header.writeUInt16LE(images.length, 4);
  const directory = Buffer.alloc(16 * images.length);
  let offset = header.length + directory.length;
  images.forEach(({ size, data }, index) => {
    const at = index * 16;
    // 0 veut dire 256.
    directory.writeUInt8(size === 256 ? 0 : size, at);
    directory.writeUInt8(size === 256 ? 0 : size, at + 1);
    directory.writeUInt16LE(1, at + 4);
    directory.writeUInt16LE(32, at + 6);
    directory.writeUInt32LE(data.length, at + 8);
    directory.writeUInt32LE(offset, at + 12);
    offset += data.length;
  });
  return Buffer.concat([header, directory, ...images.map((image) => image.data)]);
}

async function main() {
  const previewAt = process.argv.indexOf('--apercu');
  const preview = previewAt === -1 ? null : process.argv[previewAt + 1];
  const { chromium } = createRequire(import.meta.url)('playwright');
  const browser = await chromium.launch();
  try {
    const page = await browser.newPage({ deviceScaleFactor: 1 });
    const images = [];
    for (const size of SIZES) {
      await page.setViewportSize({ width: size, height: size });
      await page.setContent(
        `<style>html,body{margin:0;background:transparent}svg{display:block}</style>${drawing(size)}`,
      );
      const png = await page.screenshot({ omitBackground: true, clip: { x: 0, y: 0, width: size, height: size } });
      if (preview !== null && preview !== undefined) {
        await mkdir(preview, { recursive: true });
        await writeFile(path.join(preview, `${String(size).padStart(3, '0')}.png`), png);
      }
      if (size === 256) {
        // La grande taille reste en PNG, comme Windows le fait lui-même.
        images.push({ size, data: png });
        continue;
      }
      const rgba = await page.evaluate(async (source) => {
        const image = new Image();
        image.src = source;
        await image.decode();
        const canvas = new OffscreenCanvas(image.width, image.height);
        const context = canvas.getContext('2d');
        context.drawImage(image, 0, 0);
        return Array.from(context.getImageData(0, 0, image.width, image.height).data);
      }, `data:image/png;base64,${png.toString('base64')}`);
      images.push({ size, data: bitmapEntry(size, rgba) });
    }
    await mkdir(path.dirname(output), { recursive: true });
    await writeFile(output, ico(images));
    console.log(`${path.relative(process.cwd(), output)} : ${images.length} tailles`);
  } finally {
    await browser.close();
  }
}

await main();
