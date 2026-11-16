// Fabrique les images de l'installeur de Shinku : src-tauri/installeur/images/.
//
// Deux pièces, chacune en cinq tailles (100, 125, 150, 175 et 200 % de
// l'échelle de Windows) :
//   - bandeau-<échelle>.bmp : l'image de gauche des pages Bienvenue et
//     Terminé, 191 × 362 pixels à 100 % : le trou noir de l'Accueil (le
//     dessin du signe est celui de src/views/art.ts) et le logotype
//     « SHINKU 真空 », sur le fond Brume F4F1EE ;
//   - signe-<échelle>.bmp : le petit trou noir du bandeau d'en-tête des autres
//     pages, 76 × 58 pixels à 100 %, au bord droit.
//
// Le dessin doit suivre src/views/art.ts : si le signe ou le logotype de
// l'interface changent, il faut les reporter ici et relancer le script.
//
// L'installeur affiche ces images à leur taille réelle, sans les étirer
// (voir installeur/habillage.nsh) : il choisit la taille la plus proche de
// l'échelle de l'écran, et le trait de l'anneau reste fin à 150 %. Comme le
// fond des images est celui de la page, aucune couture ne se voit : le test
// tests/installer.test.ts vérifie que le coin de chaque image a exactement
// cette couleur.
//
// Le script se lance rarement, et son résultat est versionné. Il a besoin de
// Playwright, qui ne fait pas partie des dépendances du projet :
//
//   npm install --no-save playwright && npx playwright install chromium
//   node scripts/images-installeur.mjs
//
// Avec `--apercu <dossier>`, il écrit aussi un PNG par image pour contrôle.
// Il écrit lui-même les BMP (24 bits, sans dépendance) : un petit encodeur
// suffit.

import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { createRequire } from 'node:module';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const modules = path.join(here, '..', 'node_modules');
const output = path.join(here, '..', 'src-tauri', 'installeur', 'images');

/** Le fond des pages Bienvenue et Terminé et du bandeau (MUI_BGCOLOR). */
const BACKGROUND = [0xf4, 0xf1, 0xee];

/** Les échelles de Windows prises en charge, en pour cent. */
const SCALES = [100, 125, 150, 175, 200];

/** Les deux pièces et leur taille à 100 %, en pixels. */
const PIECES = [
  { name: 'bandeau', element: '#sidebar', width: 191, height: 362 },
  { name: 'signe', element: '#header', width: 76, height: 58 },
];

/** Une police de la pile de l'interface, lue dans node_modules. */
async function fontFace(family, weight, file, format, range) {
  const data = await readFile(path.join(modules, file));
  const unicode = range === undefined ? '' : `unicode-range:${range};`;
  return `@font-face{font-family:'${family}';font-weight:${weight};${unicode}src:url(data:font/woff2;base64,${data.toString('base64')}) format('${format}')}`;
}

/** La page de dessin : les polices, puis les deux pièces. */
async function page() {
  const mplus = '@fontsource-variable/m-plus-1/files/m-plus-1-';
  const fonts = [
    await fontFace('M PLUS 1', '100 900', `${mplus}latin-wght-normal.woff2`, 'woff2-variations', 'U+0000-00FF'),
    await fontFace('M PLUS 1', '100 900', `${mplus}112-wght-normal.woff2`, 'woff2-variations', 'U+7A7A'),
    await fontFace('M PLUS 1', '100 900', `${mplus}115-wght-normal.woff2`, 'woff2-variations', 'U+771F'),
    await fontFace('Orbitron', '600', '@fontsource/orbitron/files/orbitron-latin-600-normal.woff2', 'woff2'),
  ];
  return `<!doctype html><html><head><meta charset="utf-8"><style>
${fonts.join('\n')}
html,body{margin:0;background:#F4F1EE}
#sidebar{position:relative;width:191px;height:362px;overflow:hidden;background:#F4F1EE;background-image:radial-gradient(circle 96px at 95.5px 146px,rgba(157,147,232,0.26),rgba(157,147,232,0) 100%),radial-gradient(ellipse 96px 110px at 95.5px 362px,rgba(240,167,126,0.20),rgba(240,167,126,0) 100%)}
#sidebar svg{position:absolute;left:11.5px;top:58px}
.lname{position:absolute;left:0;right:0;top:262px;display:flex;justify-content:center;align-items:center;gap:9px;padding-left:5px}
.wm{font-family:'Orbitron';font-size:12px;font-weight:600;letter-spacing:0.42em;color:#1F1A26}
.kj{font-family:'M PLUS 1';font-size:11px;font-weight:400;letter-spacing:0.22em;color:#B8693C}
#header{position:relative;width:76px;height:58px;background:#F4F1EE}
#header svg{position:absolute;right:18px;top:11px}
</style></head><body>
<div id="sidebar">
<svg viewBox="0 0 200 200" width="168" height="168">
<defs>
<radialGradient id="h-halo" cx="0.5" cy="0.5" r="0.5"><stop offset="0.22" stop-color="#F0A77E" stop-opacity="0.22"/><stop offset="0.6" stop-color="#9D93E8" stop-opacity="0.08"/><stop offset="1" stop-color="#9D93E8" stop-opacity="0"/></radialGradient>
<linearGradient id="h-disk" gradientUnits="userSpaceOnUse" x1="14" y1="0" x2="186" y2="0"><stop offset="0" stop-color="#9D93E8" stop-opacity="0"/><stop offset="0.2" stop-color="#C98BC0" stop-opacity="0.8"/><stop offset="0.48" stop-color="#FBE3C8"/><stop offset="0.8" stop-color="#F0A77E" stop-opacity="0.85"/><stop offset="1" stop-color="#9D93E8" stop-opacity="0"/></linearGradient>
<linearGradient id="h-lens" gradientUnits="userSpaceOnUse" x1="62" y1="0" x2="138" y2="0"><stop offset="0" stop-color="#C98BC0" stop-opacity="0"/><stop offset="0.2" stop-color="#C98BC0" stop-opacity="0.8"/><stop offset="0.5" stop-color="#FBE3C8"/><stop offset="0.82" stop-color="#F0A77E" stop-opacity="0.8"/><stop offset="1" stop-color="#F0A77E" stop-opacity="0"/></linearGradient>
<linearGradient id="h-ring" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="#FBE3C8"/><stop offset="0.55" stop-color="#F0A77E"/><stop offset="1" stop-color="#C98BC0"/></linearGradient>
<filter id="h-soft" x="-40%" y="-60%" width="180%" height="220%"><feGaussianBlur stdDeviation="2.4"/></filter>
<clipPath id="h-top"><rect x="0" y="0" width="200" height="104"/></clipPath>
<clipPath id="h-bot"><rect x="0" y="104" width="200" height="96"/></clipPath>
</defs>
<circle cx="100" cy="104" r="96" fill="url(#h-halo)"/>
<g transform="rotate(-9 100 104)">
<g clip-path="url(#h-top)">
<ellipse cx="100" cy="104" rx="88" ry="17" fill="none" stroke="url(#h-disk)" stroke-opacity="0.3" stroke-width="0.6"/>
<ellipse cx="100" cy="104" rx="70" ry="13.5" fill="none" stroke="url(#h-disk)" stroke-opacity="0.55" stroke-width="7"/>
<ellipse cx="100" cy="104" rx="79" ry="15.2" fill="none" stroke="url(#h-disk)" stroke-opacity="0.6" stroke-width="0.7" stroke-dasharray="4 7 12 6 3 8"/>
</g>
<path d="M64 104A36 36 0 0 1 136 104" fill="none" stroke="url(#h-lens)" stroke-width="5" stroke-opacity="0.35" filter="url(#h-soft)"/>
<path d="M64 104A36 36 0 0 1 136 104" fill="none" stroke="url(#h-lens)" stroke-width="2.2" stroke-linecap="round"/>
<path d="M69 104A31 31 0 0 0 131 104" fill="none" stroke="url(#h-lens)" stroke-opacity="0.6" stroke-width="1"/>
<circle cx="100" cy="104" r="30.5" fill="none" stroke="url(#h-ring)" stroke-width="1.8"/>
<circle cx="100" cy="104" r="29" fill="#050407"/>
<g clip-path="url(#h-bot)">
<ellipse cx="100" cy="104" rx="88" ry="17" fill="none" stroke="url(#h-disk)" stroke-opacity="0.3" stroke-width="0.6"/>
<ellipse cx="100" cy="104" rx="70" ry="13.5" fill="none" stroke="url(#h-disk)" stroke-width="7" stroke-opacity="0.9"/>
<ellipse cx="100" cy="104" rx="79" ry="15.2" fill="none" stroke="url(#h-disk)" stroke-opacity="0.6" stroke-width="0.7" stroke-dasharray="4 7 12 6 3 8"/>
<ellipse cx="100" cy="104" rx="58" ry="11" fill="none" stroke="#FFEBD6" stroke-opacity="0.55" stroke-width="0.9"/>
</g>
</g>
</svg>
<div class="lname"><span class="wm">SHINKU</span><span class="kj">真空</span></div>
</div>
<div id="header">
<svg width="40" height="40" viewBox="0 0 24 24"><defs><linearGradient id="mk" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="#FBE3C8"/><stop offset="0.55" stop-color="#F0A77E"/><stop offset="1" stop-color="#C98BC0"/></linearGradient></defs>
<g transform="rotate(-9 12 12.5)"><ellipse cx="12" cy="12.5" rx="10.6" ry="2.7" fill="none" stroke="url(#mk)" stroke-width="1.3" stroke-opacity="0.7"/></g>
<circle cx="12" cy="12.5" r="5.3" fill="#050407" stroke="url(#mk)" stroke-width="1.6"/>
<g transform="rotate(-9 12 12.5)"><path d="M1.4 12.5A10.6 2.7 0 0 0 22.6 12.5" fill="none" stroke="url(#mk)" stroke-width="1.3"/></g>
</svg>
</div></body></html>`;
}

/** Remet des pixels RGB aux dimensions voulues : rogne ou complète avec le fond. */
function fit(rgb, from, to) {
  const out = Buffer.alloc(to.width * to.height * 3);
  for (let y = 0; y < to.height; y += 1) {
    for (let x = 0; x < to.width; x += 1) {
      const target = (y * to.width + x) * 3;
      if (x < from.width && y < from.height) {
        const source = (y * from.width + x) * 3;
        out[target] = rgb[source];
        out[target + 1] = rgb[source + 1];
        out[target + 2] = rgb[source + 2];
      } else {
        out[target] = BACKGROUND[0];
        out[target + 1] = BACKGROUND[1];
        out[target + 2] = BACKGROUND[2];
      }
    }
  }
  return out;
}

/** Un BMP 24 bits : en-tête, puis les lignes de bas en haut, en BGR, alignées sur 4 octets. */
function bmp(rgb, width, height) {
  const stride = Math.ceil((width * 3) / 4) * 4;
  const header = Buffer.alloc(54);
  header.write('BM', 0, 'latin1');
  header.writeUInt32LE(54 + stride * height, 2);
  header.writeUInt32LE(54, 10);
  header.writeUInt32LE(40, 14);
  header.writeInt32LE(width, 18);
  header.writeInt32LE(height, 22);
  header.writeUInt16LE(1, 26);
  header.writeUInt16LE(24, 28);
  header.writeUInt32LE(stride * height, 34);
  header.writeInt32LE(2835, 38);
  header.writeInt32LE(2835, 42);
  const pixels = Buffer.alloc(stride * height);
  for (let y = 0; y < height; y += 1) {
    for (let x = 0; x < width; x += 1) {
      const from = (y * width + x) * 3;
      const to = (height - 1 - y) * stride + x * 3;
      pixels[to] = rgb[from + 2];
      pixels[to + 1] = rgb[from + 1];
      pixels[to + 2] = rgb[from];
    }
  }
  return Buffer.concat([header, pixels]);
}

async function main() {
  const previewAt = process.argv.indexOf('--apercu');
  const preview = previewAt === -1 ? null : process.argv[previewAt + 1];
  const { chromium } = createRequire(import.meta.url)('playwright');
  const html = await page();
  await mkdir(output, { recursive: true });
  const browser = await chromium.launch();
  try {
    for (const scale of SCALES) {
      const context = await browser.newContext({
        viewport: { width: 400, height: 400 },
        deviceScaleFactor: scale / 100,
      });
      const tab = await context.newPage();
      await tab.setContent(html);
      // Les polices ne se chargent qu'à l'usage : on les demande avant de photographier.
      await tab.evaluate(async () => {
        await Promise.all([document.fonts.load("600 12px 'Orbitron'"), document.fonts.load("400 11px 'M PLUS 1'", '真空')]);
        await document.fonts.ready;
      });
      for (const piece of PIECES) {
        const target = { width: Math.round((piece.width * scale) / 100), height: Math.round((piece.height * scale) / 100) };
        const png = await tab.locator(piece.element).screenshot({ animations: 'disabled' });
        if (preview !== null && preview !== undefined) {
          await mkdir(preview, { recursive: true });
          await writeFile(path.join(preview, `${piece.name}-${scale}.png`), png);
        }
        // Le navigateur décode lui-même le PNG : pas de dépendance pour le lire.
        const decoded = await tab.evaluate(async (source) => {
          const image = new Image();
          image.src = source;
          await image.decode();
          const canvas = new OffscreenCanvas(image.width, image.height);
          const drawing = canvas.getContext('2d');
          drawing.drawImage(image, 0, 0);
          const rgba = drawing.getImageData(0, 0, image.width, image.height).data;
          let text = '';
          for (let at = 0; at < rgba.length; at += 4096) {
            text += String.fromCharCode(...rgba.subarray(at, Math.min(at + 4096, rgba.length)));
          }
          return { width: image.width, height: image.height, data: btoa(text) };
        }, `data:image/png;base64,${png.toString('base64')}`);
        const rgba = Buffer.from(decoded.data, 'base64');
        const rgb = Buffer.alloc(decoded.width * decoded.height * 3);
        for (let at = 0; at < decoded.width * decoded.height; at += 1) {
          rgb[at * 3] = rgba[at * 4];
          rgb[at * 3 + 1] = rgba[at * 4 + 1];
          rgb[at * 3 + 2] = rgba[at * 4 + 2];
        }
        const file = path.join(output, `${piece.name}-${scale}.bmp`);
        await writeFile(file, bmp(fit(rgb, decoded, target), target.width, target.height));
        console.log(`${path.relative(process.cwd(), file)} : ${target.width} × ${target.height}`);
      }
      await context.close();
    }
  } finally {
    await browser.close();
  }
}

await main();
