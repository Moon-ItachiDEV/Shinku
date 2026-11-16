// Essai de la page « Confidentialité » sur le vrai Windows : la vraie fenêtre efface-t-elle ce qu'on lui demande,
// et rien d'autre ?
//
// CET ESSAI EFFACE VRAIMENT. Il est fait pour une machine jetable (celle de l'intégration continue), jamais pour le
// PC de quelqu'un : il met de côté le dossier `User Data` de Chrome, vide trois clés de HKCU, le dossier « Récents »
// et le presse-papiers. Il ne tourne que si la variable d'environnement SHINKU_ESSAI_CONFIDENTIALITE vaut 1 (la CI
// seule la pose) ; sinon il s'arrête en le disant, sans rien toucher.
//
// Pour chaque scénario, le pilote :
//   1. lance une étape de `essai-confidentialite.ps1` qui prépare la machine (un faux profil Chrome, un fichier
//      tenu ouvert, des valeurs de registre...) ;
//   2. lance le vrai `shinku.exe` installé, ouvre « Confidentialité », et lit la page ;
//   3. ne coche que la ligne que le scénario vise, vérifie que la confirmation liste exactement cela, confirme ;
//   4. lit l'issue de la ligne et la carte « Dernier effacement » ;
//   5. ferme la fenêtre et lance l'étape de vérification du script, qui relit la machine.
// Un scénario qui échoue n'arrête pas les suivants (un premier passage dit ainsi tout ce qu'il peut dire) : le
// pilote les rejoue tous, remet la machine en place, et sort avec le code 1 si l'un d'eux a échoué.
//
// Ce pilote est à part de `essai-fenetre.mjs`, qui est l'essai permanent et ne clique jamais sur « Effacer » dans
// cette page : il reprend de lui le strict nécessaire (le lancement avec un jeton ordinaire, le port de débogage de
// WebView2, les gestes de souris) plutôt que de le refondre.
//
//   $env:SHINKU_ESSAI_CONFIDENTIALITE = '1'
//   node scripts/essai-confidentialite.mjs --programme "C:\Program Files\Shinku\shinku.exe" --captures captures
//
// Mise au point hors de Windows : avec `--sans-script`, aucune étape du script PowerShell n'est lancée, et
// `--adresse` désigne la démo (`npm run build:demo`, servie par un serveur web) dans un Chrome lancé par le
// programme que donne `--programme`, avec le port de débogage de la variable WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS.
// La démo n'efface rien : le garde-fou n'est alors pas demandé, et les attentes sur l'issue sont allégées (elles
// dépendent de `?privacyflow=`). `--profil Léa` désigne le profil à viser dans la démo (par défaut : « 77 »).
//
// Sans dépendance : Node 22 suffit.

import { spawn, spawnSync } from 'node:child_process';
import { closeSync, existsSync, mkdirSync, openSync, readFileSync, writeFileSync } from 'node:fs';
import { createServer } from 'node:net';
import os from 'node:os';
import path from 'node:path';
import { setTimeout as sleep } from 'node:timers/promises';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';

import { DOM, EXPECTED, readPrivacy } from './essai-confidentialite-dom.mjs';

const { values: options } = parseArgs({
  options: {
    /** Le programme à lancer. */
    programme: { type: 'string' },
    /** Dossier où écrire les captures d'écran. */
    captures: { type: 'string' },
    /** Début de l'adresse de la page (pour mettre l'essai au point hors de Windows). */
    adresse: { type: 'string' },
    /** Ne lance aucune étape du script PowerShell (démo seulement : exige --adresse). */
    'sans-script': { type: 'boolean', default: false },
    /** Ne joue que ces scénarios (noms séparés par des virgules). */
    seulement: { type: 'string' },
    /** Le profil Chrome visé : un texte de son nom (par défaut « 77 », le faux profil). */
    profil: { type: 'string' },
  },
});

const onWindows = process.platform === 'win32';
const SCRIPT = fileURLToPath(new URL('essai-confidentialite.ps1', import.meta.url));
const LAUNCHER = fileURLToPath(new URL('lancer-en-utilisateur.ps1', import.meta.url));
const WITHOUT_SCRIPT = options['sans-script'];
/** Hors démo, les attentes sont celles de la machine d'essai ; en démo, on lit seulement. */
const STRICT = !WITHOUT_SCRIPT;
const WORK = path.join(process.env.TEMP ?? process.env.TMPDIR ?? os.tmpdir(), 'essai-confidentialite-pilote');

/** Ce que disent les codes de sortie de shinku.exe (src-tauri/src/app.rs). */
const EXIT_CODES = new Map([
  [1, 'la fenêtre n’a pas pu s’ouvrir'],
  [2, 'WebView2 est absent'],
  [3, 'refus de tourner en tant qu’administrateur'],
]);

// ── Journal ────────────────────────────────────────────────────────────────

const started = Date.now();
const elapsed = () => `${((Date.now() - started) / 1000).toFixed(1).padStart(6)} s`;
const say = (line) => console.log(`[${elapsed()}] ${line}`);
const done = (line) => say(`✓ ${line}`);
const note = (line) => say(`• ${line}`);

function quote(text, limit = 40) {
  const lines = String(text)
    .split('\n')
    .map((line) => line.trimEnd())
    .filter((line) => line !== '');
  for (const line of lines.slice(0, limit)) {
    console.log(`           │ ${line}`);
  }
  if (lines.length > limit) {
    console.log(`           │ … (${lines.length - limit} lignes de plus)`);
  }
}

/** Une vérification manquée : le scénario s'arrête et le dit (« attendu ... ; vu ... »). */
class Failure extends Error {}

function fail(message) {
  throw new Failure(message);
}

const lower = (text) => text.toLocaleLowerCase('fr');
const shows = (screen, expected) => lower(screen).includes(lower(expected));

async function until(what, timeout, probe, interval = 200) {
  const deadline = Date.now() + timeout;
  for (;;) {
    const answer = await probe();
    if (answer !== null && answer !== undefined && answer !== false) {
      return answer;
    }
    if (Date.now() >= deadline) {
      fail(`${what} : toujours pas après ${Math.round(timeout / 1000)} s`);
    }
    await sleep(interval);
  }
}

// ── Le garde-fou ───────────────────────────────────────────────────────────

// `--sans-script` ne sert qu'à mettre le pilote au point contre la démo : une adresse de la machine locale, et jamais
// le vrai shinku.exe (qui, lui, efface pour de bon la ligne visée).
if (WITHOUT_SCRIPT) {
  const local = /^http:\/\/(127\.0\.0\.1|localhost):\d+(\/|$)/.test(options.adresse ?? '');
  const real = path.basename(options.programme ?? '').toLowerCase() === 'shinku.exe';
  if (!local || real) {
    console.error(
      '--sans-script ne vaut que pour la démo : il exige --adresse http://127.0.0.1:<port>/ (ou localhost) et refuse le vrai shinku.exe. Rien n’a été fait.',
    );
    process.exit(2);
  }
}
if (!WITHOUT_SCRIPT && process.env.SHINKU_ESSAI_CONFIDENTIALITE !== '1') {
  console.log('Cet essai efface vraiment (historique de Chrome mis de côté, clés du registre, récents, presse-papiers) :');
  console.log('il est fait pour une machine jetable. Il ne tourne que si SHINKU_ESSAI_CONFIDENTIALITE vaut 1 ; rien n’a été touché.');
  process.exit(2);
}

// ── Les étapes du script PowerShell ────────────────────────────────────────

/**
 * Lance une étape de essai-confidentialite.ps1 et rend son code. La sortie va dans un fichier, pas dans un tube :
 * un processus qu'une étape laisse en marche (Chrome, un fichier tenu ouvert) garderait un tube ouvert et
 * bloquerait la lecture jusqu'à sa fin. Chaque ligne est recopiée telle quelle, « FAIT : » comprises.
 */
async function runStep(name, extra = [], { timeout = 240_000, tolerate = [] } = {}) {
  if (WITHOUT_SCRIPT) {
    note(`étape ${name} : sautée (--sans-script)`);
    return 0;
  }
  mkdirSync(WORK, { recursive: true });
  const file = path.join(WORK, `etape-${name}.txt`);
  const fd = openSync(file, 'w');
  say(`▶ étape ${name}${extra.length === 0 ? '' : ` ${extra.join(' ')}`}`);
  const child = spawn(
    'powershell',
    ['-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', SCRIPT, '-Etape', name, ...extra],
    { stdio: ['ignore', fd, fd], env: process.env },
  );
  closeSync(fd);
  const code = await new Promise((resolve) => {
    const timer = setTimeout(() => {
      spawnSync('taskkill', ['/pid', String(child.pid), '/t', '/f'], { stdio: 'ignore' });
      resolve('délai');
    }, timeout);
    child.once('error', () => (clearTimeout(timer), resolve('lancement')));
    child.once('exit', (exitCode) => (clearTimeout(timer), resolve(exitCode)));
  });
  const text = readFileSync(file, 'utf8').replace(/^\uFEFF/, '');
  // Ce que le script dit n'avoir pas pu faire n'est pas un échec, mais le résumé ne dit pas « réussi » sans le nommer.
  for (const line of text.split(/\r?\n/)) {
    if (/ÉPINGLÉ NON VU|ÉPINGLAGE IMPOSSIBLE|n'a été vue par EnumWindows|voie : repli|n'a pas pu être défait/.test(line)) {
      unproven.push(`${name} : ${line.replace(/^\s*FAIT : /, '').trim().slice(0, 220)}`);
    }
  }
  for (const line of text.split(/\r?\n/)) {
    if (line.trim() !== '') {
      console.log(`           ${line}`);
    }
  }
  if (code === 0 || tolerate.includes(code)) {
    return code;
  }
  if (code === 'délai') {
    fail(`l’étape ${name} n’a pas rendu la main en ${timeout / 1000} s`);
  }
  if (code === 'lancement') {
    fail(`l’étape ${name} n’a pas pu être lancée (powershell introuvable ?)`);
  }
  const echec = text.split(/\r?\n/).find((line) => line.includes('ECHEC'));
  fail(`l’étape ${name} a échoué (code ${code})${echec === undefined ? '' : `\n${echec.trim()}`}`);
}

// ── Le programme ───────────────────────────────────────────────────────────

function freePort() {
  return new Promise((resolve, reject) => {
    const server = createServer();
    server.once('error', reject);
    server.listen(0, '127.0.0.1', () => {
      const { port } = server.address();
      server.close(() => resolve(port));
    });
  });
}

/** Vrai si cet essai tourne avec les droits d'un administrateur (niveau d'intégrité élevé ou système). */
function elevated() {
  if (!onWindows) {
    return false;
  }
  const groups = spawnSync('whoami', ['/groups', '/fo', 'csv', '/nh'], { encoding: 'utf8' }).stdout ?? '';
  return /S-1-16-(12288|16384)/.test(groups);
}

/** Lance le programme, WebView2 ouvrant son port de débogage ; en administrateur, avec un jeton ordinaire (comme essai-fenetre.mjs). */
function launch(program, port, asUser) {
  const run = { child: null, pid: null, error: null, exit: null, messages: '' };
  const [command, parameters] = asUser
    ? ['powershell', ['-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', LAUNCHER, '-Programme', program]]
    : [program, []];
  try {
    run.child = spawn(command, parameters, {
      env: { ...process.env, WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${port}` },
      stdio: asUser ? ['ignore', 'pipe', 'pipe'] : 'ignore',
    });
  } catch (error) {
    run.error = error;
    return run;
  }
  if (asUser) {
    const read = (chunk) => {
      run.messages += chunk;
      const announced = /^PID (\d+)/m.exec(run.messages);
      if (announced !== null) {
        run.pid = Number(announced[1]);
      }
    };
    run.child.stdout.setEncoding('utf8').on('data', read);
    run.child.stderr.setEncoding('utf8').on('data', read);
  } else {
    run.pid = run.child.pid ?? null;
  }
  run.child.once('error', (error) => {
    run.error = error;
  });
  run.child.once('close', (code, signal) => {
    run.exit = { code, signal };
  });
  return run;
}

/** Arrête le programme et les processus de WebView2 qu'il a lancés, et attend qu'ils soient partis. */
async function stop(run) {
  if (run.exit === null && run.child?.pid !== undefined) {
    if (onWindows) {
      spawnSync('taskkill', ['/pid', String(run.child.pid), '/t', '/f'], { stdio: 'ignore' });
      if (run.pid !== null && run.pid !== run.child.pid) {
        spawnSync('taskkill', ['/pid', String(run.pid), '/t', '/f'], { stdio: 'ignore' });
      }
    } else {
      run.child.kill('SIGKILL');
    }
  }
  if (onWindows && options.programme !== undefined) {
    const image = path.basename(options.programme);
    for (let wait = 0; wait < 40; wait += 1) {
      const list = spawnSync('tasklist', ['/fi', `imagename eq ${image}`, '/nh'], { encoding: 'utf8' }).stdout ?? '';
      if (!list.toLowerCase().includes(image.toLowerCase())) {
        break;
      }
      await sleep(250);
    }
    // Le moteur web de la fenêtre qui vient de se fermer finit de s'arrêter : la suivante a son propre port.
    await sleep(1500);
  }
}

// ── La page, par le port de débogage ───────────────────────────────────────

async function findPage(port, run, timeout) {
  const expected = (url) =>
    options.adresse !== undefined ? url.startsWith(options.adresse) : /^https?:\/\/tauri\.localhost\//.test(url);
  let seen = 'le port de débogage ne répond pas';
  let silence = '';
  const deadline = Date.now() + timeout;
  while (Date.now() < deadline) {
    if (run.error !== null) {
      fail(`Windows refuse de lancer le programme (${run.error.message})`);
    }
    if (run.exit !== null && run.pid === null) {
      fail(`Windows refuse de lancer le programme.\n${run.messages.trim()}`);
    }
    if (run.exit !== null) {
      const meaning = EXIT_CODES.get(run.exit.code);
      fail(`le programme s’est arrêté avant d’ouvrir sa fenêtre (code ${run.exit.code}${meaning ? ` : ${meaning}` : ''})`);
    }
    try {
      const response = await fetch(`http://127.0.0.1:${port}/json/list`, { signal: AbortSignal.timeout(2000) });
      const body = await response.text();
      if (!response.ok) {
        throw new Error(`HTTP ${response.status} : ${body.slice(0, 200)}`);
      }
      const targets = JSON.parse(body);
      const page = targets.find((target) => target.type === 'page' && expected(target.url));
      if (page !== undefined) {
        return page;
      }
      seen = `pages ouvertes : ${targets.map((target) => `${target.type} ${target.url}`).join(', ') || 'aucune'}`;
    } catch (error) {
      silence = `${error.message}${error.cause ? ` (${error.cause.code ?? error.cause.message})` : ''}`;
    }
    await sleep(250);
  }
  fail(`la fenêtre n’a pas ouvert sa page en ${Math.round(timeout / 1000)} s (${seen})\nport ${port}, dernière réponse : ${silence || 'aucune'}`);
}

/** Une conversation avec la page (protocole DevTools). */
class Session {
  #socket;
  #lastId = 0;
  #pending = new Map();
  #listeners = new Map();

  static open(url) {
    return new Promise((resolve, reject) => {
      const socket = new WebSocket(url);
      socket.addEventListener('open', () => resolve(new Session(socket)), { once: true });
      socket.addEventListener('error', () => reject(new Failure('le port de débogage refuse la connexion')), { once: true });
    });
  }

  constructor(socket) {
    this.#socket = socket;
    socket.addEventListener('message', (event) => {
      const message = JSON.parse(event.data);
      if (message.id === undefined) {
        for (const listener of this.#listeners.get(message.method) ?? []) {
          listener(message.params);
        }
        return;
      }
      const waiting = this.#pending.get(message.id);
      this.#pending.delete(message.id);
      if (message.error !== undefined) {
        waiting?.reject(new Error(`${waiting.method} : ${message.error.message}`));
      } else {
        waiting?.resolve(message.result);
      }
    });
    socket.addEventListener('close', () => {
      for (const waiting of this.#pending.values()) {
        waiting.reject(new Error(`${waiting.method} : la fenêtre s’est fermée`));
      }
      this.#pending.clear();
    });
  }

  on(event, listener) {
    this.#listeners.set(event, [...(this.#listeners.get(event) ?? []), listener]);
  }

  send(method, params = {}, timeout = 30_000) {
    const id = ++this.#lastId;
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        this.#pending.delete(id);
        reject(new Error(`${method} : pas de réponse en ${timeout / 1000} s`));
      }, timeout);
      this.#pending.set(id, {
        method,
        resolve: (value) => (clearTimeout(timer), resolve(value)),
        reject: (error) => (clearTimeout(timer), reject(error)),
      });
      this.#socket.send(JSON.stringify({ id, method, params }));
    });
  }

  /** Exécute une fonction dans la page et rend sa valeur. */
  async run(inPage, ...parameters) {
    const { result, exceptionDetails } = await this.send('Runtime.evaluate', {
      expression: `(${inPage})(...${JSON.stringify(parameters)})`,
      returnByValue: true,
      awaitPromise: true,
    });
    if (exceptionDetails !== undefined) {
      throw new Error(`dans la page : ${exceptionDetails.exception?.description ?? exceptionDetails.text}`);
    }
    return result.value;
  }

  close() {
    this.#socket.close();
  }
}

// Les fonctions ci-dessous s'exécutent dans la page : elles ne voient rien de ce fichier.

/** Vrai quand la fenêtre a affiché son menu (ou 'failed' si l'interface n'a pas démarré). */
function readBoot() {
  if (document.querySelector('.side') !== null) {
    return 'ready';
  }
  return document.querySelector('.loader [role=alert]') !== null ? 'failed' : 'waiting';
}

/** Le menu : le module affiché, et un message éventuel. */
function readShell() {
  const text = (element) => (element?.innerText ?? '').replace(/[ \u00a0\u202f]+/g, ' ').trim();
  const toast = document.querySelector('.toast.show');
  return { current: text(document.querySelector('button.nav[aria-current=page] span')), toast: toast === null ? '' : text(toast) };
}

/** Où cliquer sur un élément : son centre, une fois qu'il est à l'écran, et ce qui le recouvre. */
function locate(selector, label) {
  const clean = (text) => text.replace(/\s+/g, ' ').trim();
  const element = [...document.querySelectorAll(selector)].find(
    (candidate) => label === null || clean(candidate.textContent ?? '').startsWith(label),
  );
  if (element === undefined) {
    return null;
  }
  element.scrollIntoView({ block: 'nearest', inline: 'nearest' });
  const box = element.getBoundingClientRect();
  const x = box.left + box.width / 2;
  const y = box.top + box.height / 2;
  const hit = document.elementFromPoint(x, y);
  return {
    x,
    y,
    disabled: element.disabled === true || element.getAttribute('aria-disabled') === 'true',
    reached: hit !== null && element.contains(hit),
    cover: hit === null ? 'rien (hors de la fenêtre)' : `<${hit.tagName.toLowerCase()} class="${hit.className}">`,
  };
}

/** Clique dans la page, sans passer par la souris. */
function clickInPage(selector, label) {
  const clean = (text) => text.replace(/\s+/g, ' ').trim();
  const element = [...document.querySelectorAll(selector)].find(
    (candidate) => label === null || clean(candidate.textContent ?? '').startsWith(label),
  );
  element?.click();
  return element !== undefined;
}

/** Où cliquer pour cocher la case dont le nom accessible est `name` : sa ligne entière (le `label`). */
function locateRow(checkSelector, name) {
  const normal = (text) => (text ?? '').replace(/[ \u00a0\u202f]+/g, ' ').trim();
  const input = [...document.querySelectorAll(checkSelector)].find((candidate) => normal(candidate.getAttribute('aria-label')) === name);
  if (input === undefined) {
    return null;
  }
  const row = input.closest('label') ?? input;
  row.scrollIntoView({ block: 'nearest', inline: 'nearest' });
  const box = row.getBoundingClientRect();
  const x = box.left + box.width / 2;
  const y = box.top + box.height / 2;
  const hit = document.elementFromPoint(x, y);
  return {
    x,
    y,
    disabled: input.getAttribute('aria-disabled') === 'true',
    reached: hit !== null && row.contains(hit),
    cover: hit === null ? 'rien (hors de la fenêtre)' : `<${hit.tagName.toLowerCase()} class="${hit.className}">`,
  };
}

function clickRow(checkSelector, name) {
  const normal = (text) => (text ?? '').replace(/[ \u00a0\u202f]+/g, ' ').trim();
  const input = [...document.querySelectorAll(checkSelector)].find((candidate) => normal(candidate.getAttribute('aria-label')) === name);
  input?.click();
  return input !== undefined;
}

// ── La fenêtre ─────────────────────────────────────────────────────────────

class Window {
  /** Ce que la page a signalé d'anormal : erreurs de console, ressources refusées. */
  problems = [];
  /** Vrai tant que la page reçoit les gestes de souris envoyés par le port de débogage. */
  mouse = true;
  #shots = 0;
  #pictures = true;

  constructor(session, prefix) {
    this.session = session;
    this.prefix = prefix;
    session.on('Runtime.consoleAPICalled', ({ type, args }) => {
      const text = args.map((value) => value.value ?? value.description ?? value.type).join(' ');
      if (type === 'error' || type === 'assert') {
        this.problems.push(`console.${type} : ${text}`);
      }
    });
    session.on('Runtime.exceptionThrown', ({ exceptionDetails }) => {
      this.problems.push(`erreur non rattrapée : ${exceptionDetails.exception?.description ?? exceptionDetails.text}`);
    });
    session.on('Log.entryAdded', ({ entry }) => {
      if (entry.level === 'error') {
        this.problems.push(`${entry.source}, ${entry.level} : ${entry.text}${entry.url ? ` (${entry.url})` : ''}`);
      }
    });
  }

  async listen() {
    await this.session.send('Runtime.enable');
    await this.session.send('Log.enable');
    await this.session.send('Page.enable');
    try {
      await this.session.send('Input.dispatchMouseEvent', { type: 'mouseMoved', x: 1, y: 1 }, 5000);
    } catch {
      this.mouse = false;
      say('  la page ne reçoit pas la souris : les clics seront déclenchés dans la page');
    }
  }

  /** La page Confidentialité, telle qu'elle s'affiche (voir essai-confidentialite-dom.mjs). */
  privacy() {
    return this.session.run(readPrivacy, DOM);
  }

  /** Clique sur un élément comme le ferait la souris, une fois qu'il ne bouge plus. */
  async press(what, find, inPage, parameters) {
    let before = null;
    const target = await until(`${what} est prêt`, 20_000, async () => {
      const found = await this.session.run(find, ...parameters);
      if (found === null || found.disabled) {
        before = null;
        return null;
      }
      const still = before !== null && Math.abs(before.x - found.x) < 0.5 && Math.abs(before.y - found.y) < 0.5;
      before = found;
      return still ? found : null;
    });
    if (!target.reached) {
      fail(`${what} est recouvert par ${target.cover} : un clic ne l’atteindrait pas`);
    }
    if (!this.mouse) {
      await this.session.run(inPage, ...parameters);
      return;
    }
    const point = { x: target.x, y: target.y };
    const button = { ...point, button: 'left', clickCount: 1 };
    await this.session.send('Input.dispatchMouseEvent', { type: 'mouseMoved', ...point });
    await this.session.send('Input.dispatchMouseEvent', { type: 'mousePressed', ...button });
    await this.session.send('Input.dispatchMouseEvent', { type: 'mouseReleased', ...button });
  }

  click(what, selector, label = null) {
    return this.press(`le bouton « ${what} »`, locate, clickInPage, [selector, label]);
  }

  /** Enregistre l'image de la page. */
  async capture(name) {
    if (options.captures === undefined || !this.#pictures) {
      return;
    }
    this.#shots += 1;
    const file = path.join(options.captures, `${this.prefix}-${String(this.#shots).padStart(2, '0')}-${name}.png`);
    await sleep(400);
    try {
      const { data } = await this.session.send('Page.captureScreenshot', { format: 'png' }, 15_000);
      writeFileSync(file, Buffer.from(data, 'base64'));
    } catch (error) {
      this.#pictures = false;
      say(`  capture de la page impossible (${error.message})`);
    }
  }
}

// ── Lire la page ───────────────────────────────────────────────────────────

const OUTCOME_TEXTS = Object.values(EXPECTED.outcomes);

/** Les lignes de la page, une par ligne de texte : de quoi comprendre un échec. */
function describeScreen(screen) {
  const lines = [`titre « ${screen.title} », ${screen.rows.length} ligne(s), bouton « ${screen.erase?.label ?? 'absent'} »${screen.erase?.inert ? ' (inerte)' : ''}`];
  for (const row of screen.rows) {
    const mark = row.kind === 'check' ? (row.checked ? '[x]' : '[ ]') : row.kind === 'inert' ? '[inerte]' : row.kind === 'empty' ? '[vide]' : '[?]';
    lines.push(`${mark} ${row.block} / ${row.title}${row.sub === '' ? '' : ` / ${row.sub}`}${row.value === '' ? '' : ` / ${row.value}`}${row.outcome === '' ? '' : ` -> ${row.outcome}`}`);
  }
  for (const warning of screen.running) {
    lines.push(`navigateur ouvert : ${warning.block} : ${warning.text}`);
  }
  for (const line of screen.last) {
    lines.push(`dernier effacement : ${line.block} / ${line.trace} -> ${line.outcome}${line.help === '' ? '' : ` (${line.help})`}`);
  }
  if (screen.dialog !== null) {
    lines.push(`confirmation ${screen.dialog.open ? 'ouverte' : 'fermée'} : ${screen.dialog.blocks.map((block) => `${block.name} [${block.lines.join(' ; ')}]`).join(' ; ')}`);
  }
  return lines.join('\n');
}

/** Ouvre « Confidentialité » par le menu et attend que la page soit chargée, au repos, et immobile. */
async function openPrivacy(window) {
  const boot = await until('la fenêtre affiche son menu', 60_000, async () => {
    const state = await window.session.run(readBoot);
    return state === 'waiting' ? null : state;
  });
  if (boot === 'failed') {
    fail('l’interface n’a pas démarré (message d’erreur à la place du menu)');
  }
  await window.click(EXPECTED.title, 'button.nav', EXPECTED.title);
  let before = null;
  const screen = await until('la page « Confidentialité » est affichée, chargée et immobile', 60_000, async () => {
    const now = await window.privacy();
    const ready = now.title === EXPECTED.title && now.ready && !now.busy;
    const key = ready ? JSON.stringify([now.rows, now.running, now.erase]) : null;
    const still = key !== null && key === before;
    before = key;
    return still ? now : null;
  }, 400);
  const shell = await window.session.run(readShell);
  if (shell.toast !== '') {
    fail(`la fenêtre affiche le message « ${shell.toast} »`);
  }
  return screen;
}

// ── Les lignes visées ──────────────────────────────────────────────────────

const PROFILE = options.profil === undefined ? /\b77\b/ : new RegExp(options.profil.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'), 'i');

/** Ce que chaque scénario vise dans la page. */
const TARGETS = {
  /** Le faux profil « Profile 77 » de Chrome. */
  chrome: { label: `la ligne de Chrome « ${options.profil ?? 'Profil 77'} »`, pick: (row) => row.block === EXPECTED.chrome && PROFILE.test(row.title) },
  /** Le profil que le vrai Chrome a créé : le seul profil de Chrome. */
  chromeReal: { label: 'la ligne du profil de Chrome', pick: (row) => row.block === EXPECTED.chrome },
  recent: { label: `la ligne Windows « ${EXPECTED.recent} »`, pick: (row) => row.block === EXPECTED.windows && row.title === EXPECTED.recent },
  typed: { label: `la ligne Windows « ${EXPECTED.typed} »`, pick: (row) => row.block === EXPECTED.windows && row.title === EXPECTED.typed },
  clipboard: { label: `la ligne Windows « ${EXPECTED.clipboard} »`, pick: (row) => row.block === EXPECTED.windows && row.title === EXPECTED.clipboard },
};

function pickRow(screen, target) {
  const rows = screen.rows.filter(target.pick);
  if (rows.length > 1) {
    fail(`attendu : ${target.label} une seule fois ; vu : ${rows.length} lignes\n${describeScreen(screen)}`);
  }
  return rows[0] ?? null;
}

/** Ce que la confirmation doit lister pour cette ligne, et rien d'autre. */
function askFor(row) {
  if (row.block === EXPECTED.windows) {
    return [{ name: EXPECTED.windows, lines: [row.title] }];
  }
  return [{ name: row.block, lines: [`${row.title} : ${lower(EXPECTED.history)}`] }];
}

// ── Cocher, confirmer, lire l'issue ────────────────────────────────────────

/**
 * Coche la seule ligne visée, ouvre la confirmation, vérifie qu'elle liste exactement cela, confirme, et lit
 * l'issue. Rend { outcome, line } : le texte court de l'issue, dans la carte « Dernier effacement ».
 */
async function erase(window, row, between) {
  const label = row.label;
  await window.press(`la case « ${label} »`, locateRow, clickRow, [DOM.check, label]);
  const ticked = await until(`la case « ${label} » est cochée`, 10_000, async () => {
    const now = await window.privacy();
    const mine = now.rows.find((one) => one.label === label);
    return mine?.checked === true ? now : null;
  });
  const others = ticked.rows.filter((one) => one.checked && one.label !== label);
  if (others.length > 0) {
    fail(`attendu : seule « ${label} » est cochée ; vu : aussi ${others.map((one) => `« ${one.label} »`).join(', ')}`);
  }
  if (ticked.erase === null || ticked.erase.inert) {
    fail(`attendu : le bouton « ${EXPECTED.erase} » est actif une fois la case cochée ; vu : ${ticked.erase === null ? 'absent' : 'inerte'}`);
  }
  done(`seule la case « ${label} » est cochée`);
  await window.capture('cochee');

  if (between !== undefined) {
    await between();
    const now = await window.privacy();
    if (now.erase === null || now.erase.inert || !now.rows.some((one) => one.label === label && one.checked)) {
      fail(
        `la page a relu la vue pendant le changement et n'a plus la case cochée : la preuve « Ce profil a changé » ne peut pas se faire ainsi\n${describeScreen(now)}`,
      );
    }
  }

  await window.click(EXPECTED.erase, DOM.erase, EXPECTED.erase);
  const asked = await until('la confirmation s’ouvre', 15_000, async () => {
    const now = await window.privacy();
    return now.dialog?.open === true ? now : null;
  });
  const wanted = askFor(row);
  const seen = asked.dialog.blocks;
  if (JSON.stringify(seen) !== JSON.stringify(wanted)) {
    fail(
      `attendu : la confirmation liste exactement ${JSON.stringify(wanted)} ; vu : ${JSON.stringify(seen)}\n` +
        `(la fenêtre n'a pas été confirmée : rien n'a été effacé par ce clic)\n${asked.dialog.text}`,
    );
  }
  // Une seule trace cochée : le titre et la phrase sont au singulier.
  if (!shows(asked.dialog.title, EXPECTED.askTitleOne) || !shows(asked.dialog.text, EXPECTED.directOne)) {
    fail(`attendu : la confirmation s'intitule « ${EXPECTED.askTitleOne} » et dit « ${EXPECTED.directOne} » ; vu : titre « ${asked.dialog.title} », texte :\n${asked.dialog.text}`);
  }
  done(`la confirmation liste exactement ${wanted.map((block) => `${block.name} : ${block.lines.join(', ')}`).join(' ; ')}`);
  await window.capture('confirmation');

  await window.click(EXPECTED.confirm, `${DOM.dialog} ${DOM.askConfirm}`, EXPECTED.confirm);
  const finished = await until('l’effacement se termine (la carte « Dernier effacement » apparaît)', 180_000, async () => {
    const now = await window.privacy();
    const closed = now.dialog === null || !now.dialog.open;
    if (closed && !now.busy && now.last.length === 0 && now.warnings.some((text) => shows(text, EXPECTED.listChanged))) {
      // La page a vu que ce qui allait partir n'est plus ce qui a été listé : rien n'a été effacé.
      fail(`la confirmation s'est refermée d'elle-même (« ${EXPECTED.listChanged} ») : la vue a changé entre la liste et le clic, rien n'a été effacé\n${describeScreen(now)}`);
    }
    return !now.busy && now.last.length > 0 && closed ? now : null;
  }, 300);
  await window.capture('apres');
  if (!shows(finished.lastTitle, EXPECTED.lastTitle) || !/\d{1,2}\s*h\s*\d{2}/.test(finished.lastTitle)) {
    fail(`attendu : le titre de la carte est « ${EXPECTED.lastTitle}, à 14 h 32 » (avec l'heure) ; vu : « ${finished.lastTitle} »`);
  }
  if (finished.last.length !== 1) {
    fail(`attendu : une seule ligne dans « Dernier effacement » ; vu : ${finished.last.length}\n${describeScreen(finished)}`);
  }
  const [line] = finished.last;
  const named = row.block === EXPECTED.windows ? line.trace === row.title : line.block.includes(row.title) && line.trace === EXPECTED.history;
  if (!named) {
    fail(`attendu : « Dernier effacement » parle de « ${row.block} / ${row.title} » ; vu : « ${line.block} / ${line.trace} »\n${describeScreen(finished)}`);
  }
  if (!OUTCOME_TEXTS.includes(line.outcome)) {
    fail(`attendu : une issue parmi « ${OUTCOME_TEXTS.join(' », « ')} » ; vu : « ${line.outcome} »`);
  }
  const mine = finished.rows.find((one) => one.block === row.block && one.title === row.title);
  // Sous la ligne, l'issue est une phrase (« En cours d'utilisation. Ferme Chrome et recommence. ») qui commence par les deux mots de la carte.
  if (mine !== undefined && mine.outcome !== '' && !mine.outcome.startsWith(line.outcome)) {
    fail(`attendu : la ligne commence par l'issue de la carte ; vu : la ligne « ${mine.outcome} », la carte « ${line.outcome} »`);
  }
  note(`issue de « ${row.block} / ${row.title} » : « ${line.outcome} »${line.help === '' ? '' : ` (${line.help})`}${mine === undefined ? ' ; la ligne n’est plus dans la page' : ''}`);
  return { outcome: line.outcome, line, screen: finished };
}

/** Compare l'issue lue à celle qu'attend le scénario. */
function checkOutcome(scenario, outcome) {
  const E = EXPECTED.outcomes;
  const wanted = {
    erased: [E.erased],
    inUse: [E.inUse],
    changed: [E.changed],
    // Un piège : tout sauf « Effacé » pour la ligne qui porte le piège.
    refused: [E.nothing, E.inUse, E.changed, E.partial, E.failed],
    // Une mesure (le presse-papiers) : l'issue est journalisée, le script dit si la machine est vide.
    measure: OUTCOME_TEXTS,
  }[scenario.outcome];
  if (!STRICT) {
    note(`(démo) issue attendue sur la machine : ${wanted.join(' ou ')} ; lue : ${outcome}`);
    return;
  }
  if (!wanted.includes(outcome)) {
    fail(`attendu : l'issue « ${wanted.join(' » ou « ')} » ; vu : « ${outcome} »`);
  }
  done(`l'issue est « ${outcome} »`);
}

// ── Les scénarios ──────────────────────────────────────────────────────────

/**
 * `before` : l'étape de préparation, avec ses paramètres ; `after` : celle qui relit la machine ; `target` : la
 * ligne visée ; `row` : « check » (la page propose une case), « inert » (pas de case : le piège est vu avant
 * tout clic) ou « maybe » (un piège : pas de case, ou une case dont l'effacement ne doit pas réussir) ;
 * `outcome` : ce que l'issue doit être ; `between` : une étape lancée entre le clic sur la case et celui sur le
 * bouton.
 */
const SCENARIOS = [
  { name: 'profil', title: '1. le faux profil Chrome', before: ['profil-preparer'], after: 'profil-verifier', target: 'chrome', row: 'check', outcome: 'erased' },
  { name: 'verrou-fichier', title: '2. un fichier du lot tenu ouvert', before: ['verrou-fichier-preparer'], after: 'verrou-fichier-verifier', target: 'chrome', row: 'check', outcome: 'inUse' },
  { name: 'verrou-lockfile-chromium', title: '2. un lockfile tenu comme Chromium', before: ['verrou-lockfile-preparer', '-Partage', 'Chromium'], after: 'verrou-lockfile-verifier', target: 'chrome', row: 'inert' },
  { name: 'verrou-lockfile-aucun', title: '2. un lockfile tenu sans aucun partage', before: ['verrou-lockfile-preparer', '-Partage', 'Aucun'], after: 'verrou-lockfile-verifier', target: 'chrome', row: 'inert' },
  { name: 'jonction', title: '3. une jonction à la place du profil', before: ['jonction-preparer'], after: 'jonction-verifier', target: 'chrome', row: 'maybe', outcome: 'refused' },
  { name: 'lien-physique', title: '3. un lien physique sur History', before: ['lien-physique-preparer'], after: 'lien-physique-verifier', target: 'chrome', row: 'maybe', outcome: 'refused' },
  { name: 'lien-registre', title: '3. un lien de registre sur RunMRU', before: ['lien-registre-preparer'], after: 'lien-registre-verifier', target: 'typed', row: 'maybe', outcome: 'measure' },
  { name: 'profil-remplace', title: '4. un profil remplacé entre la vue et le clic', before: ['profil-remplace-preparer'], between: ['profil-remplace-remplacer'], after: 'profil-remplace-verifier', target: 'chrome', row: 'check', outcome: 'changed' },
  { name: 'registre', title: '5. le registre : ce qui a été tapé', before: ['registre-preparer'], after: 'registre-verifier', target: 'typed', row: 'check', outcome: 'erased' },
  { name: 'registre-explorateur', title: '5. le registre, une fenêtre de l’Explorateur ouverte', before: ['registre-explorateur-preparer'], after: 'registre-explorateur-verifier', target: 'typed', row: 'check', outcome: 'erased' },
  { name: 'recents', title: '6. les fichiers et dossiers récents', before: ['recents-preparer'], after: 'recents-verifier', target: 'recent', row: 'check', outcome: 'erased' },
  { name: 'presse-papiers', title: '7. le presse-papiers', before: ['presse-papiers-preparer'], after: 'presse-papiers-verifier', target: 'clipboard', row: 'check', outcome: 'measure' },
];

const unproven = [];
const failures = [];

async function withWindow(scenario, body) {
  const port = await freePort();
  const asUser = elevated();
  const run = launch(options.programme, port, asUser);
  let window = null;
  try {
    const page = await findPage(port, run, 90_000);
    done(`la fenêtre s’ouvre sur ${page.url}`);
    window = new Window(await Session.open(page.webSocketDebuggerUrl), scenario.name);
    await window.listen();
    const result = await body(window);
    if (window.problems.length > 0) {
      fail(`la page signale ${window.problems.length} erreur(s)\n${window.problems.join('\n')}`);
    }
    return result;
  } catch (error) {
    if (window !== null) {
      const screen = await window.privacy().catch(() => null);
      if (screen !== null) {
        say('la fenêtre affichait :');
        quote(describeScreen(screen));
      }
      await window.capture('echec');
    }
    throw error;
  } finally {
    window?.session.close();
    await stop(run);
  }
}

async function playScenario(scenario) {
  const target = TARGETS[scenario.target];
  await runStep(scenario.before[0], scenario.before.slice(1));
  await withWindow(scenario, async (window) => {
    const screen = await openPrivacy(window);
    done('la page « Confidentialité » est chargée');
    quote(describeScreen(screen), 60);
    await window.capture('vue');
    const row = pickRow(screen, target);

    if (scenario.row === 'inert' || scenario.row === 'maybe') {
      if (scenario.row === 'inert' && STRICT) {
        // La ligne doit être là, sans case, et la carte de Chrome doit dire pourquoi (« Ferme Chrome… ») : une ligne
        // absente est un échec (un défaut qui ferait disparaître le bloc passerait sinon pour une réussite).
        if (row === null) {
          fail(`attendu : ${target.label} est présente et inerte, avec « ${EXPECTED.running} » ; vu : la page ne la montre pas\n${describeScreen(screen)}`);
        }
        if (row.kind !== 'inert') {
          fail(`attendu : ${target.label} est inerte (pas de case) ; vu : ${row.kind === 'check' ? `une case « ${row.label} »` : `une ligne ${row.kind}`}\n${describeScreen(screen)}`);
        }
        const warning = screen.running.find((one) => one.block === EXPECTED.chrome);
        if (warning === undefined || !shows(warning.text, EXPECTED.running)) {
          fail(`attendu : la carte de Chrome dit « ${EXPECTED.running} » ; vu : ${warning === undefined ? 'aucune phrase' : `« ${warning.text} »`}\n${describeScreen(screen)}`);
        }
        done(`${target.label} est présente, sans case, et la carte de Chrome dit « ${warning.text.split('.')[0]}. »`);
        note(`mode de partage du lockfile : voir la ligne « FAIT : lockfile tenu » de l'étape de préparation`);
        return;
      }
      if (row === null || row.kind !== 'check') {
        done(
          row === null
            ? `la page ne propose pas ${target.label} (rien à cocher)`
            : `${target.label} est sans case (${row.kind === 'inert' ? 'inerte' : row.kind}) : « ${row.sub || row.text} »`,
        );
        return;
      }
      if (scenario.row === 'inert') {
        note(`(démo) ${target.label} a une case ; sur la machine d'essai elle devrait être inerte`);
        return;
      }
      note(`${target.label} a une case : on la coche, l'effacement ne doit pas réussir`);
    } else if (row === null || row.kind !== 'check') {
      fail(`attendu : ${target.label} avec une case ; vu : ${row === null ? 'aucune ligne' : `une ligne ${row.kind} « ${row.sub || row.text} »`}\n${describeScreen(screen)}`);
    }

    const between = scenario.between === undefined ? undefined : () => runStep(scenario.between[0], scenario.between.slice(1));
    const result = await erase(window, row, between);
    checkOutcome(scenario, result.outcome);
    if (scenario.name === 'presse-papiers') {
      note(`issue affichée pour le presse-papiers : « ${result.outcome} »`);
    }
    if (scenario.name === 'recents') {
      // Ce qui mesure SHAddToRecentDocs(NULL) : le cœur relit Recent jusqu'à 5 fois à 100 ms après l'appel, et dit « Effacé »
      // seulement s'il n'y voit plus aucun raccourci dans ce délai. Le script, lui, compte les raccourcis juste après.
      note(`issue affichée pour les récents : « ${result.outcome} » (« ${EXPECTED.outcomes.erased} » = zéro raccourci vu par le cœur dans ses 5 relectures à 100 ms ; « ${EXPECTED.outcomes.partial} » ou « ${EXPECTED.outcomes.failed} » = il en restait)`);
    }
  }).catch((error) => {
    if (scenario.between !== undefined && error instanceof Failure && error.message.includes('a relu la vue')) {
      unproven.push(`${scenario.name} : la page a relu la vue avant le clic (profil remplacé : « Ce profil a changé » non prouvé)`);
    }
    throw error;
  }).then(
    () => undefined,
    async (error) => {
      // La fenêtre a échoué : la machine est quand même relue, en mode « constat » (ce qui a disparu ou changé, sans
      // verdict), pour savoir quels fichiers sont partis avant que le nettoyage ne les efface. Puis l'erreur repart.
      try {
        await runStep(scenario.after, ['-Constat']);
      } catch (constat) {
        note(`le constat de la machine n'a pas pu être fait : ${constat.message.split('\n')[0]}`);
      }
      throw error;
    },
  );
  await runStep(scenario.after);
}

// ── Le vrai Chrome ─────────────────────────────────────────────────────────

async function playRealChrome() {
  const scenario = { name: 'chrome-reel', outcome: 'erased' };
  const created = await runStep('chrome-creer', [], { timeout: 240_000, tolerate: [10] });
  if (created === 10) {
    unproven.push('chrome-reel : Chrome n’est pas installé sur cette machine (preuve 8 non faite)');
    note('Chrome n’est pas installé : la preuve du vrai Chrome n’est pas faite, l’essai continue');
    return;
  }
  // Chrome ouvert : sa ligne doit être inerte (le lockfile d'un vrai Chromium est vu tenu). Un écart ici n'empêche pas
  // la suite (l'effacement réel) : il est gardé, et relancé à la fin.
  let opened = null;
  await runStep('chrome-ouvrir');
  await withWindow({ ...scenario, name: 'chrome-ouvert' }, async (window) => {
    const screen = await openPrivacy(window);
    quote(describeScreen(screen), 60);
    await window.capture('chrome-ouvert');
    const rows = screen.rows.filter(TARGETS.chromeReal.pick);
    const tickable = rows.filter((row) => row.kind === 'check');
    const warned = screen.running.some((one) => one.block === EXPECTED.chrome);
    if (STRICT && tickable.length > 0) {
      fail(`attendu : Chrome est ouvert, sa ligne est inerte (le lockfile d'un vrai Chromium est vu tenu) ; vu : une case « ${tickable[0].label} »\n${describeScreen(screen)}`);
    }
    if (STRICT && !warned) {
      fail(`attendu : la carte de Chrome dit « ${EXPECTED.running} » ; vu : aucune phrase d'avertissement\n${describeScreen(screen)}`);
    }
    done(`Chrome ouvert : ${rows.length} ligne(s) de Chrome dont ${tickable.length} avec case ; avertissement : ${warned ? screen.running.find((one) => one.block === EXPECTED.chrome).text : 'aucun'}`);
  }).catch((error) => {
    opened = error;
    say(`✗ chrome-ouvert : ${error.message.split('\n')[0]}`);
    if (error.message.includes('\n')) {
      quote(error.message.split('\n').slice(1).join('\n'));
    }
  });
  await runStep('chrome-fermer');
  // Chrome fermé : la fenêtre efface son historique ; Bookmarks, Login Data, Preferences, Local State ne bougent pas.
  const erased = withWindow(scenario, async (window) => {
    const screen = await openPrivacy(window);
    quote(describeScreen(screen), 60);
    const row = pickRow(screen, TARGETS.chromeReal);
    if (row === null || row.kind !== 'check') {
      fail(`attendu : ${TARGETS.chromeReal.label} avec une case, Chrome étant fermé ; vu : ${row === null ? 'aucune ligne' : `une ligne ${row.kind} « ${row.sub || row.text} »`}\n${describeScreen(screen)}`);
    }
    await runStep('chrome-avant');
    const result = await erase(window, row);
    checkOutcome(scenario, result.outcome);
  });
  try {
    await erased;
  } catch (error) {
    // La fenêtre a échoué : ce que Chrome a perdu ou gardé est relevé quand même (constat, sans verdict).
    if (opened !== null) {
      failures.push(`chrome-ouvert : ${opened.message.split('\n')[0]}`);
    }
    try {
      await runStep('chrome-apres', ['-Constat'], { timeout: 120_000 });
    } catch (constat) {
      note(`le constat de la machine n'a pas pu être fait : ${constat.message.split('\n')[0]}`);
    }
    throw error;
  }
  await runStep('chrome-apres', [], { timeout: 300_000 });
  if (opened !== null) {
    throw opened;
  }
}

// ── Programme ──────────────────────────────────────────────────────────────

async function main() {
  if (options.programme === undefined) {
    console.error('Usage : node scripts/essai-confidentialite.mjs --programme <shinku.exe> [--captures <dossier>]');
    return 2;
  }
  if (!existsSync(options.programme)) {
    console.error(`Programme introuvable : ${options.programme}`);
    return 2;
  }
  if (options.captures !== undefined) {
    mkdirSync(options.captures, { recursive: true });
  }
  const only = options.seulement === undefined ? null : options.seulement.split(',').map((name) => name.trim());
  const all = [...SCENARIOS.map((scenario) => scenario.name), 'chrome-reel'];
  for (const name of only ?? []) {
    if (!all.includes(name)) {
      console.error(`Scénario inconnu : ${name}. Les scénarios : ${all.join(', ')}`);
      return 2;
    }
  }
  say(`sur ${os.version()} (${os.release()})${WITHOUT_SCRIPT ? ' ; démo : sans le script PowerShell' : ''}`);

  const chosen = (name) => only === null || only.includes(name);
  const played = [];
  const attempt = async (name, title, play) => {
    if (!chosen(name)) {
      return;
    }
    say(`══ ${title}`);
    try {
      await play();
      played.push([name, true]);
      done(`${name} : fait`);
    } catch (error) {
      played.push([name, false]);
      failures.push(`${name} : ${error.message.split('\n')[0]}`);
      say(`✗ ${name} : ${error instanceof Failure ? error.message.split('\n')[0] : `erreur de l’essai : ${error.stack}`}`);
      if (error instanceof Failure && error.message.includes('\n')) {
        quote(error.message.split('\n').slice(1).join('\n'));
      }
    }
    // Après chaque scénario, réussi ou non : ce qu'il a posé (registre, récents, épinglé, fenêtre, User Data d'essai, processus
    // qui tient un fichier) est remis en place ; aucun scénario ne dépend de ce que le précédent a laissé.
    try {
      await runStep('nettoyer-scenario', [], { timeout: 120_000 });
    } catch (cleaning) {
      failures.push(`nettoyage après ${name} : ${cleaning.message.split('\n')[0]}`);
      say(`✗ le nettoyage après ${name} a échoué : ${cleaning.message.split('\n')[0]}`);
    }
  };

  try {
    for (const scenario of SCENARIOS) {
      await attempt(scenario.name, scenario.title, () => playScenario(scenario));
    }
    await attempt('chrome-reel', '8. le vrai Chrome, s’il est installé', playRealChrome);
  } finally {
    // Quoi qu'il arrive : shinku.exe ne tourne plus (comme l'essai permanent : l'arbre du lanceur, puis le programme lui-même),
    // puis la machine d'essai est remise en place (User Data d'origine, registre, récents).
    if (onWindows && !WITHOUT_SCRIPT) {
      spawnSync('taskkill', ['/im', path.basename(options.programme), '/t', '/f'], { stdio: 'ignore' });
    }
    try {
      await runStep('nettoyer', [], { timeout: 120_000 });
    } catch (cleaning) {
      failures.push(`nettoyage : ${cleaning.message.split('\n')[0]}`);
      say(`✗ le nettoyage final a échoué : ${cleaning.message.split('\n')[0]}`);
    }
  }

  for (const line of unproven) {
    note(`non prouvé : ${line}`);
  }
  say(`scénarios : ${played.filter(([, ok]) => ok).length} réussi(s), ${played.filter(([, ok]) => !ok).length} échoué(s)`);
  if (failures.length > 0) {
    say('Essai manqué :');
    quote(failures.join('\n'));
    return 1;
  }
  if (unproven.length > 0) {
    say(`Essai réussi, mais ${unproven.length} point(s) non tranché(s) : voir « non prouvé » ci-dessus.`);
    return 0;
  }
  say('Essai réussi.');
  return 0;
}

process.exitCode = await main();
