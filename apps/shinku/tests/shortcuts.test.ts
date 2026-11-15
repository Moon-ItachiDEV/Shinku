import assert from 'node:assert/strict';
import { test } from 'node:test';

import { isBrowserShortcut, type Keystroke } from '../src/shortcuts.ts';

function stroke(key: string, modifiers: Partial<Keystroke> = {}): Keystroke {
  return { key, ctrlKey: false, altKey: false, ...modifiers };
}

test('les raccourcis du navigateur sont reconnus', () => {
  const browser: Keystroke[] = [
    stroke('F5'),
    stroke('F5', { ctrlKey: true }),
    stroke('F3'),
    stroke('F7'),
    stroke('F12'),
    stroke('BrowserBack'),
    stroke('r', { ctrlKey: true }),
    stroke('R', { ctrlKey: true }),
    stroke('p', { ctrlKey: true }),
    stroke('f', { ctrlKey: true }),
    stroke('u', { ctrlKey: true }),
    stroke('ArrowLeft', { altKey: true }),
    stroke('ArrowRight', { altKey: true }),
  ];
  for (const one of browser) {
    assert.equal(isBrowserShortcut(one), true, JSON.stringify(one));
  }
});

test('les raccourcis d’édition et la navigation au clavier restent libres', () => {
  const free: Keystroke[] = [
    stroke('c', { ctrlKey: true }),
    stroke('v', { ctrlKey: true }),
    stroke('x', { ctrlKey: true }),
    stroke('a', { ctrlKey: true }),
    stroke('z', { ctrlKey: true }),
    stroke('y', { ctrlKey: true }),
    stroke('r'),
    stroke('f'),
    stroke('Tab'),
    stroke('Enter'),
    stroke(' '),
    stroke('Escape'),
    stroke('ArrowLeft'),
    stroke('ArrowRight'),
    stroke('Home'),
    stroke('F4', { altKey: true }),
    stroke('F6'),
  ];
  for (const one of free) {
    assert.equal(isBrowserShortcut(one), false, JSON.stringify(one));
  }
});
