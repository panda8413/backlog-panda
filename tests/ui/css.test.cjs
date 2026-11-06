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
  return src.replace(/(^|\})(\s*):root[^{]*\{[^}]*\}/g, (m, pre, ws) => pre + ws + m.slice((pre + ws).length).replace(/[^\n]/g, ''));
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
    '--faint': '#7f86aa',
  };
  for (const [name, value] of Object.entries(expected)) {
    const m = dark.match(new RegExp(String.raw`${name}:\s*([^;]+);`));
    assert.ok(m, `${name} absent`);
    assert.equal(m[1].trim().toLowerCase(), value, name);
  }
  assert.match(dark, /color-scheme:\s*dark;/);
});

test('chaque variable du bloc sombre est définie aussi dans le bloc clair', { todo: 'Tâche 3' }, () => {
  const names = (b) => new Set([...b.matchAll(/(--[a-z0-9-]+):/g)].map((m) => m[1]));
  const dark = block(String.raw`:root,\s*:root\[data-theme="dark"\]`);
  const light = block(String.raw`:root\[data-theme="light"\]`);
  assert.ok(light, 'bloc clair introuvable');
  const missing = [...names(dark)].filter((n) => !names(light).has(n));
  assert.deepEqual(missing, []);
});
