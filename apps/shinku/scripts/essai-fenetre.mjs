// Essai de la fenêtre : le vrai shinku.exe s'ouvre-t-il, et marche-t-il ?
//
// Les autres tests tournent sans fenêtre. Celui-ci lance le programme tel
// qu'il est installé, sous Windows, et regarde sa page par le port de
// débogage de WebView2 : il voit ce que verrait la personne qui ouvre Shinku
// et clique là où elle cliquerait. Il existe parce qu'une première version,
// dont tous les autres tests passaient, ne démarrait pas une fois installée.
//
// Le port de débogage est une fonction de WebView2, activée de l'extérieur
// par une variable d'environnement : shinku.exe n'a aucune option d'essai.
//
// Shinku est essayé avec les droits ordinaires d'un utilisateur, comme sur le
// PC de quelqu'un. Les machines de l'intégration continue font tout tourner en
// administrateur : l'essai y lance le programme par `lancer-en-utilisateur.ps1`.
//
// À lancer sur une machine d'essai (celle de l'intégration continue) : il
// mène une vraie analyse du disque et change un instant le mode d'affichage
// enregistré.
//
//   node scripts/essai-fenetre.mjs --programme "C:\Program Files\Shinku\shinku.exe" --captures captures
//
// Sans dépendance : Node 22 suffit.

import { spawn, spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, statfsSync, writeFileSync } from 'node:fs';
import { createServer } from 'node:net';
import os from 'node:os';
import path from 'node:path';
import { setTimeout as sleep } from 'node:timers/promises';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';

const { values: options } = parseArgs({
  options: {
    /** Le programme à lancer. */
    programme: { type: 'string' },
    /** Dossier où écrire les captures d'écran. */
    captures: { type: 'string' },
    /** Durée laissée à l'analyse, en secondes. */
    'attente-analyse': { type: 'string', default: '600' },
    /** Début de l'adresse de la page (pour mettre l'essai au point hors de Windows). */
    adresse: { type: 'string' },
  },
});

const onWindows = process.platform === 'win32';
const LAUNCHER = fileURLToPath(new URL('lancer-en-utilisateur.ps1', import.meta.url));
const SCAN_BUDGET = Number(options['attente-analyse']) * 1000;

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

/** Écrit un texte en retrait : ce que la fenêtre affiche, un diagnostic. */
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

/** Une vérification manquée : l'essai s'arrête et le dit. */
class Failure extends Error {}

function fail(message) {
  throw new Failure(message);
}

/**
 * Vrai si l'écran montre ce texte. Les majuscules ne comptent pas : la feuille
 * de style met certains titres en capitales.
 */
function shows(screen, expected) {
  return screen.toLocaleLowerCase('fr').includes(expected.toLocaleLowerCase('fr'));
}

/** Un nom d'écran, écrit pour servir de nom de fichier. */
function slug(label) {
  return label
    .normalize('NFD')
    .replace(/[\u0300-\u036f]/g, '')
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '-');
}

/** Répète une question jusqu'à obtenir une réponse, ou échoue passé le délai. */
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

/** Vrai si cet essai tourne avec les droits d'un administrateur. */
function elevated() {
  if (!onWindows) {
    return false;
  }
  // Niveau d'intégrité élevé ou système.
  const groups = spawnSync('whoami', ['/groups', '/fo', 'csv', '/nh'], { encoding: 'utf8' }).stdout ?? '';
  return /S-1-16-(12288|16384)/.test(groups);
}

/**
 * Lance le programme, WebView2 ouvrant son port de débogage.
 *
 * Quand l'essai tourne en administrateur (les machines de l'intégration
 * continue), le programme est lancé avec les droits ordinaires d'un
 * utilisateur, par `lancer-en-utilisateur.ps1` : c'est ainsi que Shinku tourne
 * sur le PC de quelqu'un.
 */
function launch(program, port, asUser) {
  /** `pid` est le numéro du programme lui-même, connu une fois qu'il tourne. */
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
    // Windows a refusé de créer le processus (manifeste invalide, par exemple).
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
  // « close » plutôt que « exit » : tout ce que le lanceur a écrit est alors lu.
  run.child.once('close', (code, signal) => {
    run.exit = { code, signal };
  });
  return run;
}

/** Arrête le programme et les processus de WebView2 qu'il a lancés. */
function stop(run) {
  if (run.exit !== null || run.child?.pid === undefined) {
    return;
  }
  if (onWindows) {
    spawnSync('taskkill', ['/pid', String(run.child.pid), '/t', '/f'], { stdio: 'ignore' });
  } else {
    run.child.kill('SIGKILL');
  }
}

function powershell(script, timeout = 30_000) {
  const result = spawnSync('powershell', ['-NoProfile', '-NonInteractive', '-Command', script], {
    encoding: 'utf8',
    timeout,
  });
  return `${result.stdout ?? ''}${result.stderr ?? ''}`.trim();
}

const quoted = (text) => `'${text.replaceAll("'", "''")}'`;

/** La raison que donne Windows quand il refuse de lancer un programme. */
function refusalReason(program) {
  if (!onWindows) {
    return '';
  }
  return powershell(
    `try { $p = Start-Process -FilePath ${quoted(program)} -PassThru; Stop-Process -Id $p.Id -Force; 'Windows accepte pourtant de le lancer.' } catch { $_.Exception.Message }`,
  );
}

/**
 * Le texte des fenêtres de Windows ouvertes par le programme : quand Shinku
 * ne peut pas démarrer, il l'explique dans un message, et c'est lui qu'on lit.
 */
function windowTexts(pid) {
  if (!onWindows) {
    return '';
  }
  return powershell(
    `Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
     $automation = [System.Windows.Automation.AutomationElement]
     $mine = New-Object System.Windows.Automation.PropertyCondition($automation::ProcessIdProperty, ${pid})
     $all = [System.Windows.Automation.Condition]::TrueCondition
     foreach ($window in $automation::RootElement.FindAll('Children', $mine)) {
       "fenetre : $($window.Current.Name)"
       foreach ($part in $window.FindAll('Descendants', $all)) {
         if ($part.Current.Name) { "  $($part.Current.ControlType.ProgrammaticName) : $($part.Current.Name)" }
       }
     }`,
  );
}

/**
 * Les processus du moteur web et les ports qu'ils écoutent : de quoi comprendre
 * pourquoi le port de débogage reste muet.
 */
function engineState() {
  if (!onWindows) {
    return '';
  }
  return powershell(
    `$engines = Get-CimInstance Win32_Process -Filter "name = 'msedgewebview2.exe'"
     foreach ($engine in $engines) {
       if ($engine.CommandLine -notmatch '--type=') { "moteur $($engine.ProcessId), lance par $($engine.ParentProcessId) : $($engine.CommandLine)" }
     }
     $ids = @($engines | ForEach-Object { $_.ProcessId })
     foreach ($port in Get-NetTCPConnection -State Listen -ErrorAction SilentlyContinue) {
       if ($ids -contains $port.OwningProcess) { "port ecoute : $($port.LocalAddress):$($port.LocalPort) (processus $($port.OwningProcess))" }
     }`,
  );
}

/** Photographie le bureau : la fenêtre telle que Windows la montre, cadre compris. */
function captureDesktop(file) {
  if (!onWindows) {
    return;
  }
  powershell(
    `Add-Type -AssemblyName System.Windows.Forms, System.Drawing
     $screen = [System.Windows.Forms.SystemInformation]::VirtualScreen
     $image = New-Object System.Drawing.Bitmap $screen.Width, $screen.Height
     $drawing = [System.Drawing.Graphics]::FromImage($image)
     $drawing.CopyFromScreen($screen.Left, $screen.Top, 0, 0, $image.Size)
     $image.Save(${quoted(file)})`,
  );
}

// ── La page, par le port de débogage ───────────────────────────────────────

/** Attend que la fenêtre ait ouvert la page de l'interface. */
async function findPage(port, run, timeout) {
  const expected = (url) =>
    options.adresse !== undefined ? url.startsWith(options.adresse) : /^https?:\/\/tauri\.localhost\//.test(url);
  let seen = 'le port de débogage ne répond pas';
  let silence = '';
  const deadline = Date.now() + timeout;
  while (Date.now() < deadline) {
    if (run.error !== null) {
      const reason = refusalReason(options.programme);
      fail(`Windows refuse de lancer le programme (${run.error.message}).${reason === '' ? '' : `\n${reason}`}`);
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
      // Le moteur web n'a pas encore ouvert son port.
      silence = `${error.message}${error.cause ? ` (${error.cause.code ?? error.cause.message})` : ''}`;
    }
    await sleep(250);
  }
  const engine = engineState();
  fail(
    `la fenêtre n’a pas ouvert sa page en ${Math.round(timeout / 1000)} s (${seen})` +
      `\nport ${port}, dernière réponse : ${silence || 'aucune'}${engine === '' ? '' : `\n${engine}`}`,
  );
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
      socket.addEventListener('error', () => reject(new Failure('le port de débogage refuse la connexion')), {
        once: true,
      });
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

// Les fonctions ci-dessous s'exécutent dans la page : elles ne voient rien de
// ce fichier.

/** L'état de l'interface, tel qu'il s'affiche. */
function readScreen() {
  const text = (element) => (element?.innerText ?? '').replace(/[ \u00a0\u202f]+/g, ' ').trim();
  const toast = document.querySelector('.toast.show');
  return {
    boot:
      document.querySelector('.side') !== null
        ? 'ready'
        : document.querySelector('.loader [role=alert]') !== null
          ? 'failed'
          : 'waiting',
    current: text(document.querySelector('button.nav[aria-current=page] span')),
    main: text(document.querySelector('.main')),
    whole: text(document.body),
    title: text(document.querySelector('.main h1')),
    toast: toast === null ? '' : text(toast),
    theme: document.documentElement.dataset.theme ?? '',
  };
}

/** Où se trouve un bouton, et ce qu'un clic à cet endroit toucherait. */
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
    disabled: element.disabled === true,
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

/** Ce qui prouve que la feuille de style et les polices sont arrivées. */
function readLayout() {
  const box = (selector) => {
    const found = document.querySelector(selector)?.getBoundingClientRect();
    return found === undefined ? null : { width: Math.round(found.width), height: Math.round(found.height) };
  };
  return {
    width: window.innerWidth,
    height: window.innerHeight,
    scale: window.devicePixelRatio,
    visibility: document.visibilityState,
    background: getComputedStyle(document.body).backgroundColor,
    font: getComputedStyle(document.body).fontFamily,
    fonts: [...document.fonts].filter((font) => font.status === 'loaded').map((font) => font.family),
    sidebar: box('.side'),
    orb: box('.dock .orb'),
    agent: navigator.userAgent,
  };
}

/** Vrai tant qu'un écran entre en fondu ou qu'un élément glisse vers sa place. */
function animating() {
  return document.getAnimations().some((animation) => {
    // Les animations sans fin (l'orbe qui respire) ne comptent pas.
    const end = animation.effect?.getComputedTiming().endTime;
    return animation.playState === 'running' && Number.isFinite(end);
  });
}

/** L'avancement de l'analyse, tel que l'écran « Grand vide » le montre. */
function readScan() {
  const text = (element) => (element?.innerText ?? '').replace(/\s+/g, ' ').trim();
  return {
    status: text(document.querySelector('.middle [role=status]')),
    counter: text(document.querySelector('.middle .big')),
    walked: text(document.querySelector('.middle .cap')),
    rows: [...document.querySelectorAll('.middle li.row')].map((row) => text(row)),
    finished: document.querySelector('.dock .orb') !== null,
    home: document.querySelector('.hero') !== null,
  };
}

// ── L'essai ────────────────────────────────────────────────────────────────

/** La fenêtre en cours d'essai. */
class Window {
  /** Ce que la page a signalé d'anormal : erreurs de console, ressources refusées. */
  problems = [];
  /** Ce qu'elle a écrit d'autre dans sa console. */
  notes = [];
  /** Vrai tant que la page reçoit les gestes de souris envoyés par le port de débogage. */
  mouse = true;
  #shots = 0;
  #pictures = true;

  constructor(session) {
    this.session = session;
    session.on('Runtime.consoleAPICalled', ({ type, args }) => {
      const text = args.map((value) => value.value ?? value.description ?? value.type).join(' ');
      (type === 'error' || type === 'assert' ? this.problems : this.notes).push(`console.${type} : ${text}`);
    });
    session.on('Runtime.exceptionThrown', ({ exceptionDetails }) => {
      this.problems.push(`erreur non rattrapée : ${exceptionDetails.exception?.description ?? exceptionDetails.text}`);
    });
    session.on('Log.entryAdded', ({ entry }) => {
      const where = entry.url ? ` (${entry.url})` : '';
      (entry.level === 'error' ? this.problems : this.notes).push(`${entry.source}, ${entry.level} : ${entry.text}${where}`);
    });
  }

  /** Branche le journal de la page. Ce qu'elle a déjà écrit est rejoué. */
  async listen() {
    await this.session.send('Runtime.enable');
    await this.session.send('Log.enable');
    await this.session.send('Page.enable');
    // Un premier geste sans effet : si la page ne répond pas à la souris
    // (session sans bureau), les clics seront déclenchés dans la page.
    try {
      await this.session.send('Input.dispatchMouseEvent', { type: 'mouseMoved', x: 1, y: 1 }, 5000);
    } catch {
      this.mouse = false;
      say('  la page ne reçoit pas la souris : les clics seront déclenchés dans la page');
    }
  }

  screen() {
    return this.session.run(readScreen);
  }

  /** Échoue si la page a signalé une erreur ou affiche un message d'erreur. */
  async healthy(after) {
    const screen = await this.screen();
    if (screen.toast !== '') {
      fail(`${after} : la fenêtre affiche le message « ${screen.toast} »`);
    }
    if (this.problems.length > 0) {
      fail(`${after} : la page signale ${this.problems.length} erreur(s)\n${this.problems.join('\n')}`);
    }
    return screen;
  }

  /** Clique sur un bouton comme le ferait la souris, une fois qu'il ne bouge plus. */
  async click(what, selector, label = null) {
    let before = null;
    const target = await until(`le bouton « ${what} » est prêt`, 20_000, async () => {
      const found = await this.session.run(locate, selector, label);
      if (found === null || found.disabled) {
        before = null;
        return null;
      }
      const still = before !== null && Math.abs(before.x - found.x) < 0.5 && Math.abs(before.y - found.y) < 0.5;
      before = found;
      return still ? found : null;
    });
    if (!target.reached) {
      fail(`le bouton « ${what} » est recouvert par ${target.cover} : un clic ne l’atteindrait pas`);
    }
    if (!this.mouse) {
      await this.session.run(clickInPage, selector, label);
      return;
    }
    const point = { x: target.x, y: target.y };
    const button = { ...point, button: 'left', clickCount: 1 };
    await this.session.send('Input.dispatchMouseEvent', { type: 'mouseMoved', ...point });
    await this.session.send('Input.dispatchMouseEvent', { type: 'mousePressed', ...button });
    await this.session.send('Input.dispatchMouseEvent', { type: 'mouseReleased', ...button });
  }

  /** Ouvre un module par le menu, attend que son contenu soit chargé, et le rend. */
  async open(label) {
    await this.click(label, 'button.nav', label);
    let before = null;
    const screen = await until(`l’écran « ${label} » est affiché`, 30_000, async () => {
      const now = await this.screen();
      const ready = now.current === label && now.title !== '' && !shows(now.main, 'Chargement…');
      const still = ready && before === now.main;
      before = ready ? now.main : null;
      return still ? now : null;
    }, 300);
    await this.healthy(`écran « ${label} »`);
    return screen;
  }

  /** Enregistre l'image de la page. */
  async capture(name) {
    if (options.captures === undefined || !this.#pictures) {
      return;
    }
    this.#shots += 1;
    const file = path.join(options.captures, `${String(this.#shots).padStart(2, '0')}-${name}.png`);
    // Les écrans entrent en fondu : on attend qu'ils soient en place, pour
    // que l'image montre ce que la personne voit. Pendant une analyse, la
    // barre d'avancement ne s'arrête jamais : on n'attend pas plus de 2,5 s.
    const settled = Date.now() + 2500;
    while (Date.now() < settled && (await this.session.run(animating).catch(() => false))) {
      await sleep(100);
    }
    try {
      const { data } = await this.session.send('Page.captureScreenshot', { format: 'png' }, 15_000);
      writeFileSync(file, Buffer.from(data, 'base64'));
    } catch (error) {
      // Une page qui ne se laisse pas photographier une fois ne le fera pas mieux ensuite.
      this.#pictures = false;
      say(`  capture de la page impossible (${error.message})`);
    }
  }
}

async function checkStart(window) {
  const screen = await until('l’interface a reçu la réponse du cœur', 45_000, async () => {
    const now = await window.screen();
    return now.boot === 'waiting' ? null : now;
  });
  if (screen.boot === 'failed') {
    fail(`l’interface n’a pas pu parler au cœur :\n${screen.whole}`);
  }
  done('l’interface a reçu la réponse du cœur et affiche l’accueil');
  quote(screen.whole);

  for (const expected of ['Grand vide', 'Analyser', 'libres sur', 'Paramètres']) {
    if (!shows(screen.whole, expected)) {
      fail(`l’accueil ne montre pas « ${expected} »`);
    }
  }
  const layout = await window.session.run(readLayout);
  say(`  page de ${layout.width} × ${layout.height} (échelle ${layout.scale}, ${layout.visibility}), fond ${layout.background}`);
  say(`  polices chargées : ${[...new Set(layout.fonts)].join(', ') || 'aucune'}`);
  say(`  moteur : ${layout.agent}`);
  if (layout.sidebar === null || layout.sidebar.width < 150 || layout.orb === null || layout.orb.width < 60) {
    fail(`la mise en page n’est pas appliquée : ${JSON.stringify(layout)}`);
  }
  if (onWindows) {
    const disk = statfsSync(process.env.SystemDrive ? `${process.env.SystemDrive}\\` : 'C:\\');
    const gigabytes = (blocks) => ((blocks * disk.bsize) / 1024 ** 3).toFixed(1);
    say(`  disque d’après Windows : ${gigabytes(disk.bavail)} Go libres sur ${gigabytes(disk.blocks)} Go`);
  }
  await window.healthy('au démarrage');
  await window.capture('accueil');
}

async function checkScan(window) {
  await window.click('Analyser', '.dock .orb', 'Analyser');
  const begun = Date.now();
  await until('l’analyse a démarré', 20_000, async () => {
    const scan = await window.session.run(readScan);
    return scan.status !== '';
  });
  done('un clic sur « Analyser » lance l’analyse');
  await window.capture('analyse');

  let told = Date.now();
  const end = await until('l’analyse est terminée', SCAN_BUDGET, async () => {
    const scan = await window.session.run(readScan);
    if (scan.home) {
      const screen = await window.screen();
      fail(`l’analyse s’est arrêtée sans résultats :\n${screen.whole}`);
    }
    if (Date.now() - told >= 15_000 && !scan.finished) {
      told = Date.now();
      say(`  ${scan.status} : ${scan.counter} à libérer, ${scan.walked}`);
    }
    return scan.finished ? scan : null;
  }, 500);
  done(`l’analyse se termine en ${Math.round((Date.now() - begun) / 1000)} s : ${end.walked}`);
  quote(end.rows.join('\n'));
  await window.healthy('après l’analyse');

  await window.click('Voir les résultats', '.dock .orb', 'Voir les résultats');
  const results = await until('les résultats sont affichés', 20_000, async () => {
    const screen = await window.screen();
    return shows(screen.main, 'Le grand vide peut libérer') && !shows(screen.main, 'Chargement…') ? screen : null;
  });
  done('« Voir les résultats » affiche les résultats');
  quote(results.main);
  await window.healthy('sur les résultats');
  await window.capture('resultats');
}

async function checkModules(window) {
  /** Chaque module, et un texte que son écran montre quand le cœur a répondu. */
  const modules = [
    ['Fichiers inutiles', 'Ces fichiers se recréent tout seuls'],
    ['Doublons', 'Le nettoyage arrive dans une prochaine version'],
    ['Gros fichiers', 'de plus de'],
    ['Corbeille', 'Le vidage arrive dans une prochaine version'],
    ['Démarrage', 'Ce module arrive dans une prochaine version'],
    ['Mode jeu', 'Ce module arrive dans une prochaine version'],
    ['Désinstallation', 'Ce module arrive dans une prochaine version'],
    ['Confidentialité', 'Ce module arrive dans une prochaine version'],
  ];
  for (const [label, expected] of modules) {
    const screen = await window.open(label);
    if (!shows(screen.main, expected)) {
      fail(`l’écran « ${label} » ne montre pas « ${expected} » :\n${screen.main}`);
    }
    done(`écran « ${label} »`);
    quote(screen.main, 14);
    await window.capture(slug(label));
  }
}

/** Le mode d'affichage enregistré, lu dans le fichier de réglages. */
function savedTheme() {
  if (process.env.APPDATA === undefined) {
    return null;
  }
  try {
    const text = readFileSync(path.join(process.env.APPDATA, 'Shinku', 'settings.json'), 'utf8');
    return JSON.parse(text.replace(/^\ufeff/, '')).theme ?? null;
  } catch {
    return null;
  }
}

async function checkSettings(window) {
  const screen = await window.open('Paramètres');
  if (!/Shinku version \d+\.\d+\.\d+/.test(screen.main)) {
    fail(`les Paramètres ne montrent pas la version :\n${screen.main}`);
  }
  done('écran « Paramètres »');
  quote(screen.main, 14);
  await window.capture('parametres');

  const pressed = () =>
    window.session.run(() => document.querySelector('.pal[aria-pressed=true] .pn')?.textContent?.trim() ?? '');
  const initial = await pressed();
  const [label, theme] = screen.theme === 'light' ? ['Sombre', 'dark'] : ['Clair', 'light'];
  await window.click(label, '.pal', label);
  await until(`le mode ${label} est appliqué`, 15_000, async () => (await window.screen()).theme === theme);
  if (onWindows) {
    await until(`le mode ${label} est enregistré dans settings.json`, 15_000, () => savedTheme() === theme);
  }
  await window.healthy(`après le passage en mode ${label}`);
  await window.capture(`mode-${theme}`);
  done(`le mode ${label} s’applique${onWindows ? ' et s’enregistre' : ''}`);

  // On remet le mode trouvé en arrivant.
  await window.click(initial, '.pal', initial);
  await until(`le mode ${initial} est revenu`, 15_000, async () => (await pressed()) === initial);
  await window.healthy(`après le retour au mode ${initial}`);
}

/** Un second lancement ne doit pas ouvrir une seconde fenêtre. */
async function checkSingleWindow(port, asUser) {
  const second = launch(options.programme, port, asUser);
  try {
    await until('le second lancement rend la main', 30_000, () => second.error !== null || second.exit !== null);
  } finally {
    stop(second);
  }
  if (second.error !== null || second.exit.code !== 0) {
    fail(`le second lancement finit mal : ${second.error?.message ?? `code ${second.exit.code}`}`);
  }
  const targets = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json();
  const pages = targets.filter((target) => target.type === 'page');
  if (pages.length !== 1) {
    fail(`après un second lancement, ${pages.length} pages sont ouvertes au lieu d’une`);
  }
  done('un second lancement n’ouvre pas de seconde fenêtre');
}

async function checkClose(window, run) {
  try {
    await window.click('Fermer', 'button.wclose');
  } catch (error) {
    // La fenêtre se ferme sous le clic : la réponse au geste peut ne jamais arriver.
    if (error instanceof Failure) {
      throw error;
    }
  }
  await until('le programme s’arrête après un clic sur « Fermer »', 30_000, () => run.exit !== null);
  if (run.exit.code !== 0) {
    fail(`le programme s’arrête avec le code ${run.exit.code} au lieu de 0`);
  }
  done('le bouton « Fermer » ferme la fenêtre, et le programme s’arrête');
}

async function main() {
  if (options.programme === undefined) {
    console.error('Usage : node scripts/essai-fenetre.mjs --programme <shinku.exe> [--captures <dossier>]');
    return 2;
  }
  if (!existsSync(options.programme)) {
    console.error(`Programme introuvable : ${options.programme}`);
    return 2;
  }
  if (options.captures !== undefined) {
    mkdirSync(options.captures, { recursive: true });
  }

  const port = await freePort();
  const asUser = elevated();
  say(`sur ${os.version()} (${os.release()})`);
  say(`lancement de ${options.programme}${asUser ? ', avec les droits ordinaires d’un utilisateur' : ''}`);
  const run = launch(options.programme, port, asUser);
  let window = null;
  try {
    const page = await findPage(port, run, 90_000);
    done(`la fenêtre s’ouvre sur ${page.url}`);

    window = new Window(await Session.open(page.webSocketDebuggerUrl));
    await window.listen();
    await checkStart(window);
    if (options.captures !== undefined) {
      captureDesktop(path.join(options.captures, '00-bureau.png'));
    }
    await checkScan(window);
    await checkModules(window);
    await checkSettings(window);
    if (onWindows) {
      await checkSingleWindow(port, asUser);
    }
    await checkClose(window, run);

    if (window.notes.length > 0) {
      say('la page a écrit dans sa console :');
      quote(window.notes.join('\n'));
    }
    say('Essai réussi.');
    return 0;
  } catch (error) {
    say(`✗ ${error instanceof Failure ? error.message.split('\n')[0] : `erreur de l’essai : ${error.stack}`}`);
    if (error instanceof Failure && error.message.includes('\n')) {
      quote(error.message.split('\n').slice(1).join('\n'));
    }
    if (window !== null) {
      const screen = await window.screen().catch(() => null);
      if (screen !== null) {
        say('la fenêtre affichait :');
        quote(screen.whole);
      }
      if (window.problems.length + window.notes.length > 0) {
        say('la page a écrit dans sa console :');
        quote([...window.problems, ...window.notes].join('\n'));
      }
      await window.capture('echec');
    } else if (run.exit === null && run.pid !== null) {
      const texts = windowTexts(run.pid);
      if (texts !== '') {
        say('le programme affichait :');
        quote(texts);
      }
    }
    if (options.captures !== undefined) {
      captureDesktop(path.join(options.captures, '99-bureau-echec.png'));
    }
    return 1;
  } finally {
    window?.session.close();
    stop(run);
  }
}

process.exitCode = await main();
