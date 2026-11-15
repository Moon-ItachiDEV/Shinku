// Le faux cœur du mode jeu : il réagit vraiment aux gestes, et il rend ce que
// les paramètres de la démo annoncent.

import assert from 'node:assert/strict';
import { test } from 'node:test';

import { demoOptions, type GameDemo, mockBackend } from '../src/backend/mock.ts';
import type { Backend, BackendError, GameRefusal } from '../src/backend/types.ts';

const HOLLOW_PEAKS = 'D:\\SteamLibrary\\steamapps\\common\\Hollow Peaks\\hp.exe';

/** Un faux cœur de démo, avec les paramètres d'adresse donnés. */
function demo(query = ''): Backend {
  return mockBackend(demoOptions(query));
}

async function refusal(promise: Promise<unknown>): Promise<GameRefusal | null> {
  try {
    await promise;
    return null;
  } catch (error) {
    const refused = error as Partial<Extract<BackendError, { kind: 'gameRefused' }>>;
    assert.equal(refused.kind, 'gameRefused');
    return refused.reason ?? null;
  }
}

test('les paramètres d’adresse de la démo sont lus, et le reste prend les valeurs par défaut', () => {
  const expected = (parts: Partial<GameDemo>): GameDemo => ({
    state: 'off',
    agent: 'running',
    power: 'available',
    startup: 'enabled',
    ...parts,
  });
  assert.deepEqual(demoOptions('').game, expected({}));
  assert.deepEqual(demoOptions('?game=playing').game, expected({ state: 'playing' }));
  assert.deepEqual(demoOptions('?game=pending&agent=stopped').game, expected({ state: 'pending', agent: 'stopped' }));
  assert.deepEqual(demoOptions('?agent=missing&power=battery').game, expected({ agent: 'missing', power: 'battery' }));
  assert.deepEqual(demoOptions('?power=unsuited&startup=disabled').game, expected({ power: 'unsuited', startup: 'disabled' }));
  // Une valeur inconnue ne change rien.
  assert.deepEqual(demoOptions('?game=xyz&agent=xyz&power=xyz&startup=xyz').game, expected({}));
});

test('au départ, le mode jeu est éteint et rien n’est en cours', async () => {
  const view = await demo().gameView();
  assert.equal(view.auto, false);
  assert.equal(view.session, null);
  assert.equal(view.agent, 'stopped');
  assert.ok(view.last !== null && view.last.complete);
  // Hollow Peaks, deux heures dix.
  assert.equal(view.last.name, 'Hollow Peaks');
  assert.equal(view.last.ended - view.last.started, 130 * 60_000);
});

test('chaque état de la démo rend la vue qu’il annonce', async () => {
  const idle = await demo('?game=idle').gameView();
  assert.ok(idle.auto && idle.session === null && idle.agent === 'running');

  const playing = await demo('?game=playing').gameView();
  assert.equal(playing.session?.name, 'Hollow Peaks');
  assert.equal(playing.session?.source, 'library');
  assert.equal(playing.session?.power, 'applied');
  assert.equal(playing.session?.quietedApps, 1);
  assert.equal(playing.session?.released, false);
  assert.equal(playing.session?.restorePending, false);

  const released = await demo('?game=released').gameView();
  assert.equal(released.session?.released, true);

  const pending = await demo('?game=pending').gameView();
  assert.equal(pending.session?.restorePending, true);
});

test('l’agent, l’alimentation et le démarrage suivent les paramètres', async () => {
  assert.equal((await demo('?game=idle&agent=missing').gameView()).agent, 'missing');
  assert.equal((await demo('?game=idle&agent=stopped').gameView()).agent, 'stopped');
  assert.equal((await demo('?game=idle&startup=disabled').gameView()).startupDisabled, true);
  assert.equal((await demo('?game=idle').gameView()).startupDisabled, false);

  const unavailable = await demo('?game=playing&power=unavailable').gameView();
  assert.equal(unavailable.powerSupport, 'unavailable');
  assert.equal(unavailable.session?.power, 'unavailable');
  const unsuited = await demo('?game=playing&power=unsuited').gameView();
  assert.equal(unsuited.powerSupport, 'unsuited');
  assert.equal(unsuited.session?.power, 'unsuited');
  const battery = await demo('?game=playing&power=battery').gameView();
  assert.equal(battery.onBattery, true);
  assert.equal(battery.powerSupport, 'available');
  assert.equal(battery.session?.power, 'onBattery');
});

test('les interrupteurs répondent, et les réglages de la vue d’ensemble les suivent', async () => {
  const backend = demo();
  assert.equal((await backend.setGameOption('auto', true)).auto, true);
  assert.equal((await backend.gameView()).agent, 'running');
  assert.equal((await backend.setGameOption('power', false)).power, false);
  assert.equal((await backend.setGameOption('quietApps', false)).quietApps, false);
  const settings = (await backend.overview()).settings;
  assert.equal(settings.gameModeAuto, true);
  assert.equal(settings.gameModePower, false);
  assert.equal(settings.gameModeQuietApps, false);
  assert.equal('gameModeOnLaunch' in settings, false);
});

test('éteindre le mode automatique pendant une partie la termine et note le bilan', async () => {
  const backend = demo('?game=playing');
  const view = await backend.setGameOption('auto', false);
  assert.equal(view.session, null);
  assert.equal(view.agent, 'stopped');
  assert.equal(view.last?.name, 'Hollow Peaks');
  assert.equal(view.last?.complete, true);
  assert.equal(view.last?.powerRestored, true);
});

test('l’alimentation éteinte dit « off » à la partie en cours', async () => {
  const backend = demo('?game=playing');
  const view = await backend.setGameOption('power', false);
  assert.equal(view.session?.power, 'off');
});

test('les apps de la démo : intégrées cochées d’office, navigateurs décochés, une app ajoutée', async () => {
  const { apps } = await demo().gameView();
  const by = (exe: string) => apps.find((entry) => entry.exe === exe);
  assert.deepEqual([by('onedrive.exe')?.on, by('onedrive.exe')?.running], [true, true]);
  assert.deepEqual([by('chrome.exe')?.on, by('chrome.exe')?.running], [false, true]);
  assert.deepEqual([by('dropbox.exe')?.on, by('dropbox.exe')?.running], [true, false]);
  assert.deepEqual([by('googledrivefs.exe')?.on, by('googledrivefs.exe')?.running], [true, false]);
  assert.equal(by('qbittorrent.exe')?.label, 'qBittorrent');
  assert.equal(by('qbittorrent.exe')?.builtIn, false);
  assert.equal(by('qbittorrent.exe')?.on, true);
});

test('cocher et décocher une app', async () => {
  const backend = demo();
  const checked = await backend.setQuietApp('chrome.exe', true);
  assert.equal(checked.apps.find((entry) => entry.exe === 'chrome.exe')?.on, true);
  assert.deepEqual((await backend.overview()).settings.gameModeQuietAdded.sort(), ['chrome.exe', 'qbittorrent.exe']);

  const unchecked = await backend.setQuietApp('onedrive.exe', false);
  assert.equal(unchecked.apps.find((entry) => entry.exe === 'onedrive.exe')?.on, false);
  assert.deepEqual((await backend.overview()).settings.gameModeQuietRemoved, ['onedrive.exe']);

  const back = await backend.setQuietApp('onedrive.exe', true);
  assert.equal(back.apps.find((entry) => entry.exe === 'onedrive.exe')?.on, true);
  assert.deepEqual((await backend.overview()).settings.gameModeQuietRemoved, []);
});

test('décocher une app ajoutée la retire de la liste', async () => {
  const backend = demo();
  const view = await backend.setQuietApp('qbittorrent.exe', false);
  assert.ok(!view.apps.some((entry) => entry.exe === 'qbittorrent.exe'));
});

test('une app inconnue est refusée, et rien ne change', async () => {
  const backend = demo();
  assert.equal(await refusal(backend.setQuietApp('inconnue.exe', true)), 'unknownApp');
  assert.deepEqual((await backend.overview()).settings.gameModeQuietAdded, ['qbittorrent.exe']);
});

test('le sélecteur liste quatre apps ouvertes', async () => {
  const apps = await demo().openApps();
  assert.equal(apps.length, 4);
  assert.deepEqual(
    apps.map((entry) => entry.id),
    [1, 2, 3, 4],
  );
});

test('ajouter un jeu ouvert, puis le retirer', async () => {
  const backend = demo();
  const [roblox] = await backend.openApps();
  assert.ok(roblox !== undefined);
  const added = await backend.addGame(roblox.id);
  const game = added.games.find((entry) => entry.path === roblox.path);
  assert.equal(game?.source, 'added');
  assert.equal(game?.name, 'Roblox');
  assert.ok((await backend.overview()).settings.gameModeGames.includes(roblox.path));

  const removed = await backend.setGameStatus(roblox.path, 'forgotten');
  assert.ok(!removed.games.some((entry) => entry.path === roblox.path));
  assert.ok(removed.notGames.length === 0);
});

test('un id qui n’est pas dans la dernière liste est refusé, comme un programme sous veto', async () => {
  const backend = demo();
  // Sans liste demandée, aucun id ne vaut.
  assert.equal(await refusal(backend.addGame(1)), 'unknownApp');
  assert.equal(await refusal(backend.addQuietApp(1)), 'unknownApp');
  await backend.openApps();
  assert.equal(await refusal(backend.addGame(99)), 'unknownApp');
  // Battle.net est un lanceur : jamais un jeu.
  assert.equal(await refusal(backend.addGame(4)), 'notAGame');
});

test('ajouter une app en veilleuse depuis le sélecteur', async () => {
  const backend = demo();
  await backend.openApps();
  const view = await backend.addQuietApp(2);
  const notepad = view.apps.find((entry) => entry.exe === 'notepad++.exe');
  assert.equal(notepad?.label, 'Notepad++');
  assert.equal(notepad?.on, true);
  assert.equal(notepad?.builtIn, false);
});

test('« Ce n’est pas un jeu » écarte un jeu vu, et « Rétablir » le ramène', async () => {
  const backend = demo();
  const refused = await backend.setGameStatus(HOLLOW_PEAKS, 'notGame');
  assert.ok(!refused.games.some((entry) => entry.path === HOLLOW_PEAKS));
  assert.deepEqual(refused.notGames, [{ path: HOLLOW_PEAKS, name: 'Hollow Peaks' }]);

  const restored = await backend.setGameStatus(HOLLOW_PEAKS, 'forgotten');
  assert.equal(restored.notGames.length, 0);
  assert.equal(restored.games.find((entry) => entry.path === HOLLOW_PEAKS)?.source, 'library');
});

test('la liste des jeux met les jeux ajoutés avant les jeux vus', async () => {
  const { games } = await demo().gameView();
  assert.deepEqual(
    games.map((entry) => [entry.name, entry.source]),
    [
      ['Minecraft', 'added'],
      ['Hollow Peaks', 'library'],
    ],
  );
  assert.equal(games[0]?.lastPlayed, null);
  assert.ok((games[1]?.lastPlayed ?? 0) > 0);
});

test('un chemin que le cœur ne connaît pas est refusé', async () => {
  assert.equal(await refusal(demo().setGameStatus('C:\\Windows\\System32\\notepad.exe', 'notGame')), 'unknownGame');
});

test('dire que le jeu en cours n’en est pas un termine la partie', async () => {
  const backend = demo('?game=playing');
  const view = await backend.setGameStatus(HOLLOW_PEAKS, 'notGame');
  assert.equal(view.session, null);
  assert.equal(view.notGames.length, 1);
  assert.ok(view.last !== null);
});

test('« Tout remettre maintenant » remet en place sans finir la partie', async () => {
  const backend = demo('?game=playing');
  const view = await backend.releaseGame();
  assert.equal(view.session?.released, true);
  assert.equal(view.session?.quietedApps, 0);
  assert.equal(view.session?.restorePending, false);
});

test('relâcher une remise en attente la remet en place', async () => {
  const view = await demo('?game=pending').releaseGame();
  assert.equal(view.session?.restorePending, false);
  assert.equal(view.session?.released, true);
});

test('relâcher sans partie ne change rien', async () => {
  const view = await demo('?game=idle').releaseGame();
  assert.equal(view.session, null);
});

test('les réglages de Windows s’ouvrent sans erreur', async () => {
  const backend = demo();
  await backend.openWindowsSettings('gameMode');
  await backend.openWindowsSettings('power');
});
