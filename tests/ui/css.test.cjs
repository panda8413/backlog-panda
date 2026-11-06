const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');

// Les commentaires sont vidés (retours à la ligne conservés pour garder les numéros de ligne).
const css = fs
  .readFileSync(path.join(__dirname, '../../ui/app.css'), 'utf8')
  .replace(/\/\*[\s\S]*?\*\//g, (c) => c.replace(/[^\n]/g, ''));

// Retire chaque bloc `sélecteurs { … }` dont la liste de sélecteurs commence par `:root`
// (blocs de thème). Les blocs de thème ne contiennent pas d'accolades imbriquées :
// un balayage jusqu'à la première `}` suffit.
function stripRootBlocks(src) {
  // Contenu vidé, retours à la ligne conservés : les numéros de ligne restent ceux de app.css.
  return src.replace(/(^|(?<=\}))(\s*):root[^{]*\{[^}]*\}/g, (m, pre, ws) => pre + ws + m.slice((pre + ws).length).replace(/[^\n]/g, ''));
}

function block(selectorRe) {
  const m = css.match(new RegExp(String.raw`(?:^|\})\s*(${selectorRe})\s*\{([^}]*)\}`));
  return m ? m[2] : null;
}

test('aucune couleur en dur hors des blocs de thème', () => {
  const rest = stripRootBlocks(css);
  // Les dégradés des jaquettes de secours dérivent de la teinte `--h` du jeu : ce sont des
  // illustrations générées, identiques dans les deux thèmes, pas des couleurs de thème.
  const hueArt = /hsl\(\s*(?:calc\(\s*)?var\(--h\)/g;
  const bad = [];
  rest.split('\n').forEach((line, i) => {
    const l = line.replace(hueArt, '');
    if (/#[0-9a-fA-F]{3,8}\b|rgba?\(|hsla?\(/.test(l)) bad.push(`${i + 1}: ${line.trim()}`);
  });
  assert.deepEqual(bad, [], 'couleurs en dur :\n' + bad.join('\n'));
});

test('le bloc sombre porte les valeurs de la spec', () => {
  const dark = block(String.raw`:root,\s*:root\[data-theme="dark"\]`);
  assert.ok(dark, 'bloc `:root, :root[data-theme="dark"]` introuvable');
  const expected = {
    '--bg': '#0e1326',
    '--bg-rail': '#131a33',
    '--panel': '#1a2140',
    '--panel-2': '#222a50',
    '--dim': '#b0b6d4',
    '--faint': '#8c92b3',
  };
  for (const [name, value] of Object.entries(expected)) {
    const m = dark.match(new RegExp(String.raw`${name}:\s*([^;]+);`));
    assert.ok(m, `${name} absent`);
    assert.equal(m[1].trim().toLowerCase(), value, name);
  }
  assert.match(dark, /color-scheme:\s*dark;/);
});

test('chaque variable du bloc sombre est définie aussi dans le bloc clair', () => {
  const names = (b) => new Set([...b.matchAll(/(--[a-z0-9-]+):/g)].map((m) => m[1]));
  const dark = block(String.raw`:root,\s*:root\[data-theme="dark"\]`);
  const light = block(String.raw`:root\[data-theme="light"\]`);
  assert.ok(light, 'bloc clair introuvable');
  const missing = [...names(dark)].filter((n) => !names(light).has(n));
  assert.deepEqual(missing, []);
});

const LIGHT = String.raw`:root\[data-theme="light"\]`;
const DARK = String.raw`:root,\s*:root\[data-theme="dark"\]`;

function val(b, name) {
  const m = b.match(new RegExp(String.raw`${name}:\s*([^;]+);`));
  assert.ok(m, `${name} absent`);
  return m[1].trim().toLowerCase();
}

test('le bloc clair porte les valeurs de la spec', () => {
  const light = block(LIGHT);
  assert.ok(light, 'bloc clair introuvable');
  const expected = {
    '--bg': '#f7f5f2',
    '--bg-rail': '#efebe6',
    '--panel': '#ffffff',
    '--panel-2': '#f3efea',
    '--line': 'rgba(60, 40, 20, 0.10)',
    '--line-strong': 'rgba(60, 40, 20, 0.20)',
    '--text': '#1f1a16',
    '--dim': '#5e554c',
    '--faint': '#736860',
    '--bubble-bg': '#f6e3d3',
    '--bubble-border': '#e8bf9f',
    '--bubble-fg': '#8a4a22',
    '--accent': '#c8794a',
    '--star': '#d99a00',
    '--st-backlog': '#2f6fd6',
    '--st-playing': '#107956',
    '--st-completed': '#926100',
    '--st-played': '#6b4fd1',
    '--st-abandoned': '#c2344d',
    '--st-wishlist': '#c2408e',
  };
  for (const [name, value] of Object.entries(expected)) assert.equal(val(light, name), value.toLowerCase(), name);
  assert.match(light, /color-scheme:\s*light;/);
});

function lum(hex) {
  assert.match(hex, /^#[0-9a-f]{6}$/, `couleur hexadécimale à 6 chiffres attendue : ${hex}`);
  const n = parseInt(hex.slice(1), 16);
  const ch = [(n >> 16) & 255, (n >> 8) & 255, n & 255].map((v) => {
    const c = v / 255;
    return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * ch[0] + 0.7152 * ch[1] + 0.0722 * ch[2];
}
function contrast(a, b) {
  const [hi, lo] = [lum(a), lum(b)].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
}
function atLeast(b, fg, bg, min = 4.5) {
  const r = contrast(val(b, fg), val(b, bg));
  assert.ok(r >= min, `${fg} sur ${bg} : ${r.toFixed(2)}:1 < ${min}`);
}

test('contraste AA en clair', () => {
  const l = block(LIGHT);
  const bgs = ['--bg', '--panel', '--panel-2', '--bg-rail'];
  for (const fg of ['--text', '--dim', '--faint']) for (const bg of bgs) atLeast(l, fg, bg);
  atLeast(l, '--bubble-fg', '--bubble-bg');
  // Les couleurs d'état servent de texte (messages, compteurs) sur le fond comme sur les panneaux.
  for (const s of ['playing', 'completed', 'abandoned', 'played']) for (const bg of ['--bg', '--panel']) atLeast(l, `--st-${s}`, bg);
  for (const s of ['backlog', 'wishlist']) atLeast(l, `--st-${s}`, '--panel');
  // `.bk-mode em` prend --bubble-fg sur la bulle cochée (règle claire).
  assert.match(css, /\[data-theme="light"\] \.bk-mode:has\(input:checked\) em \{ color: var\(--bubble-fg\); \}/);
});

test('contraste AA en sombre', () => {
  const d = block(DARK);
  const bgs = ['--bg', '--panel', '--panel-2', '--bg-rail'];
  for (const fg of ['--text', '--dim', '--faint']) for (const bg of bgs) atLeast(d, fg, bg);
});
