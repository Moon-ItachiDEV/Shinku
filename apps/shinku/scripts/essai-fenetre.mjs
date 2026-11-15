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
// enregistré. Il ouvre la confirmation du grand vide, puis l'annule : rien
// n'est supprimé.
//
//   node scripts/essai-fenetre.mjs --programme "C:\Program Files\Shinku\shinku.exe" --captures captures
//
// Avec --grand-vide, il va au bout : il confirme, et Shinku vide la corbeille,
// supprime les fichiers inutiles et envoie les doublons à la corbeille, pour
// de bon. À réserver à une machine jetable (celle de l'intégration continue).
// L'essai y dépose d'abord un faux fichier inutile et deux fichiers
// identiques, et vérifie sur le disque ce qu'ils sont devenus.
//
// L'essai allume aussi, un instant, le mode jeu automatique : le programme
// lance alors shinku-agent.exe (installé à côté de lui), que l'essai compte
// parmi les processus de la machine (il doit y en avoir exactement un). Il
// l'éteint ensuite, ou le laisse allumé si la machine l'avait allumé. La reconnaissance
// d'un vrai jeu est essayée à part, par essai-mode-jeu.ps1.
//
// Mise au point hors de Windows : `--programme` peut être un script qui ouvre
// la démo (`npm run build:demo`, servie par n'importe quel serveur web) dans un
// Chrome lancé avec le port de débogage que l'essai donne dans la variable
// WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS, et `--adresse` le début de l'adresse
// de la démo. La démo n'a pas de vrai agent : seuls les contrôles de la page
// sont alors faits.
//
// Sans dépendance : Node 22 suffit.

import { spawn, spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, rmSync, statfsSync, writeFileSync } from 'node:fs';
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
    /** Va au bout du grand vide : des fichiers sont réellement supprimés. */
    'grand-vide': { type: 'boolean', default: false },
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
const note = (line) => say(`• ${line}`);

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

/** La page Mode jeu : l'interrupteur principal, le titre de l'en-tête, l'avertissement. */
function readGame() {
  const text = (element) => (element?.innerText ?? '').replace(/[ \u00a0\u202f]+/g, ' ').trim();
  const main = document.querySelector('#game-auto');
  return {
    on: main === null ? null : main.getAttribute('aria-checked') === 'true',
    title: text(document.querySelector('.phead h1')),
    warning: text(document.querySelector('.pwarn')),
  };
}

/**
 * La page Démarrage : son titre, ses lignes (avec la carte où elles sont), et la
 * zone de retour. Une ligne se reconnaît à ce qu'elle dit de son nom : le nom
 * lui-même, ou le nom d'inscription (« inscrite sous « … » »).
 */
function readStartup() {
  const text = (element) => (element?.innerText ?? '').replace(/[ \u00a0\u202f]+/g, ' ').trim();
  const say = document.querySelector('.pzone .psay');
  return {
    title: text(document.querySelector('.main h1')),
    loading: document.querySelector('.page.startup .note') !== null,
    rows: [...document.querySelectorAll('.page.startup .plist > li.row')].map((row) => {
      const control = row.querySelector('button[role=switch]');
      return {
        name: text(row.querySelector('.t1')),
        detail: text(row.querySelector('.t2')),
        card: text(row.closest('section.psec')?.querySelector('h2')),
        hasSwitch: control !== null,
        checked: control?.getAttribute('aria-checked') === 'true',
        disabled: control?.disabled === true || control?.getAttribute('aria-disabled') === 'true',
        label: control?.getAttribute('aria-label') ?? '',
        tags: [...row.querySelectorAll('.tag')].map((tag) => text(tag)),
        text: text(row),
      };
    }),
    say: say === null ? null : { text: text(say.querySelector('span')), undo: say.querySelector('button') !== null },
  };
}

/** Où se trouve l'interrupteur de la ligne qui porte ce nom, et ce qu'un clic à cet endroit toucherait. */
function locateSwitch(name) {
  const wanted = name.toLowerCase();
  const row = [...document.querySelectorAll('.page.startup .plist > li.row')].find((candidate) =>
    `${candidate.querySelector('.t1')?.textContent ?? ''} ${candidate.querySelector('.t2')?.textContent ?? ''}`
      .toLowerCase()
      .includes(wanted),
  );
  const element = row?.querySelector('button[role=switch]');
  if (element === null || element === undefined) {
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
    // Les interrupteurs du Démarrage restent focalisables pendant un geste : `aria-disabled`, pas `disabled`.
    disabled: element.disabled === true || element.getAttribute('aria-disabled') === 'true',
    reached: hit !== null && element.contains(hit),
    cover: hit === null ? 'rien (hors de la fenêtre)' : `<${hit.tagName.toLowerCase()} class="${hit.className}">`,
  };
}

/** Clique sur l'interrupteur de cette ligne, sans passer par la souris. */
function clickSwitchInPage(name) {
  const wanted = name.toLowerCase();
  const row = [...document.querySelectorAll('.page.startup .plist > li.row')].find((candidate) =>
    `${candidate.querySelector('.t1')?.textContent ?? ''} ${candidate.querySelector('.t2')?.textContent ?? ''}`
      .toLowerCase()
      .includes(wanted),
  );
  const element = row?.querySelector('button[role=switch]');
  element?.click();
  return element !== null && element !== undefined;
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
    // Les interrupteurs du Démarrage restent focalisables pendant un geste : `aria-disabled`, pas `disabled`.
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

/** Les lignes à cocher de l'écran Résultats, telles qu'elles s'affichent. */
function readChoices() {
  const text = (element) => (element?.innerText ?? '').replace(/[ \u00a0\u202f]+/g, ' ').trim();
  return [...document.querySelectorAll('.main label.row')].map((row) => {
    const box = row.querySelector('input[type=checkbox]');
    return {
      name: text(row.querySelector('.t1')),
      value: text(row.querySelector('.val')),
      checked: box?.checked === true,
      locked: box?.disabled === true,
    };
  });
}

/** La confirmation du grand vide, si elle est ouverte. */
function readDialog() {
  const text = (element) => (element?.innerText ?? '').replace(/[ \u00a0\u202f]+/g, ' ').trim();
  const dialog = document.querySelector('dialog.ask[open]');
  if (dialog === null) {
    return null;
  }
  return {
    whole: text(dialog),
    rows: [...dialog.querySelectorAll('li.row')].map((row) => ({
      name: text(row.querySelector('.t1')),
      value: text(row.querySelector('.val')),
    })),
    focus: text(document.activeElement),
  };
}

/** Le grand vide en cours, ou son bilan. */
function readClean() {
  const text = (element) => (element?.innerText ?? '').replace(/[ \u00a0\u202f]+/g, ' ').trim();
  const bilan = document.querySelector('.main .bilan');
  return {
    running: document.querySelector('.dock .core') !== null,
    status: text(document.querySelector('.main [role=status]')),
    report: bilan === null ? null : text(bilan),
    alerts: bilan === null ? '' : text(bilan.querySelector('.alert')),
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
  click(what, selector, label = null) {
    return this.press(`le bouton « ${what} »`, locate, clickInPage, [selector, label]);
  }

  /** Clique sur l'interrupteur de la ligne du Démarrage qui porte ce nom. */
  clickSwitch(name) {
    return this.press(`l’interrupteur de la ligne « ${name} »`, locateSwitch, clickSwitchInPage, [name]);
  }

  /** `find` (dans la page) dit où est la cible ; `inPage` la clique quand la souris ne répond pas. */
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
  // La ligne « Démarrage » compte les apps actives, ou dit « Non mesuré » : plus « Bientôt ».
  const startupRow = end.rows.find((row) => row.startsWith('Démarrage'));
  if (startupRow === undefined) {
    fail(`l’écran d’analyse n’a pas de ligne « Démarrage » :\n${end.rows.join('\n')}`);
  }
  if (!/(app active|apps actives|Aucune app active|Non mesuré)/.test(startupRow)) {
    fail(`la ligne « Démarrage » de l’analyse ne compte pas d’apps actives : « ${startupRow} »`);
  }
  await window.healthy('après l’analyse');

  await window.click('Voir les résultats', '.dock .orb', 'Voir les résultats');
  const results = await until('les résultats sont affichés', 20_000, async () => {
    const screen = await window.screen();
    return shows(screen.main, 'Le grand vide peut libérer') && !shows(screen.main, 'Chargement…') ? screen : null;
  });
  done('« Voir les résultats » affiche les résultats');
  quote(results.main);
  if (!/(apps? actives? au démarrage de Windows|Aucune app active au démarrage|Démarrage Non mesuré)/.test(results.main)) {
    fail(`les résultats ne disent pas combien d’apps sont actives au démarrage :\n${results.main}`);
  }
  await window.healthy('sur les résultats');
  await checkChoices(window);
  await checkConfirmation(window);
}

/**
 * Ce que « Faire le vide » emporterait sans que la personne ait rien choisi :
 * les fichiers inutiles, jamais les doublons, qui sont ses propres fichiers.
 */
async function checkChoices(window) {
  const choice = async (name) => (await window.session.run(readChoices)).find((row) => row.name === name);
  for (const row of await window.session.run(readChoices)) {
    say(`  ${row.checked ? '[x]' : '[ ]'} ${row.name} : ${row.value}${row.locked ? ' (case fermée)' : ''}`);
  }
  const [junk, duplicates] = [await choice('Fichiers inutiles'), await choice('Doublons')];
  if (junk === undefined || duplicates === undefined) {
    fail('les résultats ne montrent pas les lignes « Fichiers inutiles » et « Doublons »');
  }
  if (!junk.checked) {
    fail('la ligne « Fichiers inutiles » devrait être cochée d’office');
  }
  if (duplicates.checked) {
    fail('la ligne « Doublons » est cochée d’office : des fichiers personnels partiraient sans avoir été choisis');
  }
  done('les fichiers inutiles sont cochés d’office, les doublons ne le sont pas');
  await window.capture('resultats');

  if (duplicates.locked) {
    say('  la case « Doublons » est fermée : rien à cocher');
    return;
  }
  // La personne coche les doublons, puis change d'avis.
  for (const wanted of [true, false]) {
    await window.click('Doublons', '.main label.row', 'Doublons');
    await until(`la case « Doublons » est ${wanted ? 'cochée' : 'décochée'}`, 10_000, async () => {
      const row = await choice('Doublons');
      return row !== undefined && row.checked === wanted;
    });
  }
  await window.healthy('après avoir coché puis décoché « Doublons »');
  done('la case « Doublons » se coche et se décoche à la main');
}

/** Ouvre la confirmation du grand vide. Rend `null` s'il n'y a rien à nettoyer. */
async function openConfirmation(window) {
  const button = await window.session.run(locate, '.dock .orb', 'Faire le vide');
  if (button === null || button.disabled) {
    return null;
  }
  await window.click('Faire le vide', '.dock .orb', 'Faire le vide');
  return until('la confirmation est ouverte', 30_000, () => window.session.run(readDialog));
}

/**
 * « Faire le vide » ne fait rien partir : il demande d'abord, en listant ce
 * qui partirait. Annuler referme la demande et laisse tout en place.
 */
async function checkConfirmation(window) {
  const dialog = await openConfirmation(window);
  if (dialog === null) {
    say('  le bouton « Faire le vide » est fermé : rien à nettoyer sur cette machine');
    return;
  }
  quote(dialog.whole);
  const names = dialog.rows.map((row) => row.name);
  if (!names.includes('Fichiers inutiles')) {
    fail('la confirmation ne liste pas les fichiers inutiles, pourtant cochés');
  }
  if (names.includes('Doublons')) {
    fail('la confirmation liste les doublons, que personne n’a cochés');
  }
  if (dialog.focus !== 'Annuler') {
    fail(`à l’ouverture de la confirmation, le bouton actif est « ${dialog.focus} » au lieu de « Annuler »`);
  }
  await window.capture('confirmation');
  await window.click('Annuler', 'dialog.ask button', 'Annuler');
  await until('la confirmation est refermée', 10_000, async () => (await window.session.run(readDialog)) === null);
  const screen = await window.healthy('après avoir annulé le grand vide');
  if (!shows(screen.main, 'Le grand vide peut libérer')) {
    fail(`après « Annuler », les résultats ne sont plus affichés :\n${screen.main}`);
  }
  done('« Faire le vide » demande confirmation, et « Annuler » ne touche à rien');
}

/** Les fichiers que l'essai dépose avant un vrai grand vide (sous Windows). */
function plantedFiles() {
  if (!onWindows || process.env.LOCALAPPDATA === undefined || process.env.USERPROFILE === undefined) {
    return null;
  }
  const folder = path.join(process.env.USERPROFILE, 'Documents', 'Essai Shinku');
  return {
    // Une miniature de l'Explorateur : la règle « windows.thumbnails » la compte.
    junk: path.join(process.env.LOCALAPPDATA, 'Microsoft', 'Windows', 'Explorer', 'thumbcache_essai-shinku.db'),
    folder,
    twins: [path.join(folder, 'film.bin'), path.join(folder, 'film - Copie.bin')],
  };
}

function plant(files) {
  mkdirSync(path.dirname(files.junk), { recursive: true });
  writeFileSync(files.junk, Buffer.alloc(256 * 1024, 7));
  mkdirSync(files.folder, { recursive: true });
  // Deux fichiers identiques, assez gros pour compter comme doublons.
  const content = Buffer.alloc(2 * 1024 * 1024);
  for (let index = 0; index < content.length; index += 4) {
    content.writeUInt32LE((index * 2654435761) >>> 0, index);
  }
  for (const twin of files.twins) {
    writeFileSync(twin, content);
  }
  say(`  fichiers déposés : ${files.junk}, et deux fichiers identiques dans ${files.folder}`);
}

/** L'état des lignes d'un groupe de doublons dont un exemplaire porte ce nom. */
function readCopies(name) {
  const lines = [...document.querySelectorAll('.main li.pick button.row')];
  const mine = lines.find((line) => (line.querySelector('.path')?.textContent ?? '').trim().endsWith(`\\${name}`));
  const group = mine?.closest('ul');
  if (mine === undefined || group == null) {
    return null;
  }
  return [...group.querySelectorAll('li.pick button.row')].map((line) => ({
    path: (line.querySelector('.path')?.textContent ?? '').trim(),
    kept: line.getAttribute('aria-pressed') === 'true',
  }));
}

/** Marque la ligne d'un exemplaire, pour que `click` la retrouve. */
function markCopy(name) {
  const lines = [...document.querySelectorAll('.main li.pick button.row')];
  const mine = lines.find((line) => (line.querySelector('.path')?.textContent ?? '').trim().endsWith(`\\${name}`));
  mine?.setAttribute('data-essai', 'copie');
  return mine !== undefined;
}

/**
 * La personne choisit l'exemplaire à garder : sur l'écran Doublons, l'essai
 * clique la copie que Shinku n'aurait pas gardée de lui-même, et rend son nom.
 * Le grand vide devra laisser celle-là, et elle seule.
 */
async function chooseKept(window, planted) {
  await window.open('Doublons');
  const names = planted.twins.map((twin) => path.basename(twin));
  const copies = await until('les deux fichiers déposés sont listés', 30_000, async () => {
    const found = await window.session.run(readCopies, names[0]);
    if (found === null) {
      // Le groupe est peut-être plus loin dans la liste : l'essai demande la suite.
      await window.session.run(clickInPage, '.main .more button:not(:disabled)', null);
    }
    return found !== null && found.length === 2 ? found : null;
  }, 500);
  if (copies.filter((copy) => copy.kept).length !== 1) {
    fail(`le groupe des fichiers déposés ne garde pas un exemplaire et un seul : ${JSON.stringify(copies)}`);
  }
  const wanted = names.find((name) => copies.some((copy) => !copy.kept && copy.path.endsWith(`\\${name}`)));
  if (wanted === undefined || !(await window.session.run(markCopy, wanted))) {
    fail('la copie non gardée ne se retrouve pas dans la liste');
  }
  await window.click(`garder « ${wanted} »`, '.main button.row[data-essai=copie]');
  await until(`« ${wanted} » est l’exemplaire conservé`, 10_000, async () => {
    const now = await window.session.run(readCopies, wanted);
    return now !== null && now.filter((copy) => copy.kept).length === 1 && now.some((copy) => copy.kept && copy.path.endsWith(`\\${wanted}`));
  });
  await window.healthy('après avoir choisi l’exemplaire à garder');
  await window.capture('doublons-choix');
  done(`sur l’écran Doublons, un clic garde « ${wanted} » à la place de l’exemplaire le plus ancien`);
  return wanted;
}

/**
 * Le grand vide, pour de bon : confirmer, suivre l'avancement, lire le bilan,
 * puis regarder sur le disque ce que les fichiers déposés sont devenus.
 */
async function checkClean(window, planted) {
  const wanted = planted === null ? null : await chooseKept(window, planted);
  await window.click('Grand vide', 'button.nav', 'Grand vide');
  await until('les résultats sont de nouveau affichés', 20_000, async () =>
    shows((await window.screen()).main, 'Le grand vide peut libérer'),
  );
  const choice = async (name) => (await window.session.run(readChoices)).find((row) => row.name === name);
  const duplicates = await choice('Doublons');
  const withDuplicates = duplicates !== undefined && !duplicates.locked;
  if (planted !== null && !withDuplicates) {
    fail('deux fichiers identiques ont été déposés, mais la case « Doublons » est fermée');
  }
  if (withDuplicates && !duplicates.checked) {
    await window.click('Doublons', '.main label.row', 'Doublons');
    await until('la case « Doublons » est cochée', 10_000, async () => (await choice('Doublons'))?.checked === true);
  }

  const dialog = await openConfirmation(window);
  if (dialog === null) {
    fail('le bouton « Faire le vide » est fermé : le grand vide ne peut pas être essayé');
  }
  quote(dialog.whole);
  const names = dialog.rows.map((row) => row.name);
  if (withDuplicates && !names.includes('Doublons')) {
    fail('la confirmation ne liste pas les doublons, pourtant cochés');
  }
  await window.capture('confirmation-grand-vide');
  await window.click('Faire le vide', 'dialog.ask button', 'Faire le vide');

  const begun = Date.now();
  let pictured = false;
  let told = Date.now();
  const end = await until('le grand vide est terminé', SCAN_BUDGET, async () => {
    const clean = await window.session.run(readClean);
    if (clean.report !== null) {
      return clean;
    }
    if (!clean.running) {
      // Ni avancement ni bilan : la demande a été refusée, ou la fenêtre est revenue ailleurs.
      const screen = await window.screen();
      if (screen.toast !== '') {
        fail(`le grand vide n’a pas démarré : « ${screen.toast} »`);
      }
      return null;
    }
    if (!pictured) {
      pictured = true;
      await window.capture('grand-vide');
    }
    if (Date.now() - told >= 15_000) {
      told = Date.now();
      say(`  ${clean.status}`);
    }
    return null;
  }, 300);
  done(`le grand vide se termine en ${Math.round((Date.now() - begun) / 1000)} s`);
  quote(end.report);
  await window.capture('bilan');
  await window.healthy('après le grand vide');
  if (!shows(end.report, 'Grand vide terminé')) {
    fail('le bilan n’annonce pas « Grand vide terminé »');
  }
  if (end.alerts !== '') {
    fail(`le bilan signale un problème : ${end.alerts}`);
  }

  if (planted !== null) {
    if (existsSync(planted.junk)) {
      fail(`le faux fichier inutile est toujours là : ${planted.junk}`);
    }
    const left = planted.twins.filter((twin) => existsSync(twin));
    if (left.length !== 1) {
      fail(`des deux fichiers identiques, il devait en rester un seul ; il en reste ${left.length}`);
    }
    if (path.basename(left[0]) !== wanted) {
      fail(`l’exemplaire choisi était « ${wanted} », mais c’est « ${path.basename(left[0])} » qui est resté`);
    }
    if (!shows(end.report, 'dans la corbeille')) {
      fail('le bilan ne dit pas que la copie attend dans la corbeille');
    }
    done(`sur le disque : le fichier inutile est parti, et il reste l’exemplaire choisi du doublon (${path.basename(left[0])})`);
    rmSync(planted.folder, { recursive: true, force: true });
  }

  await window.click('Terminé', '.dock button', 'Terminé');
  await until('l’Accueil est revenu', 20_000, async () => shows((await window.screen()).main, 'Analyser'));
  await window.healthy('après avoir refermé le bilan');
  done('« Terminé » referme le bilan et revient à l’Accueil');
}

async function checkModules(window) {
  /**
   * Chaque module, et un texte que son écran montre quand le cœur a répondu.
   * Deux textes : le second est celui d'une machine où il n'y a rien à retirer.
   */
  const modules = [
    ['Fichiers inutiles', 'Ces fichiers se recréent tout seuls'],
    ['Doublons', 'Coche Doublons dans le grand vide', 'Aucune copie à retirer'],
    ['Gros fichiers', 'de plus de'],
    ['Corbeille', 'la corbeille se vide avec le grand vide', 'Rien à vider'],
    ['Démarrage', 'inscrite'],
    ['Mode jeu', 'Pendant que tu joues'],
    ['Désinstallation', 'Ce module arrive dans une prochaine version'],
    ['Confidentialité', 'Ce module arrive dans une prochaine version'],
  ];
  for (const [label, ...expected] of modules) {
    const screen = await window.open(label);
    if (!expected.some((text) => shows(screen.main, text))) {
      fail(`l’écran « ${label} » ne montre pas « ${expected.join(' » ni « ')} » :\n${screen.main}`);
    }
    done(`écran « ${label} »`);
    quote(screen.main, 14);
    await window.capture(slug(label));
  }
}

// ── Le Démarrage ───────────────────────────────────────────────────────────

/**
 * Les entrées que `essai-demarrage.ps1 -Preparer` crée. `name` est le nom de la
 * valeur de registre (ou du raccourci) : la page le montre comme nom, ou comme
 * nom d'inscription quand le fichier a une description.
 */
const STARTUP_ENTRIES = [
  // `approved` : la sous-clé de StartupApproved (HKCU) et le nom de la valeur que Windows y note.
  { name: 'Essai Shinku Compte', approved: ['Run', 'Essai Shinku Compte'], undo: 'Annuler' },
  { name: 'Essai Shinku Cible', approved: ['Run', 'Essai Shinku Cible'], undo: 'Annuler' },
  { name: 'Essai Shinku 32', approved: ['Run32', 'Essai Shinku 32'], undo: 'interrupteur' },
  { name: 'Essai Shinku Dossier', approved: ['StartupFolder', 'Essai Shinku Dossier.lnk'], undo: 'interrupteur' },
];
const STARTUP_MACHINE = 'Essai Shinku PC';
const STARTUP_APPROVED = 'HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\StartupApproved';
const APPROVED_ON = '020000000000000000000000';

/** La valeur d'approbation, en hexadécimal, lue par `reg query /reg:64` ; `null` si elle n'existe pas. */
function readApproval([key, name]) {
  const result = spawnSync('reg', ['query', `${STARTUP_APPROVED}\\${key}`, '/v', name, '/reg:64'], {
    encoding: 'utf8',
    timeout: 15_000,
  });
  if (result.status !== 0) {
    return null;
  }
  const found = /REG_BINARY\s+([0-9A-Fa-f]+)/.exec(result.stdout ?? '');
  return found === null ? null : found[1].toLowerCase();
}

/** Attend que l'approbation prenne la forme voulue ; dit ce qu'elle valait en dernier sinon. */
async function expectApproval(entry, what, accepts) {
  let last = null;
  const deadline = Date.now() + 15_000;
  for (;;) {
    last = readApproval(entry.approved);
    if (last !== null && accepts(last)) {
      return last;
    }
    if (Date.now() >= deadline) {
      fail(
        `${entry.name} : ${what} dans StartupApproved\\${entry.approved[0]}\\${entry.approved[1]} (reg query /reg:64), ` +
          `dernière valeur lue : ${last === null ? 'aucune' : last}`,
      );
    }
    await sleep(250);
  }
}

async function checkStartup(window) {
  if (options.adresse !== undefined) {
    note('essai du Démarrage sauté : la démo n’a pas les entrées de la machine d’essai');
    return;
  }
  const read = () => window.session.run(readStartup);
  const rowOf = (screen, name) => screen.rows.find((row) => `${row.name} ${row.detail}`.toLowerCase().includes(name.toLowerCase()));
  await window.open('Démarrage');
  const loaded = await until('la liste du Démarrage est affichée', 30_000, async () => {
    const now = await read();
    return !now.loading && now.rows.length > 0 ? now : null;
  });
  if (!shows(loaded.title, 'inscrite')) {
    fail(`le titre de la page Démarrage ne compte pas d’apps inscrites : « ${loaded.title} »`);
  }
  const mine = [...STARTUP_ENTRIES.map((entry) => entry.name), STARTUP_MACHINE];
  for (const row of loaded.rows.filter((one) => mine.some((name) => rowOf({ rows: [one] }, name) !== undefined))) {
    say(`  ${row.card} : ${row.name}${row.detail === '' ? '' : ` (${row.detail})`}${row.hasSwitch ? ` [${row.checked ? 'actif' : 'coupé'}]` : ` ${row.tags.join(' ')}`}`);
  }

  // Les quatre lignes du compte sont là, avec un interrupteur, allumé.
  for (const entry of STARTUP_ENTRIES) {
    const row = rowOf(loaded, entry.name);
    if (row === undefined) {
      fail(`la page Démarrage n’a pas de ligne « ${entry.name} » :\n${loaded.rows.map((one) => `${one.card} | ${one.name} | ${one.detail}`).join('\n')}`);
    }
    if (!shows(row.card, 'Pour ton compte')) {
      fail(`la ligne « ${entry.name} » est dans la carte « ${row.card} » au lieu de « Pour ton compte »`);
    }
    if (!row.hasSwitch || !row.checked) {
      fail(`la ligne « ${entry.name} » devrait avoir un interrupteur allumé : ${JSON.stringify(row)}`);
    }
  }
  // La ligne du PC : là, sans interrupteur, avec sa pastille d'état.
  const machine = rowOf(loaded, STARTUP_MACHINE);
  if (machine === undefined) {
    fail(`la page Démarrage n’a pas la ligne du PC « ${STARTUP_MACHINE} »`);
  }
  if (!shows(machine.card, 'Pour tous les comptes')) {
    fail(`la ligne « ${STARTUP_MACHINE} » est dans la carte « ${machine.card} »`);
  }
  if (machine.hasSwitch) {
    fail(`la ligne du PC « ${STARTUP_MACHINE} » a un interrupteur : Shinku ne règle pas le PC`);
  }
  if (!machine.tags.includes('Active')) {
    fail(`la ligne du PC devrait porter la pastille « Active » : ${JSON.stringify(machine.tags)}`);
  }
  // La description du fichier a été lue : la ligne Cible a le nom de notepad.exe, et l'inscription en second nom.
  const target = rowOf(loaded, 'Essai Shinku Cible');
  if (!shows(target.detail, 'inscrite sous') || !shows(target.detail, 'Essai Shinku Cible')) {
    fail(`la ligne « Essai Shinku Cible » ne dit pas « inscrite sous » son nom d’inscription : « ${target.name} » / « ${target.detail} »`);
  }
  done('les quatre lignes du compte et la ligne du PC sont là ; la ligne du PC n’a pas d’interrupteur ; la description du fichier est lue');
  await window.capture('demarrage');

  const controlled = onWindows;
  for (const entry of STARTUP_ENTRIES) {
    const isOn = async (on) => {
      const row = rowOf(await read(), entry.name);
      return row !== undefined && row.hasSwitch && row.checked === on ? row : null;
    };
    // Couper : l'interrupteur bouge, la zone de retour le dit, et Windows en garde la trace.
    await window.clickSwitch(entry.name);
    await until(`« ${entry.name} » : l’interrupteur s’éteint`, 10_000, () => isOn(false));
    const cutSay = await until(`« ${entry.name} » : la zone de retour dit « est coupée »`, 15_000, async () => {
      const now = await read();
      return now.say !== null && shows(now.say.text, 'est coupée') ? now.say : null;
    });
    if (!cutSay.undo) {
      fail(`« ${entry.name} » : la zone de retour n’a pas de bouton « Annuler » : « ${cutSay.text} »`);
    }
    if (controlled) {
      const value = await expectApproval(entry, 'attendu 12 octets commençant par 03000000', (hex) => hex.length === 24 && hex.startsWith('03000000'));
      done(`« ${entry.name} » coupée : « ${cutSay.text} » ; ${entry.approved[0]}\\${entry.approved[1]} = ${value}`);
    } else {
      done(`« ${entry.name} » coupée : « ${cutSay.text} »`);
    }
    await window.healthy(`après avoir coupé « ${entry.name} »`);
    await window.capture(`demarrage-${slug(entry.name)}-coupee`);

    // Remettre : par « Annuler » ou par l'interrupteur, selon la ligne.
    if (entry.undo === 'Annuler') {
      await window.click('Annuler', '.pzone .psay button', 'Annuler');
    } else {
      await window.clickSwitch(entry.name);
    }
    await until(`« ${entry.name} » : l’interrupteur se rallume`, 10_000, () => isOn(true));
    await until(`« ${entry.name} » : la zone de retour dit que l’app se lancera de nouveau`, 15_000, async () => {
      const now = await read();
      return now.say !== null && shows(now.say.text, 'se lancera de nouveau') ? now.say : null;
    });
    if (controlled) {
      await expectApproval(entry, `attendu ${APPROVED_ON}`, (hex) => hex === APPROVED_ON);
      done(`« ${entry.name} » remise (${entry.undo === 'Annuler' ? '« Annuler »' : 'interrupteur'}) : ${entry.approved[0]}\\${entry.approved[1]} = ${APPROVED_ON}`);
    } else {
      done(`« ${entry.name} » remise`);
    }
    await window.healthy(`après avoir remis « ${entry.name} »`);
    await window.capture(`demarrage-${slug(entry.name)}-remise`);
  }
}

/** Le nombre de processus shinku-agent.exe qui tournent ; `null` hors de Windows. */
function agentCount() {
  if (!onWindows) {
    return null;
  }
  const result = spawnSync('tasklist', ['/FI', 'IMAGENAME eq shinku-agent.exe', '/FO', 'CSV', '/NH'], {
    encoding: 'utf8',
  });
  return (result.stdout ?? '').split('\n').filter((line) => line.toLowerCase().includes('shinku-agent.exe')).length;
}

/**
 * Arrête, de force, l'agent du dossier d'installation que l'essai a lancé (celui
 * qui est à côté de `--programme`), et lui seul : un autre `shinku-agent.exe`
 * (celui d'une autre session, ou d'une autre installation) n'est pas à l'essai.
 * Le chemin est comparé sans tenir compte de la casse, comme Windows.
 */
function stopOwnAgent() {
  const own = path.join(path.dirname(path.resolve(options.programme)), 'shinku-agent.exe');
  const script =
    "Get-Process -Name 'shinku-agent' -ErrorAction SilentlyContinue | " +
    'Where-Object { $_.Path -and ($_.Path -ieq $env:SHINKU_ESSAI_AGENT) } | ' +
    'Stop-Process -Force -ErrorAction SilentlyContinue';
  spawnSync('powershell', ['-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-Command', script], {
    stdio: 'ignore',
    env: { ...process.env, SHINKU_ESSAI_AGENT: own },
  });
}

/**
 * L'interrupteur principal du mode jeu. Éteint au départ : l'allumer lance
 * l'agent, l'éteindre le fait sortir. Allumé au départ (la machine l'avait
 * allumé) : l'essai fait le parcours dans l'autre sens, et le laisse allumé.
 *
 * Ce qui prouve que l'agent tourne, sous Windows, c'est le nombre de processus
 * `shinku-agent.exe` (il doit valoir 1). L'avertissement de la page n'en est
 * pas une preuve : il ne s'affiche qu'après 15 s d'arrêt, alors que l'essai
 * lit la page bien avant. Sa lecture est donnée comme une indication.
 */
async function checkGameMode(window) {
  const read = () => window.session.run(readGame);
  const toggle = async (on) => {
    await window.click('Mode jeu automatique', '#game-auto');
    await until(`l’interrupteur passe sur ${on ? 'allumé' : 'éteint'}`, 10_000, async () => (await read()).on === on);
    // Un sondage de la page (toutes les 2 s) redonne l'état du cœur : l'interrupteur ne doit pas revenir en arrière.
    await sleep(2500);
    if ((await read()).on !== on) {
      fail(`l’interrupteur est revenu sur ${on ? 'éteint' : 'allumé'} : le cœur a refusé le changement`);
    }
    await window.healthy(`après avoir ${on ? 'allumé' : 'éteint'} le mode jeu`);
  };
  const controlled = agentCount() !== null;
  const turnOn = async () => {
    await toggle(true);
    if (controlled) {
      // La preuve : exactement un agent, compté parmi les processus de Windows.
      await until('un seul agent tourne', 20_000, () => agentCount() === 1, 500);
      done('allumé : un agent tourne (compte des processus shinku-agent.exe : 1)');
    } else {
      done('contrôle de l’agent sauté (démo)');
    }
  };
  const turnOff = async () => {
    await toggle(false);
    if (controlled) {
      // L'agent se voit éteint à son prochain pas (2 s) : il n'a plus de raison de veiller.
      await until('l’agent sort', 20_000, () => agentCount() === 0, 500);
      done('éteint : l’agent est sorti (compte des processus shinku-agent.exe : 0)');
    } else {
      done('contrôle de l’agent sauté (démo)');
    }
  };

  const screen = await window.open('Mode jeu');
  for (const expected of ['Mode jeu automatique', 'Pendant que tu joues', 'Tes jeux', 'Windows s’en occupe déjà']) {
    if (!shows(screen.main, expected)) {
      fail(`la page Mode jeu ne montre pas « ${expected} » :\n${screen.main}`);
    }
  }
  const start = await read();
  if (start.on === null) {
    fail('la page Mode jeu n’a pas d’interrupteur principal');
  }
  const startOn = start.on;
  if (startOn) {
    note('le mode jeu était déjà allumé au départ : l’essai fait le parcours dans l’autre sens (éteindre, puis rallumer) et le laisse allumé');
  } else {
    if (!shows(start.title, 'éteint')) {
      fail(`au départ, l’en-tête devait dire que le mode jeu est éteint : « ${start.title} »`);
    }
    const before = agentCount();
    if (before !== null && before !== 0) {
      fail(`le mode jeu est éteint, mais ${before} agent(s) tournent déjà`);
    }
    done('au départ, le mode jeu automatique est éteint');
  }
  await window.capture(startOn ? 'mode-jeu-allume-depart' : 'mode-jeu-eteint');

  const checkTitle = async (on) => {
    const now = await read();
    if (on && !(shows(now.title, 'veille') || shows(now.title, 'En jeu'))) {
      fail(`allumé, l’en-tête devait dire que Shinku veille : « ${now.title} »`);
    }
    if (!on && !shows(now.title, 'éteint')) {
      fail(`éteint, l’en-tête devait le dire : « ${now.title} »`);
    }
    return now;
  };

  try {
    for (const on of startOn ? [false, true] : [true, false]) {
      await (on ? turnOn() : turnOff());
      if (on) {
        // L'agent démarre, puis la page le revoit : le temps de deux sondages.
        await sleep(4500);
      }
      const now = await checkTitle(on);
      if (on) {
        if (now.warning !== '') {
          fail(`le mode jeu est allumé, mais la page avertit : « ${now.warning} »`);
        }
        // Une indication, pas une preuve : l'avertissement n'apparaît qu'après 15 s d'arrêt.
        done(`allumé : aucun avertissement affiché (indication seulement), l’en-tête dit « ${now.title} »`);
        await window.capture('mode-jeu-allume');
      }
    }
  } finally {
    // L'état de départ, quoi qu'il arrive.
    try {
      if ((await read()).on !== startOn) {
        await (startOn ? turnOn() : turnOff());
      }
    } catch {
      // La fenêtre a pu se fermer ; l'essai a déjà dit pourquoi il échoue.
    }
    if (!startOn && controlled && agentCount() > 0) {
      stopOwnAgent();
    }
  }
  done(startOn ? 'l’état de départ est restauré (mode jeu allumé)' : 'l’état de départ est restauré (mode jeu éteint, plus d’agent)');
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
  const planted = options['grand-vide'] ? plantedFiles() : null;
  if (planted !== null) {
    plant(planted);
  }
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
    await checkStartup(window);
    await checkGameMode(window);
    await checkSettings(window);
    if (options['grand-vide']) {
      await checkClean(window, planted);
    }
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
