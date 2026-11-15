// Le câblage entre l'interface et l'application Tauri.
//
// La fenêtre ne peut pas être lancée dans les tests : elle a besoin de
// Windows et de WebView2. Ce qui la relie à l'interface est donc vérifié ici,
// à la lecture des fichiers : une commande oubliée dans une des listes, une
// permission en trop ou un style écrit dans la page ne se verraient sinon
// qu'une fois l'application installée.

import assert from 'node:assert/strict';
import { readdirSync, readFileSync } from 'node:fs';
import { test } from 'node:test';

const root = new URL('../', import.meta.url);
const read = (path: string): string => readFileSync(new URL(path, root), 'utf8');

/** Toutes les captures du premier groupe d'un motif, dans l'ordre du texte. */
function captures(text: string, pattern: RegExp): string[] {
  return Array.from(text.matchAll(pattern), (match) => match[1] ?? '');
}

/** Le texte compris entre un début et la première fin qui le suit. */
function between(text: string, start: string, end: string): string {
  const from = text.indexOf(start);
  assert.notEqual(from, -1, `« ${start} » introuvable`);
  const to = text.indexOf(end, from + start.length);
  assert.notEqual(to, -1, `« ${end} » introuvable après « ${start} »`);
  return text.slice(from + start.length, to);
}

const sorted = (names: Iterable<string>): string[] => [...names].sort();

const capability = JSON.parse(read('src-tauri/capabilities/default.json')) as {
  windows: string[];
  permissions: string[];
};
const config = JSON.parse(read('src-tauri/tauri.conf.json')) as {
  build: { devUrl: string; frontendDist: string };
  app: { withGlobalTauri: boolean; windows: unknown[]; security: { csp: string; capabilities: string[] } };
};

test('les commandes sont les mêmes partout où elles sont listées', () => {
  const declared = captures(between(read('src-tauri/build.rs'), 'const COMMANDS: &[&str] = &[', '];'), /"(\w+)"/g);
  const handled = captures(between(read('src-tauri/src/app.rs'), 'generate_handler![', '])'), /commands::(\w+)/g);
  const written = captures(read('src-tauri/src/commands.rs'), /#\[tauri::command[^\]]*\]\s*pub (?:async )?fn (\w+)/g);
  const allowed = capability.permissions
    .filter((permission) => !permission.includes(':'))
    .map((permission) => {
      assert.match(permission, /^allow-[a-z-]+$/, permission);
      return permission.slice('allow-'.length).replaceAll('-', '_');
    });
  const called = captures(read('src/backend/tauri.ts'), /invoke(?:<[^>]*>)?\(\s*'(\w+)'/g);

  assert.ok(declared.length > 0, 'aucune commande déclarée');
  assert.equal(new Set(declared).size, declared.length, 'une commande est déclarée deux fois');
  assert.deepEqual(sorted(handled), sorted(declared), 'generate_handler! (app.rs) et build.rs');
  assert.deepEqual(sorted(written), sorted(declared), 'commands.rs et build.rs');
  assert.deepEqual(sorted(allowed), sorted(declared), 'capabilities/default.json et build.rs');
  assert.deepEqual(sorted(new Set(called)), sorted(declared), 'backend/tauri.ts et build.rs');
});

test('la fenêtre ne reçoit que les permissions dont elle se sert', () => {
  const core = capability.permissions.filter((permission) => permission.includes(':'));
  assert.deepEqual(sorted(core), [
    // Les trois boutons de la barre de titre (backend/tauri.ts).
    'core:window:allow-close',
    // Le double-clic et le glisser sur les zones « data-tauri-drag-region ».
    'core:window:allow-internal-toggle-maximize',
    'core:window:allow-minimize',
    'core:window:allow-start-dragging',
    'core:window:allow-toggle-maximize',
  ]);

  const tauri = read('src/backend/tauri.ts');
  for (const method of ['minimize', 'toggleMaximize', 'close']) {
    assert.ok(tauri.includes(`appWindow.${method}()`), `appWindow.${method}() attendu dans backend/tauri.ts`);
  }
  // Aucune autre partie de l'interface ne parle à Tauri.
  for (const file of sourceFiles('src')) {
    if (file !== 'src/backend/tauri.ts') {
      assert.ok(!read(file).includes('@tauri-apps/'), `${file} ne doit pas appeler Tauri directement`);
    }
  }
});

test('la permission vise la fenêtre que l’application ouvre', () => {
  const app = read('src-tauri/src/app.rs');
  const [label] = captures(app, /const MAIN_WINDOW: &str = "(\w+)";/g);
  assert.deepEqual(capability.windows, [label]);
  assert.deepEqual(config.app.security.capabilities, ['default']);
  // La fenêtre est créée par le code (app.rs), pas par la configuration.
  assert.deepEqual(config.app.windows, []);
  assert.equal(config.app.withGlobalTauri, false);
});

test('l’adresse de développement est la même pour Vite, Tauri et la fenêtre', () => {
  const vite = read('vite.config.ts');
  const [host] = captures(between(vite, 'server: {', '}'), /host: '([\d.]+)'/g);
  const [port] = captures(between(vite, 'server: {', '}'), /port: (\d+)/g);
  assert.equal(config.build.devUrl, `http://${host}:${port}`);
  const app = read('src-tauri/src/app.rs');
  assert.ok(app.includes(`Some("${host}")`), `app.rs doit accepter ${host} en développement`);
  assert.ok(app.includes(`Some(${port})`), `app.rs doit accepter le port ${port} en développement`);
});

test('la politique de sécurité de contenu n’accepte rien d’écrit dans la page', () => {
  const directives = new Map(
    config.app.security.csp.split(';').map((directive) => {
      const [name = '', ...sources] = directive.trim().split(/\s+/);
      return [name, sources] as const;
    }),
  );
  assert.deepEqual(directives.get('default-src'), ["'self'"]);
  assert.deepEqual(directives.get('script-src'), ["'self'"]);
  assert.deepEqual(directives.get('style-src'), ["'self'"]);
  assert.deepEqual(directives.get('object-src'), ["'none'"]);
  assert.deepEqual(directives.get('base-uri'), ["'none'"]);
  // Seule destination permise : le canal de Tauri vers le cœur. Aucun site.
  assert.deepEqual(directives.get('connect-src'), ['ipc:', 'http://ipc.localhost']);
  assert.ok(!/unsafe-|https:|\*/.test(config.app.security.csp), config.app.security.csp);
});

test('aucun gabarit n’écrit de style dans la page', () => {
  // Un attribut `style` serait refusé par la politique ci-dessus, y compris
  // posé par une bibliothèque : les styles calculés passent par views/style.ts.
  const html = read('index.html');
  assert.ok(!/\sstyle\s*=|<style/i.test(html), 'index.html');
  for (const file of sourceFiles('src')) {
    const text = read(file);
    assert.ok(!/\sstyle\s*=\s*["'$]/.test(text), `${file} : attribut style`);
    assert.ok(!/styleMap|style-map|unsafeHTML|unsafe-html|unsafeSVG|unsafe-svg/.test(text), `${file} : directive interdite`);
    assert.ok(!/\binnerHTML\b|\bouterHTML\b|insertAdjacentHTML|document\.write/.test(text), `${file} : HTML écrit à la main`);
    assert.ok(!/\beval\(|new Function\(/.test(text), `${file} : code évalué`);
  }
});

test('le fond de la fenêtre est celui de l’interface', () => {
  const tokens = read('src/styles/tokens.css');
  const app = read('src-tauri/src/app.rs');
  const background = (name: string): string => {
    const [red, green, blue] = captures(
      between(app, `const ${name}: Color = Color(`, ');'),
      /0x([0-9a-f]{2})/g,
    );
    return `#${red}${green}${blue}`;
  };
  const dark = between(tokens, ":root[data-theme='dark'] {", '}');
  const light = between(tokens, ":root[data-theme='light'] {", '}');
  assert.equal(captures(dark, /--bg: (#[0-9a-f]{6});/g)[0], background('DARK_BACKGROUND'));
  assert.equal(captures(light, /--bg: (#[0-9a-f]{6});/g)[0], background('LIGHT_BACKGROUND'));
});

/** Les fichiers TypeScript d'un dossier, sous-dossiers compris. */
function sourceFiles(folder: string): string[] {
  return readdirSync(new URL(`${folder}/`, root), { withFileTypes: true }).flatMap((entry) => {
    const path = `${folder}/${entry.name}`;
    if (entry.isDirectory()) {
      return sourceFiles(path);
    }
    return entry.name.endsWith('.ts') ? [path] : [];
  });
}
